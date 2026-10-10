//! STL reader (binary and ASCII, auto-detected) and writers.
//!
//! Detection: a file is ASCII only if it starts with `solid` *and* consists of text;
//! many binary files also start with `solid` in their 80-byte header, so a binary file
//! whose size matches `84 + 50 × count` is always read as binary.

use std::fmt::Write as _;
use std::io::Write;

use crate::error::{Result, limit, malformed, non_finite};
use crate::utils::{self, Tri};
use crate::{
    Cancel, ExportBody, ExportReport, Format, ImportOptions, ImportedMesh, MeshIoError, Soup, clean_name, finish,
};

const F: Format = Format::Stl;
const UNITS_NOTE: &str = "STL has no units; assumed millimetres";

fn binary_count(b: &[u8]) -> Option<u64> {
    let s = b.get(80..84)?;
    Some(u64::from(u32::from_le_bytes(s.try_into().ok()?)))
}

fn binary_size_matches(b: &[u8]) -> bool {
    binary_count(b).is_some_and(|n| 84 + 50 * n == b.len() as u64)
}

fn skip_bom_ws(b: &[u8]) -> &[u8] {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let start = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
    b.get(start..).unwrap_or(&[])
}

fn starts_with_solid(b: &[u8]) -> bool {
    skip_bom_ws(b).get(..5).is_some_and(|s| s.eq_ignore_ascii_case(b"solid"))
}

/// Text = printable ASCII, whitespace, or UTF-8 continuation/lead bytes (names).
fn is_text(b: &[u8]) -> bool {
    b.iter().all(|&c| c.is_ascii_graphic() || c.is_ascii_whitespace() || c >= 0x80)
}

pub(crate) fn looks_like_stl(b: &[u8]) -> bool {
    binary_size_matches(b) || starts_with_solid(b)
}

pub(crate) fn import(
    b: &[u8],
    opts: &ImportOptions,
    cancel: Cancel<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ImportedMesh>> {
    let solid = starts_with_solid(b);
    if solid && is_text(b) {
        return import_ascii(b, opts, cancel, warnings);
    }
    if b.len() >= 84 {
        if solid && !binary_size_matches(b) {
            // Text-ish file with a stray control byte: try ASCII before calling it a
            // truncated binary file.
            if let Ok(m) = import_ascii(b, opts, cancel, warnings) {
                return Ok(m);
            }
        }
        return import_binary(b, opts, cancel, warnings);
    }
    if solid {
        return import_ascii(b, opts, cancel, warnings);
    }
    Err(malformed(F, "header", format!("file is {} bytes, too small for a binary STL (84 bytes minimum)", b.len())))
}

fn base_name(opts: &ImportOptions) -> String {
    opts.name_hint
        .as_deref()
        .map(|n| clean_name(n, opts.limits.max_name_bytes))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "STL mesh".to_string())
}

fn import_binary(
    b: &[u8],
    opts: &ImportOptions,
    cancel: Cancel<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ImportedMesh>> {
    let n = binary_count(b).unwrap_or(0);
    if n > opts.limits.max_triangles {
        return Err(limit("triangle count", n, opts.limits.max_triangles));
    }
    let need = 84 + 50 * n;
    let len = b.len() as u64;
    if need > len {
        return Err(malformed(
            F,
            "header",
            format!("binary STL declares {n} triangles ({need} bytes) but the file has only {len} bytes (truncated?)"),
        ));
    }
    if need < len {
        warnings.push(format!("ignored {} trailing bytes after the {n} triangles of the binary STL", len - need));
    }
    let n = n as usize;
    let data = b.get(84..need as usize).unwrap_or(&[]);
    let mut positions = Vec::with_capacity(n * 3);
    let mut tris: Vec<Tri> = Vec::with_capacity(n);
    for (i, rec) in data.chunks_exact(50).enumerate() {
        cancel.tick(i)?;
        let coords = rec.get(12..48).unwrap_or(&[]);
        let mut v = [0.0f32; 9];
        for (dst, c) in v.iter_mut().zip(coords.chunks_exact(4)) {
            *dst = f32::from_le_bytes(c.try_into().unwrap_or([0; 4]));
        }
        if let Some(bad) = v.iter().find(|c| !c.is_finite()) {
            return Err(non_finite(F, format!("triangle {}", i + 1), f64::from(*bad)));
        }
        let base = (i * 3) as u32;
        positions.push([v[0], v[1], v[2]]);
        positions.push([v[3], v[4], v[5]]);
        positions.push([v[6], v[7], v[8]]);
        tris.push([base, base + 1, base + 2]);
    }
    let soup = Soup { positions, tris, ranges: crate::one_range(n as u32) };
    Ok(vec![finish(base_name(opts), soup, UNITS_NOTE, opts, warnings)])
}

struct Lexer<'a> {
    b: &'a [u8],
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    fn skip_ws(&mut self) {
        while let Some(&c) = self.b.get(self.pos) {
            if !c.is_ascii_whitespace() {
                break;
            }
            if c == b'\n' {
                self.line += 1;
            }
            self.pos += 1;
        }
    }
    fn token(&mut self) -> Option<&'a [u8]> {
        self.skip_ws();
        let start = self.pos;
        while self.b.get(self.pos).is_some_and(|c| !c.is_ascii_whitespace()) {
            self.pos += 1;
        }
        if start == self.pos { None } else { self.b.get(start..self.pos) }
    }
    fn peek(&mut self) -> Option<&'a [u8]> {
        let (p, l) = (self.pos, self.line);
        let t = self.token();
        self.pos = p;
        self.line = l;
        t
    }
    /// The remainder of the current line (not including the newline).
    fn rest_of_line(&mut self) -> &'a [u8] {
        let start = self.pos;
        while self.b.get(self.pos).is_some_and(|&c| c != b'\n') {
            self.pos += 1;
        }
        self.b.get(start..self.pos).unwrap_or(&[])
    }
    fn at(&self) -> String {
        format!("line {}", self.line)
    }
}

