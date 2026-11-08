//! glTF 2.0: binary `.glb` export, `.glb` and `.gltf` import. glTF is in metres with Y up;
//! Gusset is in millimetres with Z up, so positions are scaled by 1000 and turned about X
//! (export: `(x, y, z)` → `(x, z, -y)` / 1000; import: the inverse). Import walks the
//! default scene's node tree with its transforms (matrix or TRS) and reads triangle
//! primitives (`POSITION` and optional indices) as one mesh per glTF mesh instance. Only
//! the GLB binary chunk and base64 `data:` URIs are read: a `.gltf` file's external buffer
//! files are refused, so importing never reads other files. Sparse accessors, Draco and
//! meshopt compression are refused too.

use crate::error::{limit, malformed};
use crate::utils::Tri;
use crate::{
    Cancel, ExportBody, ExportReport, Format, ImportOptions, ImportedMesh, MeshIoError, Result, Soup, clean_name,
    finish,
};
use serde_json::{Value, json};
use std::io::Write;

const F: Format = Format::Gltf;
const UNITS_NOTE: &str = "glTF metres converted to mm (x1000), Y up turned to Z up";
const MAGIC: &[u8; 4] = b"glTF";
/// Node tree depth limit (cycles and deep trees in hostile files).
const MAX_DEPTH: usize = 64;

pub(crate) fn looks_like_gltf(b: &[u8]) -> bool {
    b.starts_with(MAGIC) || {
        let head = b.get(..b.len().min(4096)).unwrap_or(&[]);
        let t = String::from_utf8_lossy(head);
        t.trim_start().starts_with('{') && t.contains("\"asset\"")
    }
}

type Mat = [f64; 16];
const IDENTITY: Mat = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// Column-major 4x4 product `a * b`.
fn mul(a: &Mat, b: &Mat) -> Mat {
    std::array::from_fn(|k| {
        let (c, r) = (k / 4, k % 4);
        (0..4).map(|i| a.get(i * 4 + r).copied().unwrap_or(0.0) * b.get(c * 4 + i).copied().unwrap_or(0.0)).sum()
    })
}

fn apply(m: &Mat, p: [f64; 3]) -> [f64; 3] {
    let g = |k: usize| m.get(k).copied().unwrap_or(0.0);
    std::array::from_fn(|r| g(r) * p[0] + g(4 + r) * p[1] + g(8 + r) * p[2] + g(12 + r))
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

/// A node's local transform: `matrix`, or translation * rotation * scale.
fn local(node: &Value) -> Result<Mat> {
    if let Some(m) = node.get("matrix").and_then(Value::as_array) {
        let v: Vec<f64> = m.iter().filter_map(num).collect();
        return v.try_into().map_err(|_| malformed(F, "node", "matrix is not 16 numbers"));
    }
    let vec = |key: &str, n: usize, default: &[f64]| -> Result<Vec<f64>> {
        match node.get(key).and_then(Value::as_array) {
            Some(a) => {
                let v: Vec<f64> = a.iter().filter_map(num).collect();
                if v.len() == n { Ok(v) } else { Err(malformed(F, "node", format!("{key} is not {n} numbers"))) }
            }
            None => Ok(default.to_vec()),
        }
    };
    let t = vec("translation", 3, &[0.0, 0.0, 0.0])?;
    let q = vec("rotation", 4, &[0.0, 0.0, 0.0, 1.0])?;
    let s = vec("scale", 3, &[1.0, 1.0, 1.0])?;
    let (&[tx, ty, tz], &[x, y, z, w], &[sx, sy, sz]) = (t.as_slice(), q.as_slice(), s.as_slice()) else {
        return Ok(IDENTITY);
    };
    let r = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    let g = |k: usize| r.get(k).copied().unwrap_or(0.0);
    Ok([
        g(0) * sx,
        g(1) * sx,
        g(2) * sx,
        0.0,
        g(3) * sy,
        g(4) * sy,
        g(5) * sy,
        0.0,
        g(6) * sz,
        g(7) * sz,
        g(8) * sz,
        0.0,
        tx,
        ty,
        tz,
        1.0,
    ])
}

/// Decodes standard base64 (padding optional); `None` on any other character.
fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| *c != b'=' && !c.is_ascii_whitespace()) {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

struct Doc<'a> {
    json: Value,
    buffers: Vec<std::borrow::Cow<'a, [u8]>>,
}

