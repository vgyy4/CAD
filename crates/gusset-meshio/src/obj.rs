//! Wavefront OBJ reader (geometry only) and writer (no MTL).
//!
//! Reader: `v` and `f`/`fo` statements; `v/vt/vn` face corners; negative (relative)
//! indices; polygons triangulated by ear clipping; `o` (or, when a file has no `o`, `g`)
//! splits objects. Materials, texture coordinates, normals, smoothing groups, lines and
//! free-form geometry are ignored (reported once in the warnings).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;

use crate::error::{Result, limit, malformed, non_finite};
use crate::utils::{self, Tri};
use crate::{Cancel, ExportBody, ExportReport, Format, ImportOptions, ImportedMesh, Soup, clean_name, finish};

const F: Format = Format::Obj;
const UNITS_NOTE: &str = "OBJ has no units; assumed millimetres";

/// Content heuristic: the first meaningful lines are OBJ statements.
pub(crate) fn looks_like_obj(b: &[u8]) -> bool {
    let head = b.get(..b.len().min(4096)).unwrap_or(&[]);
    if head.contains(&0) {
        return false;
    }
    let mut seen = 0;
    for line in head.split(|&c| c == b'\n') {
        let t = trim(line);
        if t.is_empty() || t.starts_with(b"#") {
            continue;
        }
        let kw = t.split(|c| c.is_ascii_whitespace()).next().unwrap_or(&[]);
        let known: &[&[u8]] = &[b"v", b"vt", b"vn", b"f", b"o", b"g", b"s", b"usemtl", b"mtllib", b"l", b"vp"];
        if !known.contains(&kw) {
            return false;
        }
        seen += 1;
        if seen >= 3 {
            return true;
        }
    }
    seen > 0
}

fn trim(b: &[u8]) -> &[u8] {
    let s = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
    let e = b.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(s, |e| e + 1);
    b.get(s..e.max(s)).unwrap_or(&[])
}

#[derive(Default)]
struct Polys {
    /// Absolute 0-based vertex indices of all polygons, concatenated.
    idx: Vec<u32>,
    /// (start into `idx`, corner count, object id, source line)
    polys: Vec<(u32, u32, u32, u32)>,
}

