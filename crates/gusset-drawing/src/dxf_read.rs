//! Reading 2D geometry from an ASCII DXF file (ADR 0032 phase 4: DXF into a sketch).
//!
//! The file is untrusted: the reader is plain safe Rust over group-code pairs, with caps
//! on the number of entities and polyline vertices, and every number must be finite.
//! It reads LINE, CIRCLE, ARC, LWPOLYLINE and POLYLINE/VERTEX (bulges become arcs) from
//! the ENTITIES section, in the file's units scaled to millimetres by `$INSUNITS`; other
//! entities are counted and skipped. Binary DXF is refused.

use std::collections::BTreeMap;

/// Most curves read from one file.
pub const MAX_CURVES: usize = 50_000;
/// Most vertices in one polyline.
pub const MAX_VERTICES: usize = 100_000;

/// A curve in millimetres. Arcs run counterclockwise from `start` to `end` (radians).
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    Line([f64; 2], [f64; 2]),
    Circle { center: [f64; 2], radius: f64 },
    Arc { center: [f64; 2], radius: f64, start: f64, end: f64 },
}

/// What was read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Read {
    pub curves: Vec<Curve>,
    /// Millimetres per drawing unit (from `$INSUNITS`; 1 when absent or unitless).
    pub unit_mm: f64,
    /// The file states its unit (`$INSUNITS` present and not 0, unitless).
    pub units_known: bool,
    /// Entity types that were skipped, with their counts.
    pub skipped: BTreeMap<String, usize>,
}

/// Why a file cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError(pub String);

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReadError {}

fn err(m: impl Into<String>) -> ReadError {
    ReadError(m.into())
}

/// Millimetres per unit for a `$INSUNITS` code.
fn unit_mm(code: i64) -> f64 {
    match code {
        1 => 25.4,
        2 => 304.8,
        3 => 1_609_344.0,
        4 => 1.0,
        5 => 10.0,
        6 => 1000.0,
        7 => 1_000_000.0,
        8 => 25.4e-6,
        9 => 25.4e-3,
        10 => 914.4,
        14 => 100.0,
        _ => 1.0,
    }
}

/// One entity's group codes, in order.
type Groups<'a> = Vec<(i32, &'a str)>;

fn num(v: &str) -> Result<f64, ReadError> {
    let x: f64 = v
        .trim()
        .parse()
        .map_err(|_| err(format!("'{}' is not a number", v.trim().chars().take(20).collect::<String>())))?;
    if x.is_finite() && x.abs() < 1e12 { Ok(x) } else { Err(err("a coordinate is out of range")) }
}

fn get<'a>(g: &Groups<'a>, code: i32) -> Option<&'a str> {
    g.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
}

fn need(g: &Groups<'_>, code: i32, what: &str) -> Result<f64, ReadError> {
    num(get(g, code).ok_or_else(|| err(format!("{what} lacks group {code}")))?)
}

/// The arc from `a` to `b` with DXF bulge `bulge` (tan of a quarter of the included
/// angle, positive counterclockwise), or a line.
fn bulge_curve(a: [f64; 2], b: [f64; 2], bulge: f64) -> Curve {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let chord = libm::hypot(dx, dy);
    // A sagitta below 1e-6 is a straight segment (review D10): the radius would be huge.
    if (bulge * chord / 2.0).abs() < 1e-6 || chord < 1e-12 {
        return Curve::Line(a, b);
    }
    let theta = 4.0 * libm::atan(bulge);
    let r = chord / (2.0 * libm::sin(theta.abs() / 2.0));
    // The centre lies off the chord's midpoint, on the left for a counterclockwise arc.
    let h = r * libm::cos(theta.abs() / 2.0);
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let s = if bulge > 0.0 { 1.0 } else { -1.0 };
    let left = [-dy / chord, dx / chord];
    let c = [m[0] + s * h * left[0], m[1] + s * h * left[1]];
    let ang = |p: [f64; 2]| libm::atan2(p[1] - c[1], p[0] - c[0]);
    let (start, end) = if bulge > 0.0 { (ang(a), ang(b)) } else { (ang(b), ang(a)) };
    Curve::Arc { center: c, radius: r.abs(), start, end }
}