fn parse<'a>(b: &'a [u8], opts: &ImportOptions) -> Result<Doc<'a>> {
    let (json_bytes, bin) = if b.starts_with(MAGIC) {
        let word = |at: usize| match b.get(at..at + 4) {
            Some(&[a, b1, c, d]) => Some(u32::from_le_bytes([a, b1, c, d]) as usize),
            _ => None,
        };
        if word(4) != Some(2) {
            return Err(malformed(F, "GLB header", "not glTF version 2"));
        }
        let total = word(8).ok_or_else(|| malformed(F, "GLB header", "truncated"))?;
        if total > b.len() {
            return Err(malformed(F, "GLB header", "declares more bytes than the file has"));
        }
        let (mut at, mut json, mut bin) = (12usize, None, None);
        while at + 8 <= total {
            let (Some(len), Some(kind)) = (word(at), word(at + 4)) else { break };
            let chunk = b.get(at + 8..at + 8 + len).ok_or_else(|| malformed(F, "GLB chunk", "truncated"))?;
            match kind {
                0x4E4F_534A => json = json.or(Some(chunk)),
                0x004E_4942 => bin = bin.or(Some(chunk)),
                _ => {}
            }
            at = at + 8 + len.div_ceil(4) * 4;
        }
        (json.ok_or_else(|| malformed(F, "GLB", "no JSON chunk"))?, bin)
    } else {
        (b, None)
    };
    if json_bytes.len() as u64 > opts.limits.max_file_bytes {
        return Err(limit("JSON bytes", json_bytes.len() as u64, opts.limits.max_file_bytes));
    }
    let json: Value = serde_json::from_slice(json_bytes).map_err(|e| malformed(F, "JSON", e.to_string()))?;
    if let Some(ext) = json.get("extensionsRequired").and_then(Value::as_array).and_then(|a| a.first()) {
        return Err(MeshIoError::Unsupported { format: F, detail: format!("required extension {ext}") });
    }
    let mut buffers = Vec::new();
    for (i, buf) in json.get("buffers").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let data: std::borrow::Cow<'_, [u8]> = match buf.get("uri").and_then(Value::as_str) {
            None => bin
                .map(std::borrow::Cow::Borrowed)
                .ok_or_else(|| malformed(F, format!("buffer {i}"), "no GLB binary chunk"))?,
            Some(uri) => {
                let payload =
                    uri.strip_prefix("data:").and_then(|r| r.split_once(";base64,")).map(|(_, d)| d).ok_or_else(
                        || MeshIoError::Unsupported {
                            format: F,
                            detail: format!(
                                "buffer {i} is an external file; only embedded (data: URI or GLB) buffers are read"
                            ),
                        },
                    )?;
                std::borrow::Cow::Owned(
                    base64(payload).ok_or_else(|| malformed(F, format!("buffer {i}"), "bad base64"))?,
                )
            }
        };
        buffers.push(data);
    }
    Ok(Doc { json, buffers })
}

impl Doc<'_> {
    fn get(&self, kind: &str, i: usize) -> Result<&Value> {
        self.json
            .get(kind)
            .and_then(|a| a.get(i))
            .ok_or_else(|| malformed(F, kind.to_string(), format!("no entry {i}")))
    }

    /// An accessor's elements as f64 rows of `width` components.
    fn accessor(&self, i: usize, width: usize, max: u64) -> Result<Vec<f64>> {
        let a = self.get("accessors", i)?;
        let loc = || format!("accessor {i}");
        if a.get("sparse").is_some() {
            return Err(MeshIoError::Unsupported { format: F, detail: "sparse accessors".into() });
        }
        let count = a.get("count").and_then(Value::as_u64).ok_or_else(|| malformed(F, loc(), "no count"))?;
        if count > max {
            return Err(limit("accessor elements", count, max));
        }
        let ty = a.get("componentType").and_then(Value::as_u64).unwrap_or(0);
        let size = match ty {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(malformed(F, loc(), format!("component type {ty}"))),
        };
        let view_i = a.get("bufferView").and_then(Value::as_u64).ok_or_else(|| malformed(F, loc(), "no bufferView"))?;
        let view = self.get("bufferViews", view_i as usize)?;
        let buf_i = view.get("buffer").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize;
        let buf = self.buffers.get(buf_i).ok_or_else(|| malformed(F, loc(), "bad buffer"))?;
        let off = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize
            + a.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let stride = view.get("byteStride").and_then(Value::as_u64).map_or(size * width, |s| s as usize);
        let view_len = view.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as usize;
        let view_end = (view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize).saturating_add(view_len);
        let count = count as usize;
        if count > 0
            && off.saturating_add(stride.saturating_mul(count - 1)).saturating_add(size * width)
                > view_end.min(buf.len())
        {
            return Err(malformed(F, loc(), "reads past its buffer view"));
        }
        let mut out = Vec::with_capacity(count * width);
        for e in 0..count {
            for c in 0..width {
                let at = off + e * stride + c * size;
                let raw = buf.get(at..at + size).ok_or_else(|| malformed(F, loc(), "out of range"))?;
                out.push(match (ty, raw) {
                    (5120, [x]) => f64::from(*x as i8),
                    (5121, [x]) => f64::from(*x),
                    (5122, [x, y]) => f64::from(i16::from_le_bytes([*x, *y])),
                    (5123, [x, y]) => f64::from(u16::from_le_bytes([*x, *y])),
                    (5125, [x, y, z, w]) => f64::from(u32::from_le_bytes([*x, *y, *z, *w])),
                    (5126, [x, y, z, w]) => f64::from(f32::from_le_bytes([*x, *y, *z, *w])),
                    _ => 0.0,
                });
            }
        }
        Ok(out)
    }
}

