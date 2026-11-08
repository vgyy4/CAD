//! PLY (Stanford polygon file): ASCII and binary (little and big endian) import, binary
//! little-endian export. Only the `vertex` element's `x`, `y`, `z` and the `face` element's
//! index list are read; every other element and property is skipped (it is still parsed
//! in binary files, to find the data after it). PLY has no unit: values are taken as mm.
//! Polygons are triangulated by ear clipping, as for OBJ.

use crate::error::{limit, malformed, non_finite};
use crate::utils::{self, Tri};
use crate::{
    Cancel, ExportBody, ExportReport, Format, ImportOptions, ImportedMesh, MeshIoError, Result, Soup, clean_name,
    finish,
};
use std::io::Write;

const F: Format = Format::Ply;
const UNITS_NOTE: &str = "PLY has no unit; values taken as mm";
/// Header size limit: real headers are a few hundred bytes.
const MAX_HEADER: usize = 64 << 10;

pub(crate) fn looks_like_ply(b: &[u8]) -> bool {
    b.starts_with(b"ply\n") || b.starts_with(b"ply\r\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(s: &str) -> Option<Scalar> {
        Some(match s {
            "char" | "int8" => Scalar::I8,
            "uchar" | "uint8" => Scalar::U8,
            "short" | "int16" => Scalar::I16,
            "ushort" | "uint16" => Scalar::U16,
            "int" | "int32" => Scalar::I32,
            "uint" | "uint32" => Scalar::U32,
            "float" | "float32" => Scalar::F32,
            "double" | "float64" => Scalar::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Scalar::I8 | Scalar::U8 => 1,
            Scalar::I16 | Scalar::U16 => 2,
            Scalar::I32 | Scalar::U32 | Scalar::F32 => 4,
            Scalar::F64 => 8,
        }
    }
}

#[derive(Debug, Clone)]
struct Property {
    name: String,
    /// `Some(count type)` for a list.
    list: Option<Scalar>,
    ty: Scalar,
}

#[derive(Debug, Clone)]
struct Element {
    name: String,
    count: u64,
    props: Vec<Property>,
}

struct Header {
    encoding: Encoding,
    elements: Vec<Element>,
    name: Option<String>,
    /// Byte offset of the data after `end_header`.
    data: usize,
}

fn header(b: &[u8], opts: &ImportOptions) -> Result<Header> {
    let end = b
        .windows(10)
        .take(MAX_HEADER)
        .position(|w| w == b"end_header")
        .ok_or_else(|| malformed(F, "header", "no end_header in the first 64 KiB"))?;
    let after = b.get(end + 10..).unwrap_or(&[]);
    let skip = match after {
        [b'\r', b'\n', ..] => 2,
        [b'\n', ..] => 1,
        [] => 0,
        _ => return Err(malformed(F, "header", "end_header is not followed by a line break")),
    };
    let text = std::str::from_utf8(b.get(..end).unwrap_or(&[])).map_err(|_| malformed(F, "header", "not text"))?;
    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    let mut name = None;
    for (i, line) in text.lines().enumerate() {
        let loc = || format!("header line {}", i + 1);
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["ply"] | [] => {}
            ["format", f, _] => {
                encoding = Some(match *f {
                    "ascii" => Encoding::Ascii,
                    "binary_little_endian" => Encoding::Little,
                    "binary_big_endian" => Encoding::Big,
                    other => return Err(malformed(F, loc(), format!("unknown format '{other}'"))),
                });
            }
            ["comment", rest @ ..] | ["obj_info", rest @ ..] => {
                if name.is_none()
                    && let ["name", n @ ..] = rest
                {
                    name = Some(clean_name(&n.join(" "), opts.limits.max_name_bytes));
                }
            }
            ["element", n, count] => {
                let count: u64 = count.parse().map_err(|_| malformed(F, loc(), format!("bad count '{count}'")))?;
                if elements.len() >= 64 {
                    return Err(malformed(F, loc(), "more than 64 elements"));
                }
                elements.push(Element { name: (*n).to_string(), count, props: Vec::new() });
            }
            ["property", "list", c, t, n] => {
                let (Some(c), Some(t)) = (Scalar::parse(c), Scalar::parse(t)) else {
                    return Err(malformed(F, loc(), format!("unknown list types '{c} {t}'")));
                };
                let e = elements.last_mut().ok_or_else(|| malformed(F, loc(), "property before any element"))?;
                e.props.push(Property { name: (*n).to_string(), list: Some(c), ty: t });
            }
            ["property", t, n] => {
                let t = Scalar::parse(t).ok_or_else(|| malformed(F, loc(), format!("unknown type '{t}'")))?;
                let e = elements.last_mut().ok_or_else(|| malformed(F, loc(), "property before any element"))?;
                if e.props.len() >= 256 {
                    return Err(malformed(F, loc(), "more than 256 properties"));
                }
                e.props.push(Property { name: (*n).to_string(), list: None, ty: t });
            }
            _ => return Err(malformed(F, loc(), format!("unexpected '{line}'"))),
        }
    }
    let encoding = encoding.ok_or_else(|| malformed(F, "header", "no format line"))?;
    Ok(Header { encoding, elements, name, data: end + 10 + skip })
}