pub(crate) fn import(
    b: &[u8],
    opts: &ImportOptions,
    cancel: Cancel<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ImportedMesh>> {
    let lim = &opts.limits;
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut polys = Polys::default();
    // Objects named by `o` and by `g` are tracked separately; which set is used is
    // decided at the end (files without `o` use groups).
    let mut o_names: Vec<String> = Vec::new();
    let mut g_names: Vec<String> = Vec::new();
    let mut g_lookup: HashMap<String, u32> = HashMap::new();
    let mut cur_o: u32 = u32::MAX;
    let mut cur_g: u32 = u32::MAX;
    let mut poly_obj: Vec<(u32, u32)> = Vec::new();
    let mut est_tris: u64 = 0;
    let mut ignored_elems = 0u64;
    let mut unknown = 0u64;

    for (ln, raw) in b.split(|&c| c == b'\n').enumerate() {
        cancel.tick(ln)?;
        let line_no = ln + 1;
        let at = || format!("line {line_no}");
        let line = match raw.iter().position(|&c| c == b'#') {
            Some(p) => raw.get(..p).unwrap_or(&[]),
            None => raw,
        };
        let mut toks = line.split(|c| c.is_ascii_whitespace()).filter(|t| !t.is_empty());
        let Some(kw) = toks.next() else { continue };
        match kw {
            b"v" => {
                let mut p = [0.0f32; 3];
                for c in p.iter_mut() {
                    let t = toks.next();
                    let v = t
                        .and_then(|t| std::str::from_utf8(t).ok())
                        .and_then(|s| s.parse::<f32>().ok())
                        .ok_or_else(|| malformed(F, at(), "vertex needs three numbers"))?;
                    // Parsed directly as f32 (no double rounding); overflow becomes ±inf.
                    if !v.is_finite() {
                        return Err(non_finite(F, at(), f64::from(v)));
                    }
                    *c = v;
                }
                if verts.len() as u64 >= lim.max_vertices {
                    return Err(limit("vertex count", verts.len() as u64 + 1, lim.max_vertices));
                }
                verts.push(p);
            }
            b"f" | b"fo" => {
                let start = polys.idx.len() as u32;
                let mut n = 0u32;
                for t in toks {
                    let first = t.split(|&c| c == b'/').next().unwrap_or(&[]);
                    let i: i64 = std::str::from_utf8(first).ok().and_then(|s| s.parse().ok()).ok_or_else(|| {
                        malformed(F, at(), format!("bad face index {:?}", String::from_utf8_lossy(t)))
                    })?;
                    let abs = if i > 0 {
                        i - 1
                    } else if i < 0 {
                        verts.len() as i64 + i
                    } else {
                        return Err(malformed(F, at(), "face index 0 is invalid (OBJ indices start at 1)"));
                    };
                    if abs < 0 {
                        return Err(malformed(
                            F,
                            at(),
                            format!("relative index {i} points before the first vertex ({} defined)", verts.len()),
                        ));
                    }
                    let abs = u32::try_from(abs)
                        .ok()
                        .filter(|&a| u64::from(a) < lim.max_vertices)
                        .ok_or_else(|| malformed(F, at(), format!("face index {i} is out of range")))?;
                    n += 1;
                    if u64::from(n) > lim.max_polygon_vertices {
                        return Err(limit("corners in one polygon", u64::from(n), lim.max_polygon_vertices));
                    }
                    polys.idx.push(abs);
                }
                if n < 3 {
                    polys.idx.truncate(start as usize);
                    warnings.push(format!("OBJ line {line_no}: face with {n} corners ignored"));
                    continue;
                }
                est_tris += u64::from(n - 2);
                if est_tris > lim.max_triangles {
                    return Err(limit("triangle count", est_tris, lim.max_triangles));
                }
                polys.polys.push((start, n, 0, line_no as u32));
                poly_obj.push((cur_o, cur_g));
            }
            b"o" | b"g" => {
                let name =
                    clean_name(&String::from_utf8_lossy(trim(line).get(kw.len()..).unwrap_or(&[])), lim.max_name_bytes);
                if (o_names.len() + g_names.len()) as u64 >= lim.max_objects {
                    return Err(limit("object/group count", lim.max_objects + 1, lim.max_objects));
                }
                if kw == b"o" {
                    o_names.push(name);
                    cur_o = (o_names.len() - 1) as u32;
                } else {
                    cur_g = match g_lookup.get(&name) {
                        Some(&g) => g,
                        None => {
                            g_names.push(name.clone());
                            let g = (g_names.len() - 1) as u32;
                            g_lookup.insert(name, g);
                            g
                        }
                    };
                }
            }
            b"vt" | b"vn" | b"vp" | b"s" | b"usemtl" | b"mtllib" | b"usemap" | b"maplib" => {}
            b"l" | b"p" | b"curv" | b"curv2" | b"surf" | b"cstype" | b"deg" | b"parm" | b"end" => ignored_elems += 1,
            _ => unknown += 1,
        }
    }
    if ignored_elems > 0 {
        warnings.push(format!("ignored {ignored_elems} OBJ line/point/free-form statements"));
    }
    if unknown > 0 {
        warnings.push(format!("ignored {unknown} unrecognised OBJ lines"));
    }

    // Assign objects: `o` if the file has any, otherwise groups.
    let use_o = !o_names.is_empty();
    let names = if use_o { &o_names } else { &g_names };
    let mut order: Vec<u32> = Vec::new(); // object keys in first-appearance order
    let mut slot: HashMap<u32, usize> = HashMap::new();
    for (p, &(o, g)) in polys.polys.iter_mut().zip(poly_obj.iter()) {
        let key = if use_o { o } else { g };
        let s = *slot.entry(key).or_insert_with(|| {
            order.push(key);
            order.len() - 1
        });
        p.2 = s as u32;
    }

    let base = opts
        .name_hint
        .as_deref()
        .map(|n| clean_name(n, lim.max_name_bytes))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "OBJ mesh".to_string());
    let nverts = verts.len();
    let mut fallbacks = 0u64;
    let mut out = Vec::with_capacity(order.len());
    let mut corner_pts: Vec<[f64; 3]> = Vec::new();
    // Polygons grouped by object, keeping file order within each object (stable sort).
    let mut by_obj: Vec<usize> = (0..polys.polys.len()).collect();
    by_obj.sort_by_key(|&k| polys.polys.get(k).map_or(0, |p| p.2));
    let mut cursor = 0usize;
    for (s, &key) in order.iter().enumerate() {
        let mut soup = Soup::default();
        let mut local: HashMap<u32, u32> = HashMap::new();
        let begin = cursor;
        while by_obj.get(cursor).and_then(|&k| polys.polys.get(k)).is_some_and(|p| p.2 as usize == s) {
            cursor += 1;
        }
        for &k in by_obj.get(begin..cursor).unwrap_or(&[]) {
            let Some(&(start, n, _, line_no)) = polys.polys.get(k) else { continue };
            cancel.tick(k)?;
            let corners = polys.idx.get(start as usize..(start + n) as usize).unwrap_or(&[]);
            let mut ids = Vec::with_capacity(corners.len());
            for &c in corners {
                if c as usize >= nverts {
                    return Err(malformed(
                        F,
                        format!("line {line_no}"),
                        format!("face references vertex {} but only {nverts} are defined", c + 1),
                    ));
                }
                let id = *local.entry(c).or_insert_with(|| {
                    soup.positions.push(utils::pos(&verts, c));
                    (soup.positions.len() - 1) as u32
                });
                ids.push(id);
            }
            if let [a, b, c] = ids[..] {
                soup.tris.push([a, b, c]);
            } else {
                corner_pts.clear();
                corner_pts.extend(ids.iter().map(|&i| utils::v64(utils::pos(&soup.positions, i))));
                let (tris, ok) = utils::triangulate_polygon(&corner_pts);
                if !ok {
                    fallbacks += 1;
                }
                for t in tris {
                    let m = |j: usize| ids.get(j).copied().unwrap_or(0);
                    let tri: Tri = [m(t[0]), m(t[1]), m(t[2])];
                    soup.tris.push(tri);
                }
            }
        }
        soup.ranges = crate::one_range(soup.tris.len() as u32);
        let name = if key == u32::MAX {
            base.clone()
        } else {
            names.get(key as usize).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("{base} {}", s + 1))
        };
        out.push(finish(name, soup, UNITS_NOTE, opts, warnings));
    }
    if fallbacks > 0 {
        warnings.push(format!(
            "{fallbacks} OBJ polygons could not be ear-clipped (self-intersecting, degenerate or very large) and were fan-triangulated"
        ));
    }
    Ok(out)
}