pub(crate) fn import(
    b: &[u8],
    opts: &ImportOptions,
    cancel: Cancel<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ImportedMesh>> {
    let doc = parse(b, opts)?;
    let scene_i = doc.json.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
    let roots: Vec<usize> = match doc.json.get("scenes").and_then(|s| s.get(scene_i)) {
        Some(s) => s
            .get("nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .map(|n| n as usize)
            .collect(),
        // No scene: every node that is nobody's child.
        None => {
            let n = doc.json.get("nodes").and_then(Value::as_array).map_or(0, Vec::len);
            let children: std::collections::BTreeSet<u64> = doc
                .json
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(|nd| {
                    nd.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64)
                })
                .collect();
            (0..n).filter(|i| !children.contains(&(*i as u64))).collect()
        }
    };
    // Y-up metres to Z-up millimetres.
    let to_gusset: Mat = [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, -1000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let mut stack: Vec<(usize, Mat, usize)> = roots.iter().rev().map(|&r| (r, to_gusset, 0)).collect();
    let (mut out, mut triangles, mut visited) = (Vec::new(), 0u64, 0u64);
    let base = opts.name_hint.as_deref().map(|n| clean_name(n, opts.limits.max_name_bytes)).unwrap_or_default();
    while let Some((ni, parent, depth)) = stack.pop() {
        visited += 1;
        cancel.tick(visited as usize)?;
        if depth > MAX_DEPTH || visited > opts.limits.max_objects {
            return Err(limit("node tree size", visited, opts.limits.max_objects));
        }
        let node = doc.get("nodes", ni)?;
        let m = mul(&parent, &local(node)?);
        for c in node.get("children").and_then(Value::as_array).into_iter().flatten().rev() {
            let c = c.as_u64().ok_or_else(|| malformed(F, format!("node {ni}"), "bad child"))? as usize;
            stack.push((c, m, depth + 1));
        }
        let Some(mi) = node.get("mesh").and_then(Value::as_u64) else { continue };
        let mesh = doc.get("meshes", mi as usize)?;
        let mut soup = Soup::default();
        for (pi, prim) in mesh.get("primitives").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let mode = prim.get("mode").and_then(Value::as_u64).unwrap_or(4);
            if mode != 4 {
                warnings.push(format!("mesh {mi} primitive {pi}: mode {mode} is not triangles, skipped"));
                continue;
            }
            if prim.get("extensions").is_some() {
                return Err(MeshIoError::Unsupported { format: F, detail: "compressed primitives".into() });
            }
            let pos_i = prim.get("attributes").and_then(|a| a.get("POSITION")).and_then(Value::as_u64);
            let Some(pos_i) = pos_i else { continue };
            let pos = doc.accessor(pos_i as usize, 3, opts.limits.max_vertices)?;
            let base_v = soup.positions.len() as u32;
            for p in pos.chunks_exact(3) {
                let &[x, y, z] = p else { continue };
                let w = apply(&m, [x, y, z]);
                if w.iter().any(|c| !c.is_finite()) {
                    return Err(malformed(F, format!("mesh {mi}"), "a vertex is not finite"));
                }
                soup.positions.push(w.map(|c| c as f32));
            }
            let nv = pos.len() / 3;
            let idx: Vec<u32> = match prim.get("indices").and_then(Value::as_u64) {
                Some(ii) => doc
                    .accessor(ii as usize, 1, opts.limits.max_triangles.saturating_mul(3))?
                    .iter()
                    .map(|&v| v as u32)
                    .collect(),
                None => (0..nv as u32).collect(),
            };
            if let Some(bad) = idx.iter().find(|&&i| i as usize >= nv) {
                return Err(malformed(F, format!("mesh {mi}"), format!("index {bad} but {nv} vertices")));
            }
            triangles += (idx.len() / 3) as u64;
            if triangles > opts.limits.max_triangles {
                return Err(limit("triangle count", triangles, opts.limits.max_triangles));
            }
            // A mirroring transform turns the triangles inside out: flip them back.
            let g = |k: usize| m.get(k).copied().unwrap_or(0.0);
            let det = g(0) * (g(5) * g(10) - g(9) * g(6)) - g(4) * (g(1) * g(10) - g(9) * g(2))
                + g(8) * (g(1) * g(6) - g(5) * g(2));
            for t in idx.chunks_exact(3) {
                let &[a, b1, c] = t else { continue };
                let tri: Tri = if det < 0.0 { [a, c, b1] } else { [a, b1, c] };
                soup.tris.push(tri.map(|i| i + base_v));
            }
        }
        if soup.tris.is_empty() {
            continue;
        }
        soup.ranges = crate::one_range(soup.tris.len() as u32);
        let name = [node.get("name"), mesh.get("name")]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|n| clean_name(n, opts.limits.max_name_bytes))
            .find(|n| !n.is_empty())
            .unwrap_or_else(|| {
                if base.is_empty() {
                    format!("glTF mesh {}", out.len() + 1)
                } else {
                    format!("{base} {}", out.len() + 1)
                }
            });
        // The first primitive's material colour (ADR 0045): glTF factors are linear.
        let color = mesh
            .get("primitives")
            .and_then(Value::as_array)
            .and_then(|p| p.iter().find_map(|q| q.get("material").and_then(Value::as_u64)))
            .and_then(|i| doc.json.get("materials").and_then(|m| m.get(i as usize)))
            .and_then(|m| m.get("pbrMetallicRoughness")?.get("baseColorFactor")?.as_array().cloned())
            .and_then(|f| {
                let srgb = |x: &Value| {
                    let v = x.as_f64()?.clamp(0.0, 1.0);
                    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * libm::pow(v, 1.0 / 2.4) - 0.055 };
                    Some((s * 255.0).round() as u32)
                };
                Some((srgb(f.first()?)? << 16) | (srgb(f.get(1)?)? << 8) | srgb(f.get(2)?)?)
            });
        let mut m = finish(name, soup, UNITS_NOTE, opts, warnings);
        m.color = color;
        out.push(m);
    }
    Ok(out)
}