/// Reads scalars from the data section, in any of the three encodings.
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
    encoding: Encoding,
}

impl Reader<'_> {
    fn next(&mut self, ty: Scalar) -> Result<f64> {
        if self.encoding == Encoding::Ascii {
            let rest = self.b.get(self.pos..).unwrap_or(&[]);
            let start = rest.iter().position(|c| !c.is_ascii_whitespace()).ok_or_else(|| self.eof())?;
            let len =
                rest.get(start..).unwrap_or(&[]).iter().position(u8::is_ascii_whitespace).unwrap_or(rest.len() - start);
            let word = rest.get(start..start + len).unwrap_or(&[]);
            self.pos += start + len;
            let s = std::str::from_utf8(word).map_err(|_| self.bad("not a number"))?;
            return s.parse::<f64>().map_err(|_| self.bad(&format!("'{s}' is not a number")));
        }
        let n = ty.size();
        let raw = self.b.get(self.pos..self.pos + n).ok_or_else(|| self.eof())?;
        self.pos += n;
        let mut a = [0u8; 8];
        if let Some(dst) = a.get_mut(..n) {
            dst.copy_from_slice(raw);
        }
        if self.encoding == Encoding::Big
            && let Some(dst) = a.get_mut(..n)
        {
            dst.reverse();
        }
        let [b0, b1, b2, b3, b4, b5, b6, b7] = a;
        Ok(match ty {
            Scalar::I8 => f64::from(b0 as i8),
            Scalar::U8 => f64::from(b0),
            Scalar::I16 => f64::from(i16::from_le_bytes([b0, b1])),
            Scalar::U16 => f64::from(u16::from_le_bytes([b0, b1])),
            Scalar::I32 => f64::from(i32::from_le_bytes([b0, b1, b2, b3])),
            Scalar::U32 => f64::from(u32::from_le_bytes([b0, b1, b2, b3])),
            Scalar::F32 => f64::from(f32::from_le_bytes([b0, b1, b2, b3])),
            Scalar::F64 => f64::from_le_bytes([b0, b1, b2, b3, b4, b5, b6, b7]),
        })
    }

    fn eof(&self) -> MeshIoError {
        malformed(F, format!("byte {}", self.pos), "the file ends inside the data (truncated?)")
    }

    fn bad(&self, what: &str) -> MeshIoError {
        malformed(F, format!("byte {}", self.pos), what.to_string())
    }
}