/// The entity's extrusion direction (210/220/230): `Some(false)` for +Z, `Some(true)` for
/// -Z (the object coordinate system is mirrored in X, review D10), `None` when tilted.
fn ocs_mirror(g: &Groups<'_>) -> Result<Option<bool>, ReadError> {
    let n = |c| get(g, c).map(num).transpose();
    let (x, y, z) = (n(210)?.unwrap_or(0.0), n(220)?.unwrap_or(0.0), n(230)?.unwrap_or(1.0));
    let len = libm::sqrt(x * x + y * y + z * z);
    if len.is_nan() || len <= 0.0 || libm::hypot(x, y) > 1e-9 * len {
        return Ok(None);
    }
    Ok(Some(z < 0.0))
}

/// `c` seen through an object coordinate system mirrored in X.
fn mirror_x(c: Curve) -> Curve {
    use std::f64::consts::PI;
    match c {
        Curve::Line(a, b) => Curve::Line([-a[0], a[1]], [-b[0], b[1]]),
        Curve::Circle { center, radius } => Curve::Circle { center: [-center[0], center[1]], radius },
        // A mirror reverses the sweep: the counterclockwise arc runs from π-end to π-start.
        Curve::Arc { center, radius, start, end } => {
            Curve::Arc { center: [-center[0], center[1]], radius, start: PI - end, end: PI - start }
        }
    }
}

/// Maps `curves[from..]` from an object coordinate system to the world, or drops them
/// (counted as skipped) when it is tilted out of the XY plane.
fn to_world(
    curves: &mut Vec<Curve>,
    from: usize,
    mirror: Option<bool>,
    kind: &str,
    skipped: &mut BTreeMap<String, usize>,
) {
    match mirror {
        Some(false) => {}
        Some(true) => {
            for c in curves.iter_mut().skip(from) {
                *c = mirror_x(c.clone());
            }
        }
        None => {
            if curves.len() > from {
                curves.truncate(from);
                *skipped.entry(format!("{kind} (tilted)")).or_default() += 1;
            }
        }
    }
}

fn polyline(pts: &[([f64; 2], f64)], closed: bool, out: &mut Vec<Curve>) {
    for w in pts.windows(2) {
        if let [(a, bulge), (b, _)] = w {
            out.push(bulge_curve(*a, *b, *bulge));
        }
    }
    if closed
        && pts.len() > 2
        && let (Some((a, bulge)), Some((b, _))) = (pts.last(), pts.first())
    {
        out.push(bulge_curve(*a, *b, *bulge));
    }
}

/// Most entities (read or skipped) in one file.
pub const MAX_ENTITIES: usize = 1_000_000;
/// Most groups kept for one entity.
const MAX_GROUPS: usize = 4 * MAX_VERTICES + 64;

/// Entity types whose groups are kept.
fn wanted(kind: &str) -> bool {
    matches!(kind, "LINE" | "CIRCLE" | "ARC" | "LWPOLYLINE" | "POLYLINE" | "VERTEX" | "SEQEND")
}

/// A polyline vertex and the bulge to the next one.
type Vertex = ([f64; 2], f64);

/// The entity collected so far, and an old-style polyline waiting for its vertices.
#[derive(Default)]
struct State<'a> {
    kind: Option<&'a str>,
    groups: Groups<'a>,
    /// Closed, the polyline's mirror flag (`None` tilted), and its vertices.
    poly: Option<(bool, Option<bool>, Vec<Vertex>)>,
}