/// A binary glTF (`.glb`): one node and mesh per body, positions in metres, Y up.
pub(crate) fn write(bodies: &[ExportBody<'_>], out: &mut dyn Write) -> Result<ExportReport> {
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut meshes, mut nodes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    // One material per distinct body colour (ADR 0045); glTF wants linear factors.
    let mut colours: Vec<u32> = Vec::new();
    let mut materials = Vec::new();
    let mut triangles = 0u64;
    for (k, b) in bodies.iter().enumerate() {
        let pts: Vec<[f32; 3]> =
            b.mesh.positions.iter().map(|p| [p[0] / 1000.0, p[2] / 1000.0, -p[1] / 1000.0]).collect();
        if pts.is_empty() || b.mesh.indices.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &pts {
            lo = std::array::from_fn(|i| lo.get(i).copied().unwrap_or(0.0).min(p.get(i).copied().unwrap_or(0.0)));
            hi = std::array::from_fn(|i| hi.get(i).copied().unwrap_or(0.0).max(p.get(i).copied().unwrap_or(0.0)));
        }
        let start = bin.len();
        for p in &pts {
            for c in p {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "target": 34962}));
        accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5126, "count": pts.len(),
            "type": "VEC3", "min": lo, "max": hi}));
        let start = bin.len();
        for i in b.mesh.indices.iter() {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "target": 34963}));
        accessors.push(json!({"bufferView": views.len() - 1, "componentType": 5125,
            "count": b.mesh.indices.len(), "type": "SCALAR"}));
        let mut prim = json!({"attributes": {"POSITION": accessors.len() - 2},
            "indices": accessors.len() - 1, "mode": 4});
        if let Some(c) = b.color.map(|c| c & 0xff_ffff) {
            let i = match colours.iter().position(|x| *x == c) {
                Some(i) => i,
                None => {
                    let lin = |v: u32| {
                        let x = f64::from(v & 255) / 255.0;
                        if x <= 0.04045 { x / 12.92 } else { libm::pow((x + 0.055) / 1.055, 2.4) }
                    };
                    colours.push(c);
                    materials.push(json!({"name": format!("#{c:06x}"), "pbrMetallicRoughness": {
                        "baseColorFactor": [lin(c >> 16), lin(c >> 8), lin(c), 1.0],
                        "metallicFactor": 0.0, "roughnessFactor": 0.5}}));
                    colours.len() - 1
                }
            };
            if let Some(o) = prim.as_object_mut() {
                o.insert("material".into(), json!(i));
            }
        }
        meshes.push(json!({"name": b.name, "primitives": [prim]}));
        nodes.push(json!({"name": b.name, "mesh": meshes.len() - 1}));
        triangles += (b.mesh.indices.len() / 3) as u64;
        let _ = k;
    }
    let doc = json!({
        "asset": {"version": "2.0", "generator": "Gusset"},
        "scene": 0,
        "scenes": [{"nodes": (0..nodes.len()).collect::<Vec<_>>()}],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}],
    });
    let mut doc = doc;
    if !materials.is_empty()
        && let Some(o) = doc.as_object_mut()
    {
        o.insert("materials".into(), json!(materials));
    }
    let mut json_bytes =
        serde_json::to_vec(&doc).map_err(|e| MeshIoError::Unsupported { format: F, detail: e.to_string() })?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let total =
        u32::try_from(total).map_err(|_| MeshIoError::Unsupported { format: F, detail: "over 4 GiB".into() })?;
    out.write_all(MAGIC)?;
    out.write_all(&2u32.to_le_bytes())?;
    out.write_all(&total.to_le_bytes())?;
    out.write_all(&(json_bytes.len() as u32).to_le_bytes())?;
    out.write_all(&0x4E4F_534Au32.to_le_bytes())?;
    out.write_all(&json_bytes)?;
    out.write_all(&(bin.len() as u32).to_le_bytes())?;
    out.write_all(&0x004E_4942u32.to_le_bytes())?;
    out.write_all(&bin)?;
    Ok(ExportReport { warnings: Vec::new(), triangles })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExportFormat, export_to_vec, import_bytes};
    use gusset_kernel::Mesh;

    fn tetra() -> Mesh {
        Mesh {
            positions: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
            indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
            ..Default::default()
        }
    }

    #[test]
    fn body_colours_become_materials() {
        let t = tetra();
        let (bytes, _) = export_to_vec(
            &[
                ExportBody { name: "a", mesh: &t, color: Some(0x00ff_0000), face_colors: &[] },
                ExportBody { name: "b", mesh: &t, color: Some(0x00ff_0000), face_colors: &[] },
                ExportBody { name: "c", mesh: &t, color: None, face_colors: &[] },
            ],
            ExportFormat::Glb,
        )
        .unwrap();
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: Value = serde_json::from_slice(&bytes[20..20 + len]).unwrap();
        assert_eq!(doc["materials"].as_array().unwrap().len(), 1, "{doc}");
        assert_eq!(doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"], json!([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(doc["meshes"][1]["primitives"][0]["material"], 0);
        assert!(doc["meshes"][2]["primitives"][0].get("material").is_none());
        let back = import_bytes(&bytes, None, &ImportOptions::default()).unwrap();
        assert_eq!(
            back.meshes.iter().map(|m| m.color).collect::<Vec<_>>(),
            [Some(0x00ff_0000), Some(0x00ff_0000), None]
        );
    }

    #[test]
    fn glb_round_trip_keeps_size_orientation_and_names() {
        let t = tetra();
        let (bytes, r) = export_to_vec(
            &[
                ExportBody { name: "tetra", mesh: &t, color: None, face_colors: &[] },
                ExportBody { name: "two", mesh: &t, color: None, face_colors: &[] },
            ],
            ExportFormat::Glb,
        )
        .unwrap();
        assert_eq!(r.triangles, 8);
        assert!(bytes.starts_with(b"glTF"));
        let m = import_bytes(&bytes, None, &ImportOptions::default()).unwrap();
        assert_eq!(m.format, Format::Gltf);
        let names: Vec<&str> = m.meshes.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["tetra", "two"]);
        let v = &m.meshes[0].validation;
        assert!(v.can_become_solid() && (v.signed_volume - 1000.0 / 6.0).abs() < 1e-2, "{v:?}");
        // Z up survives the trip: the apex is still at z = 10.
        let top = m.meshes[0].mesh.positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!((top - 10.0).abs() < 1e-4, "{top}");
        assert_eq!(
            export_to_vec(&[ExportBody { name: "tetra", mesh: &t, color: None, face_colors: &[] }], ExportFormat::Glb)
                .unwrap()
                .0
                .len(),
            {
                let (b, _) = export_to_vec(
                    &[ExportBody { name: "tetra", mesh: &t, color: None, face_colors: &[] }],
                    ExportFormat::Glb,
                )
                .unwrap();
                b.len()
            }
        );
    }

    #[test]
    fn gltf_with_a_data_uri_node_transforms_and_no_indices() {
        // One triangle in metres, Y up, moved 1 m along glTF X by a parent node and scaled
        // by 2 in the child.
        let mut bin = Vec::new();
        for c in [0.0f32, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.001, 0.0] {
            bin.extend_from_slice(&c.to_le_bytes());
        }
        let b64 = {
            const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for ch in bin.chunks(3) {
                let n = ch.iter().enumerate().fold(0u32, |a, (i, b)| a | (u32::from(*b) << (16 - 8 * i)));
                for i in 0..4 {
                    s.push(if i <= ch.len() { T[((n >> (18 - 6 * i)) & 63) as usize] as char } else { '=' });
                }
            }
            s
        };
        let doc = json!({
            "asset": {"version": "2.0"},
            "scenes": [{"nodes": [0]}],
            "nodes": [{"translation": [1.0, 0.0, 0.0], "children": [1]}, {"scale": [2.0, 2.0, 2.0], "mesh": 0, "name": "tri"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
            "bufferViews": [{"buffer": 0, "byteLength": 36}],
            "buffers": [{"byteLength": 36, "uri": format!("data:application/octet-stream;base64,{b64}")}],
        });
        let m = import_bytes(doc.to_string().as_bytes(), Some(Format::Gltf), &ImportOptions::default()).unwrap();
        let p = &m.meshes[0].mesh.positions;
        assert_eq!(m.meshes[0].name, "tri");
        // glTF (1 m + 2 mm, 0, 0) is Gusset (1002 mm, 0, 0); glTF +Y (up) is Gusset +Z.
        assert!(p.iter().any(|q| (q[0] - 1002.0).abs() < 1e-3), "{p:?}");
        assert!(p.iter().any(|q| (q[2] - 2.0).abs() < 1e-4 && q[0] > 999.0), "{p:?}");
    }

    #[test]
    fn hostile_or_external_files_are_refused() {
        let opts = ImportOptions::default();
        let external = json!({"asset": {"version": "2.0"}, "buffers": [{"byteLength": 4, "uri": "/etc/passwd"}]});
        let e = import_bytes(external.to_string().as_bytes(), Some(Format::Gltf), &opts).unwrap_err();
        assert!(e.to_string().contains("external"), "{e}");
        let cyclic = json!({"asset": {"version": "2.0"}, "scenes": [{"nodes": [0]}], "nodes": [{"children": [0]}]});
        assert!(import_bytes(cyclic.to_string().as_bytes(), Some(Format::Gltf), &opts).is_err());
        let past = json!({"asset": {"version": "2.0"}, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            "accessors": [{"bufferView": 0, "componentType": 5126, "count": 1000, "type": "VEC3"}],
            "bufferViews": [{"buffer": 0, "byteLength": 12}],
            "buffers": [{"byteLength": 12, "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAA"}]});
        assert!(import_bytes(past.to_string().as_bytes(), Some(Format::Gltf), &opts).is_err());
        assert!(import_bytes(b"glTF\x01\x00\x00\x00", Some(Format::Gltf), &opts).is_err());
        let required = json!({"asset": {"version": "2.0"}, "extensionsRequired": ["KHR_draco_mesh_compression"]});
        assert!(import_bytes(required.to_string().as_bytes(), Some(Format::Gltf), &opts).is_err());
    }
}
