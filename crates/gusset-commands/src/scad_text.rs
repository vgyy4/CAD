//! OpenSCAD to the code view (ADR 0033 amendment, spec: ".scad, translate to our code view,
//! tier 1"): each top-level object of an evaluated `.scad` file becomes editable features.
//! Every primitive is drawn at the origin (a sketch and an extrude or revolve), turned and
//! moved to its place by a `move`, and joined by `combine` the way the file's `union`,
//! `difference` and `intersection` join it. An object that uses something the features
//! cannot say (a mirror, an uneven scale, a skew, too many curves) is left to the exact
//! import, which builds it as one body.

use std::collections::HashSet;
use std::fmt::Write as _;

use gusset_scad::{Csg, M4, Op, Prim, Shape2};

/// Most sketch curves one object may turn into.
const MAX_CURVES: usize = 5000;
/// Most features one object may turn into.
const MAX_FEATURES: usize = 2000;

/// Checks an object's features on their own: they must rebuild without errors and give one
/// body of the exact build's volume (within a millionth). The names in the check are scratch
/// names; the import writes the same features under names free in the document.
///
/// # Errors
/// Why the features do not stand in for the exact build.
pub(crate) fn check(
    c: &Csg,
    kernel: std::sync::Arc<dyn gusset_kernel::Kernel>,
    volume: f64,
    bbox: Option<gusset_kernel::BBox>,
) -> Result<(), String> {
    let mut names = Names::new(HashSet::new());
    let (text, body) = object_text(c, &mut names)?;
    let doc = gusset_doc::text::parse(&format!("gusset 1\nunits mm\n\n{text}"))
        .map_err(|e| format!("the features do not parse ({e})"))?;
    let k = kernel.clone();
    let ev = gusset_eval::Evaluator::new(kernel).evaluate(&doc, None, &());
    if let Some(e) = ev.errors().first() {
        return Err(format!("the features do not rebuild ({e})"));
    }
    let made = ev.bodies.iter().find(|b| b.feature == body).ok_or("the features make no body")?;
    let got = k.mass_properties(&made.shape).map(|m| m.volume).map_err(|e| e.message)?;
    // Where it is, not only how big: the boxes agree within a millionth of their size.
    let want = bbox.ok_or("the exact build has no box")?;
    let have = k.bbox(&made.shape).map_err(|e| e.message)?;
    let zip3 = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let size = zip3(want.max, want.min).into_iter().fold(1.0, f64::max);
    let off = zip3(have.min, want.min).into_iter().chain(zip3(have.max, want.max)).map(f64::abs).fold(0.0, f64::max);
    if off.is_nan() || off > 1e-6 * size {
        return Err(format!("the features land {off:.6} mm away from the exact build"));
    }
    if !volume.is_finite() || (got - volume).abs() > 1e-6 * volume.abs().max(1.0) {
        return Err(format!("the features give {got:.4} mm3, the exact build {volume:.4} mm3"));
    }
    if ev.bodies.len() != 1 {
        return Err(format!("the features make {} bodies, not one", ev.bodies.len()));
    }
    Ok(())
}

/// Feature names in use (lower case) and, per base name, where to look for the next free one,
/// so naming many objects stays linear (security review S3).
pub(crate) struct Names {
    taken: HashSet<String>,
    next: std::collections::HashMap<String, usize>,
}

impl Names {
    pub(crate) fn new(taken: HashSet<String>) -> Names {
        Names { taken, next: std::collections::HashMap::new() }
    }

    fn fresh(&mut self, base: &str) -> String {
        let start = self.next.get(base).copied().unwrap_or(1);
        for i in start.. {
            let name = format!("{base}{i}");
            if self.taken.insert(name.to_ascii_lowercase()) {
                self.next.insert(base.to_string(), i + 1);
                return name;
            }
        }
        unreachable!("an unbounded counter always finds a free name")
    }
}

/// Writes one object's features, naming them so they clash with nothing in `taken` (lower
/// case), which grows with the names used. Returns the statements and the feature that made
/// the object's body.
///
/// # Errors
/// Why the object cannot be written as features (the exact import takes it then).
pub(crate) fn object_text(c: &Csg, names: &mut Names) -> Result<(String, String), String> {
    let mut g = Gen { names, out: String::new(), curves: 0, features: 0 };
    let body = g.csg(c)?;
    Ok((g.out, body))
}

struct Gen<'a> {
    names: &'a mut Names,
    out: String,
    curves: usize,
    features: usize,
}

