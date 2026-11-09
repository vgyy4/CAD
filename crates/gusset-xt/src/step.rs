//! STEP AP214 text from the model (ADR 0035), in millimetres (XT model space is metres).
//! Exact for planes, cylinders, cones, spheres, doughnut tori, B-surfaces, swept, spun and
//! offset surfaces, and for lines, circles, ellipses and B-curves; intersection curves are
//! fitted through their chart points; blends, SP-curves and foreign geometry are skipped
//! with a warning (the importer heals what it can).

use crate::decode::Graph;
use crate::geom::{self, Nurbs, V3};
use crate::model::{BodyKind, Edge, Face, Loop, Model, Shell};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Metres to millimetres.
const MM: f64 = 1000.0;

/// A STEP real: always with a decimal point.
fn real(v: f64) -> String {
    if !v.is_finite() {
        return "0.".into();
    }
    let s = format!("{v:?}");
    let (mant, exp) = match s.split_once('e') {
        Some((m, e)) => (m.to_string(), Some(e.to_string())),
        None => (s, None),
    };
    let mant = if mant.contains('.') { mant } else { format!("{mant}.") };
    match exp {
        Some(e) => format!("{mant}E{e}"),
        None => mant,
    }
}

fn string(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .map(|c| if c == '\'' || c == '\\' { ' ' } else { c })
        .collect();
    format!("'{clean}'")
}

fn list(ids: &[usize]) -> String {
    let v: Vec<String> = ids.iter().map(|i| format!("#{i}")).collect();
    format!("({})", v.join(","))
}

fn flag(b: bool) -> &'static str {
    if b { ".T." } else { ".F." }
}

struct Writer<'a> {
    g: &'a Graph,
    out: String,
    next: usize,
    curves: BTreeMap<u32, Option<usize>>,
    surfaces: BTreeMap<u32, Option<(usize, bool)>>,
    vertices: BTreeMap<u32, usize>,
    edges: BTreeMap<u32, Option<usize>>,
    warnings: Vec<String>,
    /// Vertex positions of the faces on each surface (bounds for unbounded geometry).
    hints: BTreeMap<u32, Vec<V3>>,
    /// The placement of the body being written, and its length scale.
    xf: Option<crate::model::Xf>,
    scale: f64,
    /// Every face written: its STEP id and its XT node (for face colours).
    faces: Vec<(usize, u32)>,
}