/// Finishes the current entity.
fn finish<'a>(
    st: &mut State<'a>,
    curves: &mut Vec<Curve>,
    skipped: &mut BTreeMap<String, usize>,
) -> Result<(), ReadError> {
    let Some(kind) = st.kind.take() else { return Ok(()) };
    let g = std::mem::take(&mut st.groups);
    // An old-style polyline ends at anything that is not one of its vertices.
    if kind != "VERTEX"
        && let Some((closed, mirror, pts)) = st.poly.take()
    {
        let from = curves.len();
        polyline(&pts, closed, curves);
        to_world(curves, from, mirror, "POLYLINE", skipped);
    }
    // LINE is in world coordinates; the others in their object coordinate system.
    let from = curves.len();
    let mirror =
        if matches!(kind, "CIRCLE" | "ARC" | "LWPOLYLINE" | "POLYLINE") { ocs_mirror(&g)? } else { Some(false) };
    match kind {
        "LINE" => curves.push(Curve::Line(
            [need(&g, 10, "LINE")?, need(&g, 20, "LINE")?],
            [need(&g, 11, "LINE")?, need(&g, 21, "LINE")?],
        )),
        "CIRCLE" => {
            let r = need(&g, 40, "CIRCLE")?;
            if r > 0.0 {
                curves.push(Curve::Circle { center: [need(&g, 10, "CIRCLE")?, need(&g, 20, "CIRCLE")?], radius: r });
            }
        }
        "ARC" => {
            let r = need(&g, 40, "ARC")?;
            if r > 0.0 {
                curves.push(Curve::Arc {
                    center: [need(&g, 10, "ARC")?, need(&g, 20, "ARC")?],
                    radius: r,
                    start: need(&g, 50, "ARC")?.to_radians(),
                    end: need(&g, 51, "ARC")?.to_radians(),
                });
            }
        }
        "LWPOLYLINE" => {
            let closed = get(&g, 70).and_then(|v| v.trim().parse::<i64>().ok()).is_some_and(|f| f & 1 == 1);
            let mut pts: Vec<([f64; 2], f64)> = Vec::new();
            let mut x = None;
            for (code, v) in &g {
                match code {
                    10 => x = Some(num(v)?),
                    20 => {
                        let px = x.take().ok_or_else(|| err("LWPOLYLINE: a Y without its X"))?;
                        if pts.len() >= MAX_VERTICES {
                            return Err(err("a polyline has too many vertices"));
                        }
                        pts.push(([px, num(v)?], 0.0));
                    }
                    42 => {
                        if let Some(last) = pts.last_mut() {
                            last.1 = num(v)?;
                        }
                    }
                    _ => {}
                }
            }
            polyline(&pts, closed, curves);
        }
        "POLYLINE" => {
            let closed = get(&g, 70).and_then(|v| v.trim().parse::<i64>().ok()).is_some_and(|f| f & 1 == 1);
            st.poly = Some((closed, mirror, Vec::new()));
        }
        "VERTEX" => {
            if let Some((_, _, pts)) = st.poly.as_mut() {
                if pts.len() >= MAX_VERTICES {
                    return Err(err("a polyline has too many vertices"));
                }
                let b = get(&g, 42).map(num).transpose()?.unwrap_or(0.0);
                pts.push(([need(&g, 10, "VERTEX")?, need(&g, 20, "VERTEX")?], b));
            }
        }
        "SEQEND" => {}
        other => *skipped.entry(other.chars().take(32).collect()).or_default() += 1,
    }
    to_world(curves, from, mirror, kind, skipped);
    if curves.len() > MAX_CURVES {
        return Err(err(format!("the file has more than {MAX_CURVES} curves")));
    }
    Ok(())
}