/// A number for the text: finite and not absurd, at most nine decimals, no trailing zeros.
fn num(v: f64) -> Result<String, String> {
    if !v.is_finite() || v.abs() > 1e9 {
        return Err("a dimension is not a usable number".into());
    }
    if v.abs() < 1e-12 {
        return Ok("0".into());
    }
    let s = format!("{v:.9}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    Ok(if s == "-0" { "0".into() } else { s.to_string() })
}

/// A placement split into an even scale, a turn (unit axis, degrees) and a shift.
struct Place {
    scale: f64,
    turn: Option<([f64; 3], f64)>,
    shift: [f64; 3],
}

fn place(m: &M4) -> Result<Place, String> {
    let r = |i: usize, j: usize| m.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0.0);
    let col = |j: usize| [r(0, j), r(1, j), r(2, j)];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross =
        |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    if (r(3, 0).abs() + r(3, 1).abs() + r(3, 2).abs() + (r(3, 3) - 1.0).abs()) > 1e-12 {
        return Err("a projective transform cannot be written as a move".into());
    }
    let (x, y, z) = (col(0), col(1), col(2));
    let det = dot(cross(x, y), z);
    if !det.is_finite() || det <= 1e-18 {
        return Err("a mirror or flattening transform cannot be written as a move".into());
    }
    let s = libm::cbrt(det);
    // An even scale times a rotation: columns of length s, square to each other.
    let tol = 1e-9 * s.max(1.0);
    let ok = [x, y, z].iter().all(|c| (dot(*c, *c).sqrt() - s).abs() < tol)
        && dot(x, y).abs() < tol * s
        && dot(y, z).abs() < tol * s
        && dot(x, z).abs() < tol * s;
    if !ok {
        return Err("an uneven scale or a skew cannot be written as features".into());
    }
    let q = |i: usize, j: usize| r(i, j) / s;
    let trace = q(0, 0) + q(1, 1) + q(2, 2);
    let angle = libm::acos(((trace - 1.0) / 2.0).clamp(-1.0, 1.0));
    let turn = if angle < 1e-12 {
        None
    } else {
        let v = [q(2, 1) - q(1, 2), q(0, 2) - q(2, 0), q(1, 0) - q(0, 1)];
        let n = dot(v, v).sqrt();
        let axis = if n > 1e-9 {
            v.map(|c| c / n)
        } else {
            // Half a turn: the axis from the largest diagonal entry of (Q + I) / 2.
            let d = [(q(0, 0) + 1.0) / 2.0, (q(1, 1) + 1.0) / 2.0, (q(2, 2) + 1.0) / 2.0];
            let k = if d[0] >= d[1] && d[0] >= d[2] {
                0
            } else if d[1] >= d[2] {
                1
            } else {
                2
            };
            let dk = d.get(k).copied().unwrap_or(1.0).max(1e-18).sqrt();
            let a = match k {
                0 => [dk, q(0, 1) / (2.0 * dk), q(0, 2) / (2.0 * dk)],
                1 => [q(0, 1) / (2.0 * dk), dk, q(1, 2) / (2.0 * dk)],
                _ => [q(0, 2) / (2.0 * dk), q(1, 2) / (2.0 * dk), dk],
            };
            let n = dot(a, a).sqrt().max(1e-18);
            a.map(|c| c / n)
        };
        Some((axis, angle.to_degrees()))
    };
    Ok(Place { scale: s, turn, shift: [r(0, 3), r(1, 3), r(2, 3)] })
}