fn kw(t: &[u8], k: &str) -> bool {
    t.eq_ignore_ascii_case(k.as_bytes())
}

fn show(t: Option<&[u8]>) -> String {
    match t {
        None => "end of file".to_string(),
        Some(t) => format!("'{}'", String::from_utf8_lossy(t.get(..t.len().min(40)).unwrap_or(&[]))),
    }
}

fn expect(lx: &mut Lexer<'_>, k: &str) -> Result<()> {
    let t = lx.token();
    if t.is_some_and(|t| kw(t, k)) {
        Ok(())
    } else {
        Err(malformed(F, lx.at(), format!("expected '{k}', found {}", show(t))))
    }
}

fn import_ascii(
    b: &[u8],
    opts: &ImportOptions,
    cancel: Cancel<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ImportedMesh>> {
    let lim = &opts.limits;
    let body = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let mut lx = Lexer { b: body, pos: 0, line: 1 };
    let mut solids: Vec<(String, Soup)> = Vec::new();
    let mut total_tris: u64 = 0;
    let mut polygons = 0u64;
    let mut corners: Vec<[f32; 3]> = Vec::with_capacity(3);
    let mut facet_no = 0usize;

    loop {
        match lx.token() {
            None => break,
            Some(t) if kw(t, "solid") => {}
            Some(_) if !solids.is_empty() => {
                // e.g. a DOS end-of-file marker or exporter junk after the last solid
                warnings.push(format!("ignored trailing data after the last 'endsolid' ({})", lx.at()));
                break;
            }
            t => return Err(malformed(F, lx.at(), format!("expected 'solid', found {}", show(t)))),
        }
        if solids.len() as u64 >= lim.max_objects {
            return Err(limit("STL solid count", solids.len() as u64 + 1, lim.max_objects));
        }
        let name = clean_name(&String::from_utf8_lossy(lx.rest_of_line()), lim.max_name_bytes);
        let mut soup = Soup::default();
        loop {
            match lx.token() {
                None => {
                    warnings.push(format!("STL solid {name:?} has no 'endsolid' (file truncated?)"));
                    break;
                }
                Some(t) if kw(t, "endsolid") => {
                    lx.rest_of_line();
                    break;
                }
                Some(t) if kw(t, "facet") => {
                    facet_no += 1;
                    cancel.tick(facet_no)?;
                    // "normal nx ny nz" is ignored (normals are recomputed).
                    let mut skipped = 0;
                    loop {
                        match lx.token() {
                            Some(t) if kw(t, "outer") => break,
                            Some(_) if skipped < 4 => skipped += 1,
                            t => {
                                return Err(malformed(F, lx.at(), format!("expected 'outer loop', found {}", show(t))));
                            }
                        }
                    }
                    expect(&mut lx, "loop")?;
                    corners.clear();
                    while lx.peek().is_some_and(|t| kw(t, "vertex")) {
                        lx.token();
                        let mut p = [0.0f32; 3];
                        for c in p.iter_mut() {
                            let tok = lx.token();
                            let v = tok
                                .and_then(|t| std::str::from_utf8(t).ok())
                                .and_then(|s| s.parse::<f32>().ok())
                                .ok_or_else(|| {
                                malformed(F, lx.at(), format!("expected a number, found {}", show(tok)))
                            })?;
                            if !v.is_finite() {
                                return Err(non_finite(F, format!("facet {facet_no} ({})", lx.at()), f64::from(v)));
                            }
                            *c = v;
                        }
                        corners.push(p);
                        if corners.len() as u64 > lim.max_polygon_vertices {
                            return Err(limit(
                                "vertices in one STL facet",
                                corners.len() as u64,
                                lim.max_polygon_vertices,
                            ));
                        }
                    }
                    expect(&mut lx, "endloop")?;
                    expect(&mut lx, "endfacet")?;
                    if corners.len() < 3 {
                        return Err(malformed(
                            F,
                            lx.at(),
                            format!("facet {facet_no} has {} vertices (3 required)", corners.len()),
                        ));
                    }
                    let k = corners.len() - 2;
                    total_tris += k as u64;
                    if total_tris > lim.max_triangles {
                        return Err(limit("triangle count", total_tris, lim.max_triangles));
                    }
                    if k > 1 {
                        polygons += 1;
                    }
                    let base = soup.positions.len() as u32;
                    soup.positions.extend_from_slice(&corners);
                    for j in 0..k as u32 {
                        soup.tris.push([base, base + j + 1, base + j + 2]);
                    }
                }
                t => {
                    return Err(malformed(F, lx.at(), format!("expected 'facet' or 'endsolid', found {}", show(t))));
                }
            }
        }
        soup.ranges = crate::one_range(soup.tris.len() as u32);
        solids.push((name, soup));
    }
    if polygons > 0 {
        warnings.push(format!("{polygons} STL facets had more than 3 vertices and were fan-triangulated"));
    }
    if solids.is_empty() {
        return Err(MeshIoError::Empty { format: F });
    }
    let base = base_name(opts);
    let many = solids.len() > 1;
    Ok(solids
        .into_iter()
        .enumerate()
        .map(|(i, (name, soup))| {
            let name = if !name.is_empty() {
                name
            } else if many {
                format!("{base} {}", i + 1)
            } else {
                base.clone()
            };
            finish(name, soup, UNITS_NOTE, opts, warnings)
        })
        .collect())
}

// ---------------------------------------------------------------------------------------
// writers

fn body_tris<'a>(b: &'a ExportBody<'_>) -> impl Iterator<Item = ([f32; 3], [[f32; 3]; 3])> + 'a {
    utils::triangles(b.mesh).map(|t| {
        let p = &b.mesh.positions;
        let n = utils::face_normal(p, t).unwrap_or([0.0; 3]);
        (n, [utils::pos(p, t[0]), utils::pos(p, t[1]), utils::pos(p, t[2])])
    })
}

pub(crate) fn write_binary(bodies: &[ExportBody<'_>], out: &mut dyn Write) -> Result<ExportReport> {
    let total: u64 = bodies.iter().map(|b| b.mesh.triangle_count() as u64).sum();
    let count = u32::try_from(total).map_err(|_| MeshIoError::Unsupported {
        format: F,
        detail: format!("{total} triangles (binary STL max 2^32-1)"),
    })?;
    // The header must not start with "solid" (it would look like ASCII STL).
    let mut header = [b' '; 80];
    let text = b"Gusset binary STL; units: mm";
    for (h, t) in header.iter_mut().zip(text.iter()) {
        *h = *t;
    }
    out.write_all(&header)?;
    out.write_all(&count.to_le_bytes())?;
    let mut buf = Vec::with_capacity(50 * 8192);
    for b in bodies {
        for (n, tri) in body_tris(b) {
            for c in n.iter().chain(tri.iter().flatten()) {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            buf.extend_from_slice(&[0, 0]);
            if buf.len() >= 50 * 8192 {
                out.write_all(&buf)?;
                buf.clear();
            }
        }
    }
    out.write_all(&buf)?;
    Ok(ExportReport { warnings: Vec::new(), triangles: total })
}

fn ascii_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    n.trim().to_string()
}

pub(crate) fn write_ascii(bodies: &[ExportBody<'_>], out: &mut dyn Write) -> Result<ExportReport> {
    let mut s = String::with_capacity(1 << 20);
    let mut total = 0u64;
    for b in bodies {
        let name = ascii_name(b.name);
        let _ = writeln!(s, "solid {name}");
        for (n, tri) in body_tris(b) {
            total += 1;
            let _ = writeln!(s, "  facet normal {:e} {:e} {:e}", n[0], n[1], n[2]);
            s.push_str("    outer loop\n");
            for v in tri {
                let _ = writeln!(s, "      vertex {:e} {:e} {:e}", v[0], v[1], v[2]);
            }
            s.push_str("    endloop\n  endfacet\n");
            if s.len() >= 1 << 20 {
                out.write_all(s.as_bytes())?;
                s.clear();
            }
        }
        let _ = writeln!(s, "endsolid {name}");
    }
    out.write_all(s.as_bytes())?;
    Ok(ExportReport { warnings: Vec::new(), triangles: total })
}