pub(crate) fn import(
    b: &[u8],
    opts: &ImportOptions,
    cancel: Cancel<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ImportedMesh>> {
    let h = header(b, opts)?;
    let mut r = Reader { b, pos: h.data, encoding: h.encoding };
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut polys: Vec<Vec<u32>> = Vec::new();
    let mut triangles: u64 = 0;
    // Each value takes at least one byte (binary) or two (ASCII digit and space): declared
    // counts larger than the file are refused before anything is allocated from them.
    let room = b.len().saturating_sub(h.data) as u64;
    for e in &h.elements {
        let min_bytes: u64 = e.props.iter().map(|p| p.list.unwrap_or(p.ty).size() as u64).sum::<u64>().max(1);
        let min_bytes = if h.encoding == Encoding::Ascii { 2 * e.props.len().max(1) as u64 } else { min_bytes };
        if e.count.saturating_mul(min_bytes) > room {
            return Err(malformed(
                F,
                format!("element {}", e.name),
                format!("declares {} items, more than the file holds", e.count),
            ));
        }
        let is_vertex = e.name == "vertex";
        let is_face = e.name == "face";
        if is_vertex && e.count > opts.limits.max_vertices {
            return Err(limit("vertex count", e.count, opts.limits.max_vertices));
        }
        let xyz: Vec<Option<usize>> =
            ["x", "y", "z"].iter().map(|n| e.props.iter().position(|p| p.name == *n && p.list.is_none())).collect();
        if is_vertex && xyz.iter().any(Option::is_none) {
            return Err(malformed(F, "element vertex", "no x, y and z properties"));
        }
        let face_list =
            e.props.iter().position(|p| p.list.is_some() && (p.name == "vertex_indices" || p.name == "vertex_index"));
        if is_face && face_list.is_none() {
            return Err(malformed(F, "element face", "no vertex_indices list"));
        }
        for k in 0..e.count {
            cancel.tick(k as usize)?;
            let mut p = [0.0f64; 3];
            for (pi, prop) in e.props.iter().enumerate() {
                match prop.list {
                    Some(ct) => {
                        let n = r.next(ct)?;
                        if !(n >= 0.0 && n.fract() == 0.0) || n as u64 > opts.limits.max_polygon_vertices {
                            return Err(r.bad(&format!("bad list length {n}")));
                        }
                        let n = n as usize;
                        let keep = is_face && Some(pi) == face_list;
                        let mut ids = Vec::with_capacity(if keep { n } else { 0 });
                        for _ in 0..n {
                            let v = r.next(prop.ty)?;
                            if keep {
                                if !(v >= 0.0 && v.fract() == 0.0 && v < f64::from(u32::MAX)) {
                                    return Err(r.bad(&format!("bad vertex index {v}")));
                                }
                                ids.push(v as u32);
                            }
                        }
                        if keep && n >= 3 {
                            triangles += n as u64 - 2;
                            if triangles > opts.limits.max_triangles {
                                return Err(limit("triangle count", triangles, opts.limits.max_triangles));
                            }
                            polys.push(ids);
                        }
                    }
                    None => {
                        let v = r.next(prop.ty)?;
                        if is_vertex
                            && let Some(axis) = xyz.iter().position(|x| *x == Some(pi))
                            && let Some(slot) = p.get_mut(axis)
                        {
                            if !v.is_finite() {
                                return Err(non_finite(F, format!("vertex {}", k + 1), v));
                            }
                            *slot = v;
                        }
                    }
                }
            }
            if is_vertex {
                verts.push(p.map(|c| c as f32));
            }
        }
    }
    let mut soup = Soup { positions: verts, ..Soup::default() };
    let n = soup.positions.len();
    let mut ear_budget: u64 = 1_000_000_000;
    let mut fallbacks = 0u64;
    let mut corner_pts: Vec<[f64; 3]> = Vec::new();
    for (k, ids) in polys.iter().enumerate() {
        cancel.tick(k)?;
        if let Some(bad) = ids.iter().find(|&&i| i as usize >= n) {
            return Err(malformed(F, format!("face {}", k + 1), format!("references vertex {bad} but there are {n}")));
        }
        if let [a, b, c] = ids[..] {
            soup.tris.push([a, b, c]);
            continue;
        }
        corner_pts.clear();
        corner_pts.extend(ids.iter().map(|&i| utils::v64(utils::pos(&soup.positions, i))));
        let (tris, ok) = utils::triangulate_polygon_budgeted(&corner_pts, &mut ear_budget);
        if !ok {
            fallbacks += 1;
        }
        for t in tris {
            let m = |j: usize| ids.get(j).copied().unwrap_or(0);
            let tri: Tri = [m(t[0]), m(t[1]), m(t[2])];
            soup.tris.push(tri);
        }
    }
    if fallbacks > 0 {
        warnings.push(format!("{fallbacks} PLY polygons could not be ear-clipped and were fan-triangulated"));
    }
    soup.ranges = crate::one_range(soup.tris.len() as u32);
    let name = h
        .name
        .filter(|n| !n.is_empty())
        .or_else(|| opts.name_hint.as_deref().map(|n| clean_name(n, opts.limits.max_name_bytes)))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "PLY mesh".to_string());
    Ok(vec![finish(name, soup, UNITS_NOTE, opts, warnings)])
}