fn obj_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_whitespace() || c.is_control() { '_' } else { c }).collect();
    if n.is_empty() { "body".to_string() } else { n }
}

pub(crate) fn write(bodies: &[ExportBody<'_>], normals: bool, out: &mut dyn Write) -> Result<ExportReport> {
    let mut s = String::with_capacity(1 << 20);
    s.push_str("# Gusset OBJ export\n# units: millimetres\n");
    let mut base: u64 = 0;
    let mut total = 0u64;
    for b in bodies {
        let (positions, remap) = utils::weld_exact(&b.mesh.positions);
        let tris: Vec<Tri> =
            utils::triangles(b.mesh).map(|t| t.map(|i| remap.get(i as usize).copied().unwrap_or(0))).collect();
        let _ = writeln!(s, "o {}", obj_name(b.name));
        for p in &positions {
            let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
            flush(&mut s, out)?;
        }
        if normals {
            for n in utils::smooth_normals(&positions, &tris) {
                let _ = writeln!(s, "vn {} {} {}", n[0], n[1], n[2]);
                flush(&mut s, out)?;
            }
        }
        for t in &tris {
            let [a, b2, c] = t.map(|i| base + u64::from(i) + 1);
            if normals {
                let _ = writeln!(s, "f {a}//{a} {b2}//{b2} {c}//{c}");
            } else {
                let _ = writeln!(s, "f {a} {b2} {c}");
            }
            flush(&mut s, out)?;
        }
        total += tris.len() as u64;
        base += positions.len() as u64;
    }
    out.write_all(s.as_bytes())?;
    Ok(ExportReport { warnings: Vec::new(), triangles: total })
}

fn flush(s: &mut String, out: &mut dyn Write) -> Result<()> {
    if s.len() >= 1 << 20 {
        out.write_all(s.as_bytes())?;
        s.clear();
    }
    Ok(())
}