/// Reads the geometry of `bytes`. The group-code pairs are streamed: only the current
/// entity's groups are kept, so memory stays near the file size (review D6).
///
/// # Errors
/// Binary or malformed DXF, numbers out of range, too many curves or entities.
pub fn read(bytes: &[u8]) -> Result<Read, ReadError> {
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err(err("binary DXF is not supported; save the file as ASCII DXF"));
    }
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let mut out = Read { unit_mm: 1.0, ..Read::default() };
    let mut curves = Vec::new();
    let (mut prev, mut in_entities, mut seen_entities) = ((0i32, ""), false, false);
    let mut st = State::default();
    let mut entities = 0usize;
    while let Some(code) = lines.next() {
        let Some(value) = lines.next() else { break };
        let code: i32 = code
            .trim()
            .parse()
            .map_err(|_| err(format!("bad group code '{}'", code.trim().chars().take(20).collect::<String>())))?;
        if !in_entities {
            match (prev, code) {
                ((9, name), 70) if name.trim() == "$INSUNITS" => {
                    let code = value.trim().parse().unwrap_or(0);
                    out.unit_mm = unit_mm(code);
                    out.units_known = code != 0;
                }
                ((0, s), 2) if s.trim() == "SECTION" && value.trim() == "ENTITIES" => {
                    in_entities = true;
                    seen_entities = true;
                }
                _ => {}
            }
            prev = (code, value);
            continue;
        }
        if code == 0 {
            finish(&mut st, &mut curves, &mut out.skipped)?;
            let kind = value.trim();
            if kind == "ENDSEC" {
                in_entities = false;
                prev = (0, value);
                continue;
            }
            entities += 1;
            if entities > MAX_ENTITIES {
                return Err(err(format!("the file has more than {MAX_ENTITIES} entities")));
            }
            st.kind = Some(kind);
        } else if st.kind.is_some_and(wanted) {
            if st.groups.len() >= MAX_GROUPS {
                return Err(err("an entity has too many groups"));
            }
            st.groups.push((code, value));
        }
    }
    finish(&mut st, &mut curves, &mut out.skipped)?;
    if let Some((closed, mirror, pts)) = st.poly.take() {
        let from = curves.len();
        polyline(&pts, closed, &mut curves);
        to_world(&mut curves, from, mirror, "POLYLINE", &mut out.skipped);
    }
    if !seen_entities {
        return Err(err("the file has no ENTITIES section"));
    }
    let k = out.unit_mm;
    out.curves = curves
        .into_iter()
        .map(|c| match c {
            Curve::Line(a, b) => Curve::Line([a[0] * k, a[1] * k], [b[0] * k, b[1] * k]),
            Curve::Circle { center, radius } => {
                Curve::Circle { center: [center[0] * k, center[1] * k], radius: radius * k }
            }
            Curve::Arc { center, radius, start, end } => {
                Curve::Arc { center: [center[0] * k, center[1] * k], radius: radius * k, start, end }
            }
        })
        .collect();
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn our_own_exports_read_back() {
        let doc = "  0\nSECTION\n  2\nHEADER\n  9\n$INSUNITS\n 70\n1\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n\
  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 30\n0\n 11\n2\n 21\n0\n 31\n0\n\
  0\nCIRCLE\n  8\n0\n 10\n1\n 20\n1\n 40\n0.25\n\
  0\nARC\n 10\n0\n 20\n0\n 40\n1\n 50\n0\n 51\n90\n\
  0\nLWPOLYLINE\n 90\n2\n 70\n0\n 10\n0\n 20\n0\n 42\n1\n 10\n2\n 20\n0\n\
  0\nTEXT\n  1\nhello\n  0\nENDSEC\n  0\nEOF\n";
        let r = read(doc.as_bytes()).unwrap();
        assert_eq!(r.unit_mm, 25.4);
        assert_eq!(r.curves.len(), 4, "{r:?}");
        assert_eq!(r.curves[0], Curve::Line([0.0, 0.0], [50.8, 0.0]));
        assert_eq!(r.curves[1], Curve::Circle { center: [25.4, 25.4], radius: 6.35 });
        // A bulge of 1 is a half circle, counterclockwise from (0,0) to (2,0) in inches:
        // centre (1, 0), radius 1, below the chord.
        let Curve::Arc { center, radius, start, end } = r.curves[3] else { panic!("{r:?}") };
        assert!((center[0] - 25.4).abs() < 1e-9 && center[1].abs() < 1e-9 && (radius - 25.4).abs() < 1e-9);
        assert!((start.abs() - std::f64::consts::PI).abs() < 1e-9 && end.abs() < 1e-9, "{start} {end}");
        assert_eq!(r.skipped.get("TEXT"), Some(&1));
    }

    #[test]
    fn object_coordinate_systems_and_flat_bulges() {
        let doc = "  0\nSECTION\n  2\nENTITIES\n\
  0\nARC\n 10\n3\n 20\n1\n 40\n1\n 50\n0\n 51\n90\n210\n0\n220\n0\n230\n-1\n\
  0\nLINE\n 10\n1\n 20\n0\n 11\n2\n 21\n0\n230\n-1\n\
  0\nCIRCLE\n 10\n0\n 20\n0\n 40\n1\n210\n1\n220\n0\n230\n0\n\
  0\nLWPOLYLINE\n 10\n0\n 20\n0\n 42\n1e-7\n 10\n1\n 20\n0\n\
  0\nENDSEC\n  0\nEOF\n";
        let r = read(doc.as_bytes()).unwrap();
        assert_eq!(r.curves.len(), 3, "{r:?}");
        // Seen from below, the quarter arc at (3, 1) from 0° to 90° is the one at (-3, 1)
        // from 90° to 180°.
        let Curve::Arc { center, start, end, .. } = r.curves[0] else { panic!("{r:?}") };
        assert_eq!(center, [-3.0, 1.0]);
        let pi = std::f64::consts::PI;
        assert!((start - pi / 2.0).abs() < 1e-12 && (end - pi).abs() < 1e-12, "{start} {end}");
        // A LINE is in world coordinates whatever its extrusion.
        assert_eq!(r.curves[1], Curve::Line([1.0, 0.0], [2.0, 0.0]));
        // A tilted circle is skipped; a bulge with a sagitta of 5e-8 is a line.
        assert_eq!(r.skipped.get("CIRCLE (tilted)"), Some(&1));
        assert_eq!(r.curves[2], Curve::Line([0.0, 0.0], [1.0, 0.0]));
    }

    #[test]
    fn bad_files_are_refused_cleanly() {
        assert!(read(b"AutoCAD Binary DXF\r\n\x1a\0").is_err());
        assert!(read(b"  0\nSECTION\n  2\nHEADER\n  0\nENDSEC\n").is_err());
        assert!(read(b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n 10\nnan\n 20\n0\n 11\n1\n 21\n1\n").is_err());
        assert!(read(b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n 10\n1\n").is_err());
        assert!(read(b"x\ny\n").is_err());
    }

    #[test]
    fn mutated_files_never_panic() {
        let base = b"  0\nSECTION\n  2\nENTITIES\n  0\nLWPOLYLINE\n 90\n3\n 70\n1\n 10\n0\n 20\n0\n 42\n0.5\n 10\n1\n 20\n0\n 10\n1\n 20\n1\n  0\nPOLYLINE\n 70\n1\n  0\nVERTEX\n 10\n0\n 20\n0\n 42\n-1\n  0\nVERTEX\n 10\n2\n 20\n0\n  0\nSEQEND\n  0\nARC\n 10\n0\n 20\n0\n 40\n1\n 50\n10\n 51\n20\n  0\nENDSEC\n";
        assert!(read(base).is_ok());
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..20_000 {
            let mut v = base.to_vec();
            for _ in 0..1 + next() % 4 {
                let i = (next() % v.len() as u64) as usize;
                let b = [b'0', b'9', b'-', b'.', b'e', b'\n', b' ', b'1', 0xff][(next() % 9) as usize];
                match next() % 3 {
                    0 => v[i] = b,
                    1 => v.insert(i, b),
                    _ => {
                        v.remove(i);
                    }
                }
            }
            if let Ok(r) = read(&v) {
                for c in &r.curves {
                    assert!(format!("{c:?}").find("NaN").is_none(), "{c:?}");
                }
            }
        }
    }
}