impl Writer<'_> {
    fn add(&mut self, body: &str) -> usize {
        let id = self.next;
        self.next += 1;
        let _ = writeln!(self.out, "#{id}={body};");
        id
    }

    fn point(&mut self, p: V3) -> usize {
        let p = match &self.xf {
            Some((m, t)) => geom::add(crate::model::mat_vec(m, p), *t),
            None => p,
        };
        self.add(&format!("CARTESIAN_POINT('',({},{},{}))", real(p[0] * MM), real(p[1] * MM), real(p[2] * MM)))
    }

    /// Millimetres per model unit, with the placement's scale.
    fn mm(&self) -> f64 {
        MM * self.scale
    }

    fn dir(&mut self, d: V3) -> usize {
        let d = match &self.xf {
            Some((m, _)) => crate::model::mat_vec(m, d),
            None => d,
        };
        let d = geom::unit(d).unwrap_or([0.0, 0.0, 1.0]);
        self.add(&format!("DIRECTION('',({},{},{}))", real(d[0]), real(d[1]), real(d[2])))
    }

    /// An axis placement; `x` is made perpendicular to `z`.
    fn place(&mut self, at: V3, z: V3, x: V3) -> usize {
        let zu = geom::unit(z).unwrap_or([0.0, 0.0, 1.0]);
        let xp = geom::sub(x, geom::scale(zu, geom::dot(x, zu)));
        let xu = geom::unit(xp).unwrap_or_else(|| {
            let t = if zu[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            geom::unit(geom::cross(zu, t)).unwrap_or([1.0, 0.0, 0.0])
        });
        let (p, z, x) = (self.point(at), self.dir(zu), self.dir(xu));
        self.add(&format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x})"))
    }

    fn bspline(&mut self, c: &Nurbs) -> usize {
        let pts: Vec<usize> = c.poles.iter().map(|p| self.point(*p)).collect();
        let ints = |v: &[i64]| v.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let reals = |v: &[f64]| v.iter().map(|x| real(*x)).collect::<Vec<_>>().join(",");
        let (m, k) = (ints(&c.mults[0]), reals(&c.knots[0]));
        match &c.weights {
            None => self.add(&format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',{},{},.UNSPECIFIED.,{},.F.,({m}),({k}),.UNSPECIFIED.)",
                c.degree[0],
                list(&pts),
                flag(c.closed[0])
            )),
            Some(w) => self.add(&format!(
                "(BOUNDED_CURVE() B_SPLINE_CURVE({},{},.UNSPECIFIED.,{},.F.) B_SPLINE_CURVE_WITH_KNOTS(({m}),({k}),.UNSPECIFIED.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({})) REPRESENTATION_ITEM(''))",
                c.degree[0],
                list(&pts),
                flag(c.closed[0]),
                reals(w)
            )),
        }
    }

    fn bspline_surface(&mut self, s: &Nurbs) -> usize {
        let mut rows = Vec::new();
        for iu in 0..s.n[0] {
            let row: Vec<usize> =
                (0..s.n[1]).filter_map(|iv| s.poles.get(iu * s.n[1] + iv).copied()).map(|p| self.point(p)).collect();
            rows.push(list(&row));
        }
        let ints = |v: &[i64]| v.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let reals = |v: &[f64]| v.iter().map(|x| real(*x)).collect::<Vec<_>>().join(",");
        let knots = format!(
            "({}),({}),({}),({}),.UNSPECIFIED.",
            ints(&s.mults[0]),
            ints(&s.mults[1]),
            reals(&s.knots[0]),
            reals(&s.knots[1])
        );
        let head = format!(
            "{},{},({}),.UNSPECIFIED.,{},{},.F.",
            s.degree[0],
            s.degree[1],
            rows.join(","),
            flag(s.closed[0]),
            flag(s.closed[1])
        );
        match &s.weights {
            None => self.add(&format!("B_SPLINE_SURFACE_WITH_KNOTS('',{head},{knots})")),
            Some(w) => {
                let wrows: Vec<String> = (0..s.n[0])
                    .map(|iu| {
                        let r: Vec<String> =
                            (0..s.n[1]).filter_map(|iv| w.get(iu * s.n[1] + iv)).map(|x| real(*x)).collect();
                        format!("({})", r.join(","))
                    })
                    .collect();
                self.add(&format!(
                    "(BOUNDED_SURFACE() B_SPLINE_SURFACE({head}) B_SPLINE_SURFACE_WITH_KNOTS({knots}) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(({})) REPRESENTATION_ITEM('') SURFACE())",
                    wrows.join(",")
                ))
            }
        }
    }

    fn curve(&mut self, c: u32) -> Option<usize> {
        if let Some(r) = self.curves.get(&c) {
            return *r;
        }
        self.curves.insert(c, None);
        let r = self.curve_uncached(c);
        if r.is_none() {
            let name = self.g.type_name(c).unwrap_or("?").to_string();
            self.warnings.push(format!("curve {c} ({name}) cannot be converted"));
        }
        self.curves.insert(c, r);
        r
    }

    fn curve_uncached(&mut self, c: u32) -> Option<usize> {
        let g = self.g;
        match g.type_name(c)? {
            "LINE" => {
                let (p, d) = (g.vec3(c, "pvec")?, g.vec3(c, "direction")?);
                let (p, d) = (self.point(p), self.dir(d));
                let v = self.add(&format!("VECTOR('',#{d},1.)"));
                Some(self.add(&format!("LINE('',#{p},#{v})")))
            }
            "CIRCLE" => {
                let a = self.place(g.vec3(c, "centre")?, g.vec3(c, "normal")?, g.vec3(c, "x_axis")?);
                Some(self.add(&format!("CIRCLE('',#{a},{})", real(g.real(c, "radius")? * self.mm()))))
            }
            "ELLIPSE" => {
                let a = self.place(g.vec3(c, "centre")?, g.vec3(c, "normal")?, g.vec3(c, "x_axis")?);
                let (r1, r2) = (g.real(c, "major_radius")? * self.mm(), g.real(c, "minor_radius")? * self.mm());
                Some(self.add(&format!("ELLIPSE('',#{a},{},{})", real(r1), real(r2))))
            }
            "B_CURVE" => {
                let n = geom::nurbs_curve(g, g.ptr(c, "nurbs"))?;
                Some(self.bspline(&n))
            }
            "TRIMMED_CURVE" => self.curve(g.ptr(c, "basis_curve")),
            "INTERSECTION" => {
                // The chart is a polyline accurate to its chordal error: fit a cubic.
                let chart = g.ptr(c, "chart");
                let pts: Vec<V3> = g
                    .list(chart, "hvec")
                    .iter()
                    .filter_map(|v| if let crate::reader::Val::Vec3(p) = v { Some(*p) } else { None })
                    .collect();
                let n = geom::through(&pts)?;
                Some(self.bspline(&n))
            }
            "SP_CURVE" => {
                let pts = crate::eval::curve_samples(g, c, 33, 0)?;
                Some(self.bspline(&geom::through(&pts)?))
            }
            _ => None,
        }
    }

    /// A surface and whether its natural normal is opposite to the XT surface's.
    fn surface(&mut self, s: u32) -> Option<(usize, bool)> {
        if let Some(r) = self.surfaces.get(&s) {
            return *r;
        }
        self.surfaces.insert(s, None);
        let r = self.surface_uncached(s);
        if r.is_none() {
            let name = self.g.type_name(s).unwrap_or("?").to_string();
            self.warnings.push(format!("surface {s} ({name}) cannot be converted; its faces are left out"));
        }
        self.surfaces.insert(s, r);
        r
    }

    fn surface_uncached(&mut self, s: u32) -> Option<(usize, bool)> {
        let g = self.g;
        match g.type_name(s)? {
            "PLANE" => {
                let a = self.place(g.vec3(s, "pvec")?, g.vec3(s, "normal")?, g.vec3(s, "x_axis")?);
                Some((self.add(&format!("PLANE('',#{a})")), false))
            }
            "CYLINDER" => {
                let a = self.place(g.vec3(s, "pvec")?, g.vec3(s, "axis")?, g.vec3(s, "x_axis")?);
                Some((
                    self.add(&format!("CYLINDRICAL_SURFACE('',#{a},{})", real(g.real(s, "radius")? * self.mm()))),
                    false,
                ))
            }
            "CONE" => {
                // XT's cone widens along its axis: R = P + vA + (r + v tan a)(...), as STEP's
                // cone placed on the same axis (checked on countersinks: read the other way,
                // the faces' own edges are off the surface and their volumes disagree with
                // their meshes).
                let axis = g.vec3(s, "axis")?;
                let a = self.place(g.vec3(s, "pvec")?, axis, g.vec3(s, "x_axis")?);
                let angle = libm::atan2(g.real(s, "sin_half_angle")?, g.real(s, "cos_half_angle")?);
                let r = g.real(s, "radius")? * self.mm();
                Some((self.add(&format!("CONICAL_SURFACE('',#{a},{},{})", real(r), real(angle))), false))
            }
            "SPHERE" => {
                let a = self.place(g.vec3(s, "centre")?, g.vec3(s, "axis")?, g.vec3(s, "x_axis")?);
                Some((
                    self.add(&format!("SPHERICAL_SURFACE('',#{a},{})", real(g.real(s, "radius")? * self.mm()))),
                    false,
                ))
            }
            "TORUS" => {
                let (ma, mi) = (g.real(s, "major_radius")?, g.real(s, "minor_radius")?);
                if !(ma > 0.0 && mi > 0.0) {
                    self.warnings
                        .push(format!("torus {s} has a non-positive radius ({ma}, {mi}); its faces are left out"));
                    return None;
                }
                // A lemon (minor > major) is written as it is: the face's edges select the part
                // and its natural normal is XT's (see ADR 0035).
                let a = self.place(g.vec3(s, "centre")?, g.vec3(s, "axis")?, g.vec3(s, "x_axis")?);
                Some((
                    self.add(&format!("TOROIDAL_SURFACE('',#{a},{},{})", real(ma * self.mm()), real(mi * self.mm()))),
                    false,
                ))
            }
            "B_SURFACE" => {
                let n = geom::nurbs_surface(g, g.ptr(s, "nurbs"))?;
                // The Reference does not say how the poles are ordered: keep the order in which
                // the faces' own vertices lie on the surface (u-major when nothing tells).
                let alt = crate::eval::transposed_poles(&n);
                let hint = self.hints.get(&s).cloned().unwrap_or_default();
                let worst = |c: &Nurbs| {
                    hint.iter().take(16).filter_map(|p| crate::eval::nurbs_distance(c, *p)).fold(0.0f64, f64::max)
                };
                let n = if !hint.is_empty() && worst(&alt) < 0.5 * worst(&n) { alt } else { n };
                Some((self.bspline_surface(&n), false))
            }
            "SWEPT_SURF" => {
                let c = self.curve(g.ptr(s, "section"))?;
                let d = self.dir(g.vec3(s, "sweep")?);
                let v = self.add(&format!("VECTOR('',#{d},1.)"));
                Some((self.add(&format!("SURFACE_OF_LINEAR_EXTRUSION('',#{c},#{v})")), false))
            }
            "SPUN_SURF" => {
                // R(u, v): the profile point C(u) turned by v about the axis, as in STEP.
                let c = self.curve(g.ptr(s, "profile"))?;
                let (p, d) = (self.point(g.vec3(s, "base")?), self.dir(g.vec3(s, "axis")?));
                let ax = self.add(&format!("AXIS1_PLACEMENT('',#{p},#{d})"));
                Some((self.add(&format!("SURFACE_OF_REVOLUTION('',#{c},#{ax})")), false))
            }
            "OFFSET_SURF" => {
                let base = g.ptr(s, "surface");
                let (b, flipped) = self.surface(base)?;
                // Along the basis's natural normal, negated when the basis sense is '-'.
                let mut d = g.real(s, "offset")? * self.mm();
                if g.ch(base, "sense") == Some(b'-') {
                    d = -d;
                }
                if flipped {
                    d = -d;
                }
                Some((self.add(&format!("OFFSET_SURFACE('',#{b},{},.F.)", real(d))), flipped))
            }
            "BLENDED_EDGE" => {
                // No STEP counterpart: the rolling ball's arcs along the spine, fitted.
                let hint = self.hints.get(&s).cloned().unwrap_or_default();
                let grid = crate::eval::blend_grid(g, s, &hint)?;
                Some((self.bspline_surface(&crate::eval::surface_through(&grid)?), false))
            }
            _ => None,
        }
    }

    fn vertex(&mut self, v: u32) -> Option<usize> {
        if let Some(id) = self.vertices.get(&v) {
            return Some(*id);
        }
        let p = self.g.vec3(self.g.ptr(v, "point"), "pvec")?;
        let pt = self.point(p);
        let id = self.add(&format!("VERTEX_POINT('',#{pt})"));
        self.vertices.insert(v, id);
        Some(id)
    }

    /// A point on a closed curve, for the single vertex of a ring edge.
    fn ring_point(&self, c: u32) -> Option<V3> {
        let g = self.g;
        match g.type_name(c)? {
            "CIRCLE" => {
                let x = geom::unit(g.vec3(c, "x_axis")?)?;
                Some(geom::add(g.vec3(c, "centre")?, geom::scale(x, g.real(c, "radius")?)))
            }
            "ELLIPSE" => {
                let x = geom::unit(g.vec3(c, "x_axis")?)?;
                Some(geom::add(g.vec3(c, "centre")?, geom::scale(x, g.real(c, "major_radius")?)))
            }
            "B_CURVE" => {
                let n = geom::nurbs_curve(g, g.ptr(c, "nurbs"))?;
                let mut flat = Vec::new();
                for (k, m) in n.knots[0].iter().zip(&n.mults[0]) {
                    for _ in 0..*m {
                        flat.push(*k);
                    }
                }
                geom::eval_curve(&n, *flat.get(n.degree[0])?)
            }
            "TRIMMED_CURVE" => g.vec3(c, "point_1"),
            "INTERSECTION" => g
                .list(g.ptr(c, "chart"), "hvec")
                .iter()
                .find_map(|v| if let crate::reader::Val::Vec3(p) = v { Some(*p) } else { None }),
            "SP_CURVE" => crate::eval::curve_samples(g, c, 2, 0)?.first().copied(),
            _ => None,
        }
    }

    fn edge(&mut self, e: u32, info: Edge) -> Option<usize> {
        if let Some(r) = self.edges.get(&e) {
            return *r;
        }
        self.edges.insert(e, None);
        let r = (|| {
            let c = self.curve(info.curve)?;
            let (a, b) = match info.ends {
                Some((s, t)) => (self.vertex(s)?, self.vertex(t)?),
                None => {
                    let p = self.ring_point(info.curve)?;
                    let pt = self.point(p);
                    let v = self.add(&format!("VERTEX_POINT('',#{pt})"));
                    (v, v)
                }
            };
            Some(self.add(&format!("EDGE_CURVE('',#{a},#{b},#{c},{})", flag(info.same_sense))))
        })();
        if r.is_none() {
            self.warnings.push(format!("edge {e} cannot be converted"));
        }
        self.edges.insert(e, r);
        r
    }

    /// An advanced face, or `None` when its geometry cannot be written. `flip` reverses it.
    fn face(&mut self, m: &Model, f: &Face, flip: bool) -> Option<usize> {
        let (surf, flipped) = self.surface(f.surface)?;
        let mut bounds = Vec::new();
        for l in &f.loops {
            let lp = match l {
                Loop::Edges(es) => {
                    let mut oes = Vec::new();
                    for (e, along) in es {
                        let info = m.edges.get(e).copied()?;
                        let ec = self.edge(*e, info)?;
                        oes.push(self.add(&format!("ORIENTED_EDGE('',*,*,#{ec},{})", flag(*along))));
                    }
                    self.add(&format!("EDGE_LOOP('',{})", list(&oes)))
                }
                Loop::Vertex(v) => {
                    let vp = self.vertex(*v)?;
                    self.add(&format!("VERTEX_LOOP('',#{vp})"))
                }
            };
            bounds.push(self.add(&format!("FACE_BOUND('',#{lp},{})", flag(!flip))));
        }
        let same = f.same_sense != flipped;
        let id = self.add(&format!("ADVANCED_FACE('',{},#{surf},{})", list(&bounds), flag(same != flip)));
        self.faces.push((id, f.node));
        Some(id)
    }

    /// The faces' `SDL/TYSA_COLOUR` attributes as AP214 styled items (one style per colour),
    /// so a STEP reader sees the face colours (ADR 0045 amendment).
    fn face_colours(&mut self, ctx: usize) {
        let colours = crate::model::colours(self.g);
        let mut styles: BTreeMap<[u64; 3], usize> = BTreeMap::new();
        let mut items = Vec::new();
        for (face, node) in std::mem::take(&mut self.faces) {
            let Some(rgb) = colours.get(&node).copied() else { continue };
            let key = rgb.map(f64::to_bits);
            let style = match styles.get(&key) {
                Some(&st) => st,
                None => {
                    let [r, g, b] = rgb.map(real);
                    let c = self.add(&format!("COLOUR_RGB('',{r},{g},{b})"));
                    let fc = self.add(&format!("FILL_AREA_STYLE_COLOUR('',#{c})"));
                    let fa = self.add(&format!("FILL_AREA_STYLE('',(#{fc}))"));
                    let sf = self.add(&format!("SURFACE_STYLE_FILL_AREA(#{fa})"));
                    let side = self.add(&format!("SURFACE_SIDE_STYLE('',(#{sf}))"));
                    let usage = self.add(&format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
                    let st = self.add(&format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"));
                    styles.insert(key, st);
                    st
                }
            };
            items.push(self.add(&format!("STYLED_ITEM('color',(#{style}),#{face})")));
        }
        if !items.is_empty() {
            self.add(&format!("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',{},#{ctx})", list(&items)));
        }
    }

    fn shell(&mut self, m: &Model, s: &Shell, closed: bool, flip: bool) -> Option<usize> {
        let mut faces = Vec::new();
        for f in &s.faces {
            match self.face(m, f, flip) {
                Some(id) => faces.push(id),
                None => self.warnings.push(format!("face {} was left out", f.node)),
            }
        }
        if faces.is_empty() {
            return None;
        }
        let kind = if closed { "CLOSED_SHELL" } else { "OPEN_SHELL" };
        Some(self.add(&format!("{kind}('',{})", list(&faces))))
    }
}

/// The diagonal of a shell's vertex box (to find a solid's outer shell).
fn extent(g: &Graph, m: &Model, s: &Shell) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for f in &s.faces {
        for l in &f.loops {
            if let Loop::Edges(es) = l {
                for (e, _) in es {
                    if let Some((a, b)) = m.edges.get(e).and_then(|i| i.ends) {
                        for v in [a, b] {
                            if let Some(p) = g.vec3(g.ptr(v, "point"), "pvec") {
                                for ((l, h), c) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                                    *l = l.min(c);
                                    *h = h.max(c);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if lo[0].is_finite() { geom::norm(geom::sub(hi, lo)) } else { 0.0 }
}

/// STEP text for the model, one product per body named `{name} {n}`, and the warnings.
/// The STEP entity text and flip of surface `s`, for tests.
#[cfg(test)]
pub(crate) fn surface_text(g: &Graph, s: u32) -> (Option<(String, bool)>, Vec<String>) {
    let mut w = Writer {
        g,
        out: String::new(),
        next: 1,
        curves: BTreeMap::new(),
        surfaces: BTreeMap::new(),
        vertices: BTreeMap::new(),
        edges: BTreeMap::new(),
        warnings: Vec::new(),
        hints: BTreeMap::new(),
        xf: None,
        scale: 1.0,
        faces: Vec::new(),
    };
    let r = w.surface(s).map(|(id, flipped)| {
        let line = w.out.lines().find(|l| l.starts_with(&format!("#{id}="))).unwrap_or_default().to_string();
        (line, flipped)
    });
    (r, w.warnings)
}

pub(crate) fn write(g: &Graph, m: &Model, name: &str) -> (String, Vec<String>) {
    let mut w = Writer {
        g,
        out: String::new(),
        next: 1,
        curves: BTreeMap::new(),
        surfaces: BTreeMap::new(),
        vertices: BTreeMap::new(),
        edges: BTreeMap::new(),
        warnings: m.warnings.clone(),
        hints: BTreeMap::new(),
        xf: None,
        scale: 1.0,
        faces: Vec::new(),
    };
    for f in m.bodies.iter().flat_map(|b| &b.shells).flat_map(|s| &s.faces) {
        let pts = w.hints.entry(f.surface).or_default();
        for l in &f.loops {
            if let Loop::Edges(es) = l {
                for (e, _) in es {
                    if let Some((a, b)) = m.edges.get(e).and_then(|i| i.ends) {
                        for v in [a, b] {
                            if let Some(p) = g.vec3(g.ptr(v, "point"), "pvec") {
                                pts.push(p);
                            }
                        }
                    }
                }
            }
        }
    }
    let app = w.add("APPLICATION_CONTEXT('automotive design')");
    w.add(&format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"));
    let pc = w.add(&format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let pdc = w.add(&format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let len = w.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))");
    let ang = w.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))");
    let sol = w.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())");
    let unc =
        w.add(&format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#{len},'distance_accuracy_value','')"));
    let ctx = w.add(&format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len},#{ang},#{sol})) REPRESENTATION_CONTEXT('',''))"
    ));
    let many = m.bodies.len() > 1;
    let names = crate::model::names(g);
    let mut previous: Option<Option<crate::model::Xf>> = None;
    for (i, b) in m.bodies.iter().enumerate() {
        // Geometry is written per placement: a body placed twice is written twice.
        w.xf = b.placement;
        w.scale = b.placement.map_or(1.0, |(m, _)| {
            let [x, y, z] = m;
            let det = geom::dot(x, geom::cross(y, z));
            libm::cbrt(det.abs()).max(1e-12)
        });
        // The caches hold placed geometry: reuse them only under the same placement.
        if previous.is_some_and(|p| p != b.placement) {
            w.curves.clear();
            w.surfaces.clear();
            w.vertices.clear();
            w.edges.clear();
        }
        previous = Some(b.placement);
        let items: Vec<usize> = match b.kind {
            BodyKind::Solid => {
                // One solid per lump (region): within a lump, every shell but the one with
                // the largest extent bounds a cavity. V4 files (no regions) are one lump.
                let mut lumps: std::collections::BTreeMap<u32, Vec<&Shell>> = std::collections::BTreeMap::new();
                for s in &b.shells {
                    lumps.entry(s.lump).or_default().push(s);
                }
                let mut solids = Vec::new();
                for mut order in lumps.into_values() {
                    order.sort_by(|x, y| extent(g, m, y).total_cmp(&extent(g, m, x)));
                    let mut it = order.into_iter();
                    let Some(outer) = it.next().and_then(|s| w.shell(m, s, true, false)) else { continue };
                    let mut voids: Vec<usize> = Vec::new();
                    for s in it {
                        if let Some(cs) = w.shell(m, s, true, true) {
                            voids.push(w.add(&format!("ORIENTED_CLOSED_SHELL('',*,#{cs},.F.)")));
                        }
                    }
                    solids.push(if voids.is_empty() {
                        w.add(&format!("MANIFOLD_SOLID_BREP('',#{outer})"))
                    } else {
                        w.add(&format!("BREP_WITH_VOIDS('',#{outer},{})", list(&voids)))
                    });
                }
                if solids.is_empty() {
                    continue;
                }
                solids
            }
            _ => {
                let shells: Vec<usize> = b.shells.iter().filter_map(|s| w.shell(m, s, false, false)).collect();
                if shells.is_empty() {
                    continue;
                }
                vec![w.add(&format!("SHELL_BASED_SURFACE_MODEL('',{})", list(&shells)))]
            }
        };
        let rep_kind = if b.kind == BodyKind::Solid {
            "ADVANCED_BREP_SHAPE_REPRESENTATION"
        } else {
            "MANIFOLD_SURFACE_SHAPE_REPRESENTATION"
        };
        let label = match names.get(&b.node) {
            Some(n) => n.clone(),
            None if many => format!("{name} {}", i + 1),
            None => name.to_string(),
        };
        let label = string(&label);
        let prod = w.add(&format!("PRODUCT({label},{label},'',(#{pc}))"));
        let pdf = w.add(&format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
        let pd = w.add(&format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdc})"));
        let pds = w.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
        let rep = w.add(&format!("{rep_kind}('',{},#{ctx})", list(&items)));
        w.add(&format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));
    }
    w.face_colours(ctx);
    let mut text =
        String::from("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('Parasolid XT converted by Gusset'),'2;1');\n");
    let _ = writeln!(text, "FILE_NAME({},'',(''),(''),'gusset-xt','','');", string(name));
    text.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n");
    text.push_str(&w.out);
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    (text, w.warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reals_always_have_a_point() {
        assert_eq!(real(1.0), "1.0");
        assert_eq!(real(3e21), "3.E21");
        assert_eq!(real(-0.5), "-0.5");
        assert_eq!(real(1e-8), "1.E-8");
        assert_eq!(real(2.5e20), "2.5E20");
        assert_eq!(real(f64::NAN), "0.");
    }
}