impl Gen<'_> {
    fn fresh(&mut self, base: &str) -> String {
        self.names.fresh(base)
    }

    fn count(&mut self, curves: usize) -> Result<(), String> {
        self.curves += curves;
        self.features += 1;
        if self.curves > MAX_CURVES || self.features > MAX_FEATURES {
            return Err("the object has too many curves or features to write out".into());
        }
        Ok(())
    }

    fn csg(&mut self, c: &Csg) -> Result<String, String> {
        match c {
            Csg::Leaf { prim, at, .. } => self.leaf(prim, at),
            Csg::Op(op, kids) => {
                let mut names = Vec::with_capacity(kids.len());
                for k in kids {
                    names.push(self.csg(k)?);
                }
                let (first, rest) = names.split_first().ok_or("an empty boolean operation")?;
                if rest.is_empty() {
                    return Ok(first.clone());
                }
                let op = match op {
                    Op::Union => "add",
                    Op::Difference => "cut",
                    Op::Intersection => "intersect",
                };
                self.count(0)?;
                let name = self.fresh("Combine");
                let _ = writeln!(self.out, "combine {name} (body: {first}, tools: [{}], op: {op})", rest.join(", "));
                Ok(first.clone())
            }
        }
    }

    fn leaf(&mut self, prim: &Prim, at: &M4) -> Result<String, String> {
        let p = place(at)?;
        let s = p.scale;
        let body = match prim {
            Prim::Cube { size, center } => {
                let [sx, sy, sz] = size.map(|v| v * s);
                if !(sx > 0.0 && sy > 0.0 && sz > 0.0) {
                    return Err("a cube without volume".into());
                }
                let (x0, y0) = if *center { (-sx / 2.0, -sy / 2.0) } else { (0.0, 0.0) };
                let sk = self.rect_sketch([x0, y0], [sx, sy])?;
                self.extrude(&sk, &sk, sz, *center, "Cube")?
            }
            Prim::Cylinder { h, r1, r2, center } => {
                let (h, r1, r2) = (h * s, r1 * s, r2 * s);
                if !(h > 0.0 && r1 >= 0.0 && r2 >= 0.0 && r1.max(r2) > 0.0) {
                    return Err("a cylinder without volume".into());
                }
                if (r1 - r2).abs() <= 1e-12 * r1.max(1.0) {
                    let sk = self.circle_sketch("XY", [0.0, 0.0], r1)?;
                    self.extrude(&sk, &sk, h, *center, "Cylinder")?
                } else {
                    let z0 = if *center { -h / 2.0 } else { 0.0 };
                    let mut pts = vec![[0.0, z0]];
                    if r1 > 0.0 {
                        pts.push([r1, z0]);
                    }
                    if r2 > 0.0 {
                        pts.push([r2, z0 + h]);
                    }
                    pts.push([0.0, z0 + h]);
                    let sk = self.polygon_sketch("XZ", &[pts])?;
                    self.revolve(&sk.0, &sk.1, 360.0, "Cone")?
                }
            }
            Prim::Sphere { radius } => {
                let r = radius * s;
                if r <= 0.0 {
                    return Err("a sphere without volume".into());
                }
                let sk = self.fresh("SphereSketch");
                self.count(2)?;
                let (rn, mn) = (num(r)?, num(-r)?);
                let _ = writeln!(
                    self.out,
                    "sketch {sk} (plane: XZ) {{\n  p1 = point(0, {mn})\n  p2 = point(0, {rn})\n  p3 = point(0, 0)\n  a4 = arc(p3, p1, p2)\n  l5 = line(p2, p1)\n  k6 = at_x(p3, 0 mm)\n  k7 = at_y(p3, 0 mm)\n  k8 = on_line(p3, l5)\n  k9 = vertical(l5)\n  k10 = radius(a4, {rn} mm)\n}}"
                );
                self.revolve(&sk, &sk, 360.0, "Sphere")?
            }
            Prim::Prism { shape, h, center } => {
                let h = h * s;
                if h <= 0.0 {
                    return Err("an extrusion without height".into());
                }
                let (sk, profile) = self.shape_sketch("XY", shape, s)?;
                self.extrude(&sk, &profile, h, *center, "Extrude")?
            }
            Prim::Revolve { shape, angle } => {
                if !(angle.is_finite() && *angle > 0.0 && *angle <= 360.0) {
                    return Err("a rotate_extrude angle outside 0 to 360 degrees".into());
                }
                let (sk, profile) = self.shape_sketch("XZ", shape, s)?;
                self.revolve(&sk, &profile, *angle, "Revolve")?
            }
        };
        let moved = p.turn.is_some() || p.shift.iter().any(|v| v.abs() > 1e-12);
        if moved {
            self.count(0)?;
            let name = self.fresh("Place");
            let [tx, ty, tz] = [num(p.shift[0])?, num(p.shift[1])?, num(p.shift[2])?];
            let mut line = format!("move {name} (body: {body}, translate: [{tx} mm, {ty} mm, {tz} mm]");
            if let Some((a, deg)) = p.turn {
                let _ =
                    write!(line, ", axis: [{}, {}, {}], angle: {} deg", num(a[0])?, num(a[1])?, num(a[2])?, num(deg)?);
            }
            let _ = writeln!(self.out, "{line})");
        }
        Ok(body)
    }

    /// A sketch of `shape` (scaled by `s`) on `plane`; returns the sketch and its profile.
    fn shape_sketch(&mut self, plane: &str, shape: &Shape2, s: f64) -> Result<(String, String), String> {
        match shape {
            Shape2::Circle { center, radius } => {
                let sk = self.circle_sketch(plane, center.map(|v| v * s), radius * s)?;
                Ok((sk.clone(), sk))
            }
            Shape2::Polygon(paths) => {
                let scaled: Vec<Vec<[f64; 2]>> =
                    paths.iter().map(|p| p.iter().map(|q| q.map(|v| v * s)).collect()).collect();
                self.polygon_sketch(plane, &scaled)
            }
        }
    }

    /// A rectangle with its corner, sides and size as constraints: editing the width or the
    /// height in the text resizes it.
    fn rect_sketch(&mut self, at: [f64; 2], size: [f64; 2]) -> Result<String, String> {
        self.count(4)?;
        let sk = self.fresh("Sketch");
        let [x0, y0] = at;
        let [w, h] = size;
        let (a, b, c, d) = (num(x0)?, num(y0)?, num(x0 + w)?, num(y0 + h)?);
        let (wn, hn) = (num(w)?, num(h)?);
        let _ = write!(
            self.out,
            "sketch {sk} (plane: XY) {{\n  p1 = point({a}, {b})\n  p2 = point({c}, {b})\n  p3 = point({c}, {d})\n  p4 = point({a}, {d})\n  \
             l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n  \
             k9 = horizontal(l5)\n  k10 = horizontal(l7)\n  k11 = vertical(l6)\n  k12 = vertical(l8)\n  \
             k13 = at_x(p1, {a} mm)\n  k14 = at_y(p1, {b} mm)\n  k15 = h_distance(p1, p2, {wn} mm)\n  k16 = v_distance(p2, p3, {hn} mm)\n}}\n"
        );
        Ok(sk)
    }

    fn circle_sketch(&mut self, plane: &str, c: [f64; 2], r: f64) -> Result<String, String> {
        if r.is_nan() || r <= 0.0 {
            return Err("a circle without radius".into());
        }
        self.count(1)?;
        let sk = self.fresh("Sketch");
        let _ = writeln!(
            self.out,
            "sketch {sk} (plane: {plane}) {{\n  p1 = point({x}, {y})\n  c2 = circle(p1, {r})\n  k3 = at_x(p1, {x} mm)\n  k4 = at_y(p1, {y} mm)\n  k5 = radius(c2, {r} mm)\n}}",
            x = num(c[0])?,
            y = num(c[1])?,
            r = num(r)?
        );
        Ok(sk)
    }

    /// Closed outlines (the first outside, the rest holes) as lines; the profile is the
    /// region the outside's first line bounds, so holes stay open.
    fn polygon_sketch(&mut self, plane: &str, paths: &[Vec<[f64; 2]>]) -> Result<(String, String), String> {
        let sk = self.fresh("Sketch");
        let mut body = String::new();
        let mut id = 0u32;
        let mut first_line = None;
        for path in paths {
            // Repeated points (also a closing repeat of the first) would make empty lines.
            let mut pts: Vec<[f64; 2]> = Vec::with_capacity(path.len());
            for p in path {
                if pts.last().is_none_or(|q: &[f64; 2]| (q[0] - p[0]).abs() + (q[1] - p[1]).abs() > 1e-12) {
                    pts.push(*p);
                }
            }
            while pts.len() > 1
                && pts.first().zip(pts.last()).is_some_and(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() <= 1e-12)
            {
                pts.pop();
            }
            if pts.len() < 3 {
                return Err("an outline with fewer than three points".into());
            }
            self.count(pts.len())?;
            let start = id + 1;
            for p in &pts {
                id += 1;
                let _ = writeln!(body, "  p{id} = point({}, {})", num(p[0])?, num(p[1])?);
            }
            let n = pts.len() as u32;
            for k in 0..n {
                id += 1;
                first_line.get_or_insert(id);
                let _ = writeln!(body, "  l{id} = line(p{}, p{})", start + k, start + (k + 1) % n);
            }
            // Every corner at its place: the outline is fully defined, and a coordinate edited
            // in the text moves that corner only.
            for (k, p) in pts.iter().enumerate() {
                let pid = start + k as u32;
                let _ = writeln!(body, "  k{} = at_x(p{pid}, {} mm)", id + 1, num(p[0])?);
                let _ = writeln!(body, "  k{} = at_y(p{pid}, {} mm)", id + 2, num(p[1])?);
                id += 2;
            }
        }
        let _ = write!(self.out, "sketch {sk} (plane: {plane}) {{\n{body}}}\n");
        let profile = match (paths.len(), first_line) {
            (1, _) | (_, None) => sk.clone(),
            (_, Some(l)) => format!("{sk}.regions(l{l})"),
        };
        Ok((sk, profile))
    }

    fn extrude(&mut self, _sketch: &str, profile: &str, h: f64, centered: bool, base: &str) -> Result<String, String> {
        self.count(0)?;
        let name = self.fresh(base);
        let sym = if centered { ", symmetric: true" } else { "" };
        let _ = writeln!(self.out, "extrude {name} (profile: {profile}, distance: {} mm{sym}, op: new)", num(h)?);
        Ok(name)
    }

    /// Revolves an XZ-plane profile about the world Z axis, which is the sketch's own Y
    /// (`axis:` names the sketch's axes).
    fn revolve(&mut self, _sketch: &str, profile: &str, angle: f64, base: &str) -> Result<String, String> {
        self.count(0)?;
        let name = self.fresh(base);
        let _ = writeln!(self.out, "revolve {name} (profile: {profile}, axis: Y, angle: {} deg, op: new)", num(angle)?);
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot_z(deg: f64) -> M4 {
        let (s, c) = (libm::sin(deg.to_radians()), libm::cos(deg.to_radians()));
        [[c, -s, 0.0, 1.0], [s, c, 0.0, 2.0], [0.0, 0.0, 1.0, 3.0], [0.0, 0.0, 0.0, 1.0]]
    }

    #[test]
    fn placements_split_into_scale_turn_and_shift() {
        let p = place(&rot_z(90.0)).unwrap();
        let (axis, deg) = p.turn.unwrap();
        assert!((deg - 90.0).abs() < 1e-9 && (axis[2] - 1.0).abs() < 1e-9, "{axis:?} {deg}");
        assert_eq!(p.shift.map(f64::to_bits), [1.0f64, 2.0, 3.0].map(f64::to_bits));
        // Half a turn about X.
        let half = [[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let (axis, deg) = place(&half).unwrap().turn.unwrap();
        assert!((deg - 180.0).abs() < 1e-9 && (axis[0].abs() - 1.0).abs() < 1e-9, "{axis:?}");
        // An even scale is kept; a mirror and an uneven scale are refused.
        let even = [[2.0, 0.0, 0.0, 0.0], [0.0, 2.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert!((place(&even).unwrap().scale - 2.0).abs() < 1e-12);
        let mirror = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert!(place(&mirror).is_err());
        let uneven = [[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert!(place(&uneven).is_err());
        let nan = [[f64::NAN, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert!(place(&nan).is_err());
    }

    #[test]
    fn a_program_becomes_features_that_parse() {
        let src = "difference() { translate([1,2,3]) rotate([0,0,30]) cube([10,20,5], center=true); \
                   cylinder(h=8, r=2); } sphere(4); cylinder(h=5, r1=3, r2=0, center=true); \
                   linear_extrude(4) difference() { square(10); translate([5,5]) circle(2); } \
                   rotate_extrude(angle=90) translate([5,0]) square([1,2]);";
        let model = gusset_scad::read(src).unwrap();
        let mut taken = Names::new(["sketch1".to_string()].into_iter().collect());
        let mut text = String::from("gusset 1\nunits mm\n\n");
        for o in &model.objects {
            let (t, body) = object_text(o, &mut taken).unwrap();
            assert!(t.contains(&body), "{t}");
            text.push_str(&t);
        }
        assert!(!text.contains("sketch Sketch1 "), "a taken name was reused:\n{text}");
        let doc = gusset_doc::text::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert!(doc.features.iter().any(|f| f.name.starts_with("Combine")), "{text}");
        assert!(text.contains("move Place"), "{text}");
        assert!(text.contains("angle: 90 deg"), "{text}");
    }

    #[test]
    fn what_features_cannot_say_is_refused() {
        let model = gusset_scad::read("mirror([1,0,0]) cube(2); scale([1,2,1]) cube(2);").unwrap();
        let mut taken = Names::new(HashSet::new());
        for o in &model.objects {
            assert!(object_text(o, &mut taken).is_err());
        }
    }
}