/// Binary little-endian PLY: one vertex and one face element for all bodies together (PLY
/// has no objects; the first body's name is the file's `comment name`).
pub(crate) fn write(bodies: &[ExportBody<'_>], out: &mut dyn Write) -> Result<ExportReport> {
    let nv: usize = bodies.iter().map(|b| b.mesh.positions.len()).sum();
    let nt: usize = bodies.iter().map(|b| b.mesh.indices.len() / 3).sum();
    let (Ok(nv32), Ok(_)) = (u32::try_from(nv), u32::try_from(nt)) else {
        return Err(MeshIoError::Unsupported { format: F, detail: format!("{nv} vertices (PLY indices are 32-bit)") });
    };
    let _ = nv32;
    let mut warnings = Vec::new();
    if bodies.len() > 1 {
        warnings.push(format!("PLY has no objects: {} bodies were written as one mesh", bodies.len()));
    }
    let name = bodies.first().map_or(String::new(), |b| clean_name(b.name, 200).replace(['\r', '\n'], " "));
    let head = format!(
        "ply\nformat binary_little_endian 1.0\ncomment Gusset; units: mm\ncomment name {name}\nelement vertex {nv}\nproperty float x\nproperty float y\nproperty float z\nelement face {nt}\nproperty list uchar uint vertex_indices\nend_header\n"
    );
    out.write_all(head.as_bytes())?;
    let mut buf = Vec::with_capacity(1 << 16);
    for b in bodies {
        for p in &b.mesh.positions {
            for c in p {
                buf.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    let mut base = 0u32;
    for b in bodies {
        for t in b.mesh.indices.chunks_exact(3) {
            buf.push(3);
            for i in t {
                buf.extend_from_slice(&(i + base).to_le_bytes());
            }
            if buf.len() >= 1 << 16 {
                out.write_all(&buf)?;
                buf.clear();
            }
        }
        base += b.mesh.positions.len() as u32;
    }
    out.write_all(&buf)?;
    Ok(ExportReport { warnings, triangles: nt as u64 })
}

#[cfg(test)]
mod tests {
    use crate::{ExportBody, ExportFormat, Format, ImportOptions, export_to_vec, import_bytes};
    use gusset_kernel::Mesh;

    fn tetra() -> Mesh {
        Mesh {
            positions: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]],
            indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
            ..Default::default()
        }
    }

    #[test]
    fn binary_round_trip_keeps_the_solid_and_its_name() {
        let t = tetra();
        let (bytes, r) =
            export_to_vec(&[ExportBody { name: "tetra", mesh: &t, color: None }], ExportFormat::Ply).unwrap();
        assert_eq!(r.triangles, 4);
        let m = import_bytes(&bytes, None, &ImportOptions::default()).unwrap();
        assert_eq!(m.format, Format::Ply);
        let mesh = &m.meshes[0];
        assert_eq!(mesh.name, "tetra");
        assert!(mesh.validation.can_become_solid(), "{:?}", mesh.validation);
        assert!((mesh.validation.signed_volume - 1000.0 / 6.0).abs() < 1e-3);
        // Deterministic bytes.
        assert_eq!(
            export_to_vec(&[ExportBody { name: "tetra", mesh: &t, color: None }], ExportFormat::Ply).unwrap().0,
            bytes
        );
    }

    #[test]
    fn ascii_quads_extra_properties_and_big_endian() {
        // A unit cube of quads, with colours and an extra element to skip.
        let ascii = "ply\nformat ascii 1.0\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nelement face 6\nproperty list uchar int vertex_indices\nelement extra 1\nproperty int k\nend_header\n\
0 0 0 9\n1 0 0 9\n1 1 0 9\n0 1 0 9\n0 0 1 9\n1 0 1 9\n1 1 1 9\n0 1 1 9\n\
4 0 3 2 1\n4 4 5 6 7\n4 0 1 5 4\n4 1 2 6 5\n4 2 3 7 6\n4 3 0 4 7\n42\n";
        let m = import_bytes(ascii.as_bytes(), Some(Format::Ply), &ImportOptions::default()).unwrap();
        let v = &m.meshes[0].validation;
        assert!(v.can_become_solid(), "{v:?}");
        assert!((v.signed_volume - 1.0).abs() < 1e-6);
        // The same triangle as big-endian binary with double coordinates.
        let mut b = b"ply\nformat binary_big_endian 1.0\nelement vertex 3\nproperty double x\nproperty double y\nproperty double z\nelement face 1\nproperty list uchar ushort vertex_indices\nend_header\n".to_vec();
        for p in [[0.0f64, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]] {
            for c in p {
                b.extend_from_slice(&c.to_be_bytes());
            }
        }
        b.push(3);
        for i in [0u16, 1, 2] {
            b.extend_from_slice(&i.to_be_bytes());
        }
        let m = import_bytes(&b, Some(Format::Ply), &ImportOptions::default()).unwrap();
        assert_eq!(m.meshes[0].mesh.indices.len(), 3);
        assert_eq!(m.meshes[0].mesh.positions.iter().map(|p| p[0]).fold(0.0f32, f32::max), 2.0);
    }

    #[test]
    fn hostile_files_are_refused_cleanly() {
        let opts = ImportOptions::default();
        for bad in [
            "ply\nformat ascii 1.0\nelement vertex 99999999999\nproperty float x\nproperty float y\nproperty float z\nend_header\n0 0 0\n",
            "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n3 0 1 2\n",
            "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nend_header\n0\n",
            "ply\nformat binary_little_endian 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nend_header\n\x00\x00",
            "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\n",
            "ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nend_header\nnan 0 0\n",
        ] {
            assert!(import_bytes(bad.as_bytes(), Some(Format::Ply), &opts).is_err(), "{bad:?}");
        }
    }
}
