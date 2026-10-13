//! Hole feature (ADR 0019): each hole is a revolved profile (mouth, steps, wall, drill point);
//! all of a feature's holes are cut from the body that owns the face in one boolean
//! (review H3).

use super::*;
use crate::ops::Tool;
use gusset_doc::{FaceKey, HoleDef, HoleDepth, HolePart, HoleSites, HoleType, MAX_HOLE_SITES};
use gusset_kernel::{Loop2, Mesh, Profile, ProfileEdge, Region2, Seg2, TessellationParams, V3, norm};
use gusset_naming::hole_tag;
use std::f64::consts::{PI, TAU};

/// The full drill-point angle of a blind hole when none is given (a standard twist drill).
pub const DEFAULT_POINT_ANGLE: f64 = 118.0 * PI / 180.0;
/// Smallest and largest drill-point angle accepted.
const POINT_ANGLE_RANGE: (f64, f64) = (30.0 * PI / 180.0, PI);
/// How far past the body a through hole's tool reaches (mm).
const THROUGH_OVERSHOOT: f64 = 1.0;

/// The cosmetic thread of a tapped hole.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadInfo {
    pub designation: String,
    pub pitch: f64,
    /// Threaded length from the face (mm); `None` = through the whole hole.
    pub depth: Option<f64>,
}

/// One hole made by a `hole` feature, as recorded on the body (`query_body` lists it).
/// Lengths in mm, angles in radians.
#[derive(Debug, Clone, PartialEq)]
pub struct HoleInfo {
    pub feature: String,
    /// The hole's id (`p5`, `e1`).
    pub at: String,
    pub kind: HoleType,
    pub diameter: f64,
    /// Depth of the full-diameter part from the face; `None` = through.
    pub depth: Option<f64>,
    /// Full angle of the drill point (blind holes).
    pub point_angle: Option<f64>,
    /// Counterbore (diameter, depth).
    pub counterbore: Option<(f64, f64)>,
    /// Countersink (diameter, full cone angle).
    pub countersink: Option<(f64, f64)>,
    pub thread: Option<ThreadInfo>,
    /// The wall face (`Hole1/wall(p5)`, without a split index): the hole's handle.
    pub wall: FaceRef,
}

/// The evaluated dimensions of one hole (mm, radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dims {
    pub diameter: f64,
    /// `None` = through.
    pub depth: Option<f64>,
    pub point_angle: f64,
    /// Counterbore (diameter, depth).
    pub counterbore: Option<(f64, f64)>,
    /// Countersink (diameter, full cone angle).
    pub countersink: Option<(f64, f64)>,
}

impl Dims {
    /// Radius of the widest part, which sits in the mouth plane.
    pub fn mouth_radius(&self) -> f64 {
        let r = self.diameter / 2.0;
        let c = self.counterbore.map_or(0.0, |(d, _)| d / 2.0);
        let s = self.countersink.map_or(0.0, |(d, _)| d / 2.0);
        r.max(c).max(s)
    }

    /// Depth of the countersink cone below the mouth.
    pub fn countersink_depth(&self) -> Option<f64> {
        self.countersink.map(|(d, a)| (d - self.diameter) / 2.0 / libm::tan(a / 2.0))
    }

    /// How deep the counterbore or countersink goes.
    pub fn recess_depth(&self) -> Option<f64> {
        match (self.counterbore.map(|(_, h)| h), self.countersink_depth()) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }

    /// Height of the drill-point cone (0 for a flat end).
    pub fn point_height(&self) -> f64 {
        if self.point_angle >= PI - 1e-9 { 0.0 } else { self.diameter / 2.0 / libm::tan(self.point_angle / 2.0) }
    }

    /// The closed outline of the hole's tool in (radius, depth) coordinates, counter-clockwise,
    /// ending with the edge along the axis. `through_len` is the tool's length for a through
    /// hole. Every edge is tagged with the [`HolePart`] of the face it makes.
    pub fn outline(&self, through_len: f64) -> Vec<ProfileEdge> {
        let r = self.diameter / 2.0;
        // Each point is the end of the edge that makes the part next to it.
        let mut pts: Vec<([f64; 2], HolePart)> = vec![([0.0, 0.0], HolePart::Top)];
        if let Some((d, depth)) = self.counterbore {
            pts.push(([d / 2.0, 0.0], HolePart::Top));
            pts.push(([d / 2.0, depth], HolePart::CboreWall));
            pts.push(([r, depth], HolePart::CboreFloor));
        } else if let (Some((d, _)), Some(h)) = (self.countersink, self.countersink_depth()) {
            pts.push(([d / 2.0, 0.0], HolePart::Top));
            pts.push(([r, h], HolePart::Csink));
        } else {
            pts.push(([r, 0.0], HolePart::Top));
        }
        let end = self.depth.unwrap_or(through_len);
        pts.push(([r, end], HolePart::Wall));
        let tip = if self.depth.is_some() { end + self.point_height() } else { end };
        pts.push(([0.0, tip], HolePart::Floor));
        let mut edges: Vec<ProfileEdge> = Vec::new();
        for w in pts.windows(2) {
            if let [(a, _), (b, part)] = w {
                edges.push(ProfileEdge { seg: Seg2::Line { a: *a, b: *b }, tag: hole_tag(*part) });
            }
        }
        if let (Some((last, _)), Some((first, _))) = (pts.last(), pts.first()) {
            // Along the axis: no face comes of it, so any tag will do.
            edges.push(ProfileEdge { seg: Seg2::Line { a: *last, b: *first }, tag: hole_tag(HolePart::Top) });
        }
        edges
    }
}

impl Evaluator {
    pub(crate) fn run_hole(
        &self,
        s: &mut State,
        name: &str,
        d: &HoleDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let (dims, thread) = hole_dims(name, d, units, params)?;
        let fmt = |v: f64| Quantity::mm(v).display(units);

        // The face, its body and its frame.
        let (bi, fi) = find_face(name, &d.face, &s.bodies)?;
        let body = s.bodies.get(bi).cloned().ok_or_else(|| EvalError::new(name, ErrorCode::NoTargetBody, "no body"))?;
        let frame = face_frame(name, &d.face, &s.bodies)?;
        let normal = frame.normal();

        // Where the holes go.
        let sites = hole_positions(name, d, &frame, s, units, params)?;
        if sites.is_empty() {
            return Err(EvalError::new(name, ErrorCode::InvalidReference, "there are no points to drill")
                .with_hint("add points to the sketch (circle centres count), or give at: [[u, v]]"));
        }
        if sites.len() > MAX_HOLE_SITES {
            return Err(EvalError::new(
                name,
                ErrorCode::TooComplex,
                format!("{} hole positions (at most {MAX_HOLE_SITES} per feature)", sites.len()),
            )
            .with_hint("split the holes over several hole features, or use fewer points"));
        }
        for (i, (a, pa)) in sites.iter().enumerate() {
            if let Some((b, _)) = sites.iter().skip(i + 1).find(|(_, pb)| dist(*pa, *pb) < 1e-6) {
                return Err(EvalError::new(
                    name,
                    ErrorCode::InvalidReference,
                    format!("holes {a} and {b} are at the same place"),
                )
                .with_hint("remove one of the points"));
            }
        }

        // Every hole's mouth must lie within the face.
        let mouth = dims.mouth_radius();
        let mesh = k
            .tessellate(&body.shape, &TessellationParams { linear_deflection: 0.01, angular_deflection: 0.5 })
            .map_err(|ke| kernel_err(name, "face check", ke))?;
        let outline = FaceOutline::new(&mesh, fi, &frame);
        for (id, pos) in &sites {
            if !outline.holds_disc(frame_uv(&frame, *pos), mouth) {
                let at = frame_uv(&frame, *pos);
                return Err(EvalError::new(
                    name,
                    ErrorCode::InvalidReference,
                    format!(
                        "hole {id} (diameter {}) at ({}, {}) does not fit inside face {}",
                        fmt(2.0 * mouth),
                        fmt(at[0]),
                        fmt(at[1]),
                        d.face
                    ),
                )
                .with_hint("move the point away from the face's edges, or make the hole smaller"));
            }
        }

        // Every tool has one face per outline edge but the one on the axis.
        let tool_faces = (dims.outline(1.0).len().saturating_sub(1) * sites.len()) as u64;
        self.charge(s, name, "the holes", sites.len(), body.topology.faces.len() as u64 + tool_faces)?;

        // Build every tool, then cut them all at once.
        let bbox = k.bbox(&body.shape).map_err(|ke| kernel_err(name, "bounding box", ke))?;
        let mut tools = Vec::with_capacity(sites.len());
        let mut infos = Vec::with_capacity(sites.len());
        for (id, pos) in &sites {
            let through_len = if dims.depth.is_none() {
                let reach = corners(&bbox).iter().map(|c| -dot(sub(*c, *pos), normal)).fold(f64::MIN, f64::max);
                if reach <= 1e-6 {
                    return Err(EvalError::new(
                        name,
                        ErrorCode::InvalidReference,
                        "there is no material below the face",
                    )
                    .with_hint("the face looks away from the body; pick the face the hole starts on"));
                }
                if dims.recess_depth().is_some_and(|h| h >= reach) {
                    return Err(EvalError::new(
                        name,
                        ErrorCode::Expression,
                        format!("the recess at the mouth of hole {id} is as deep as the part"),
                    )
                    .with_hint("make the counterbore or countersink shallower"));
                }
                reach + THROUGH_OVERSHOOT
            } else {
                0.0
            };
            let tool_frame = Frame { origin: *pos, x: frame.x, y: scale(normal, -1.0) };
            let profile = Profile {
                frame: tool_frame,
                regions: vec![Region2 { outer: Loop2 { edges: dims.outline(through_len) }, holes: Vec::new() }],
            };
            let tool = k
                .revolve(&profile, &RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: TAU })
                .map_err(|ke| kernel_err(name, &format!("hole {id}"), ke))?;
            let tool_topo = k.topology(&tool.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
            let tool_names = name_body(&OpContext::Hole { feature: name, at: id }, &tool.face_origins, &tool_topo)
                .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
            tools.push(Tool { shape: tool.shape, names: tool_names });
            infos.push(HoleInfo {
                feature: name.to_string(),
                at: id.clone(),
                kind: d.kind,
                diameter: dims.diameter,
                depth: dims.depth,
                point_angle: dims.depth.map(|_| dims.point_angle),
                counterbore: dims.counterbore,
                countersink: dims.countersink,
                thread: thread.as_ref().map(|t| ThreadInfo {
                    designation: t.designation.clone(),
                    pitch: t.pitch,
                    depth: t.depth.or(dims.depth),
                }),
                wall: FaceRef::new(name, FaceKey::Hole { part: HolePart::Wall, at: id.clone() }),
            });
        }
        let on_error = |i: Option<usize>, ke: KernelError| {
            let what = match i.and_then(|i| sites.get(i)) {
                Some((id, _)) => format!("cutting hole {id}"),
                None => "cutting the holes".to_string(),
            };
            let mut e = kernel_err(name, &what, ke);
            e.hint
                .get_or_insert_with(|| "check that the hole fits the part and does not run into other features".into());
            e
        };
        let (shape, names, topo) = self.boolean_tools(
            name,
            BoolOp::Cut,
            (&body.shape, &body.names),
            &tools,
            "the holes removed the whole body",
            &on_error,
        )?;
        let mut holes = body.holes.clone();
        holes.extend(infos);
        if let Some(slot) = s.bodies.get_mut(bi) {
            *slot = Arc::new(Body {
                name: body.name.clone(),
                feature: body.feature.clone(),
                shape,
                names,
                topology: topo,
                holes,
            });
        }
        Ok(())
    }
}

/// The thread of a tapped hole, evaluated (mm).
struct ThreadVals {
    designation: String,
    pitch: f64,
    depth: Option<f64>,
}

/// Evaluates and checks the dimensions of a hole feature.
fn hole_dims(
    name: &str,
    d: &HoleDef,
    units: LengthUnit,
    params: &ParamValues,
) -> Result<(Dims, Option<ThreadVals>), EvalError> {
    let fmt = |v: f64| Quantity::mm(v).display(units);
    let bad = |msg: String, hint: &str| EvalError::new(name, ErrorCode::Expression, msg).with_hint(hint);
    let len = |what: &str, e: &gusset_units::expr::Expr| -> Result<f64, EvalError> {
        let v = eval_expr(name, e, Dim::LENGTH, params, units)?;
        if !v.is_finite() || v <= 0.0 {
            return Err(bad(
                format!("{what} must be positive, not {}", fmt(v)),
                &format!("enter a {what} such as 6 mm"),
            ));
        }
        Ok(v)
    };
    let diameter = len("diameter", &d.diameter)?;
    let depth = match &d.depth {
        HoleDepth::Blind(e) => Some(len("depth", e)?),
        HoleDepth::Through => None,
    };
    let point_angle = match &d.point_angle {
        Some(e) => {
            let a = eval_expr(name, e, Dim::ANGLE, params, units)?;
            if !(POINT_ANGLE_RANGE.0..=POINT_ANGLE_RANGE.1).contains(&a) {
                return Err(bad(
                    format!("the drill point angle is {:.1} deg", a.to_degrees()),
                    "use an angle from 30 deg to 180 deg (118 deg is the usual drill; 180 deg is flat)",
                ));
            }
            a
        }
        None => DEFAULT_POINT_ANGLE,
    };
    let counterbore = match &d.counterbore {
        Some(c) => {
            let (cd, ch) = (len("counterbore diameter", &c.diameter)?, len("counterbore depth", &c.depth)?);
            if cd <= diameter {
                return Err(bad(
                    format!(
                        "the counterbore diameter {} is not larger than the hole diameter {}",
                        fmt(cd),
                        fmt(diameter)
                    ),
                    "make the counterbore wider than the hole, or use a simple hole",
                ));
            }
            if let Some(l) = depth
                && ch >= l
            {
                return Err(bad(
                    format!("the counterbore depth {} is not less than the hole depth {}", fmt(ch), fmt(l)),
                    "make the hole deeper than the counterbore",
                ));
            }
            Some((cd, ch))
        }
        None => None,
    };
    let countersink = match &d.countersink {
        Some(c) => {
            let cd = len("countersink diameter", &c.diameter)?;
            let a = eval_expr(name, &c.angle, Dim::ANGLE, params, units)?;
            if !(a > 0.0 && a < PI) {
                return Err(bad(
                    format!("the countersink angle is {:.1} deg", a.to_degrees()),
                    "use the full cone angle between 0 and 180 deg, for example 90 deg",
                ));
            }
            if cd <= diameter {
                return Err(bad(
                    format!(
                        "the countersink diameter {} is not larger than the hole diameter {}",
                        fmt(cd),
                        fmt(diameter)
                    ),
                    "make the countersink wider than the hole, or use a simple hole",
                ));
            }
            let sink = Dims { diameter, depth, point_angle, counterbore: None, countersink: Some((cd, a)) };
            let h = sink.countersink_depth().unwrap_or(0.0);
            if let Some(l) = depth
                && h >= l
            {
                return Err(bad(
                    format!("the countersink is {} deep, as deep as the hole ({})", fmt(h), fmt(l)),
                    "make the hole deeper, the countersink narrower, or its angle wider",
                ));
            }
            Some((cd, a))
        }
        None => None,
    };
    let thread = match &d.thread {
        Some(t) => {
            let pitch = len("thread pitch", &t.pitch)?;
            let tdepth = t.depth.as_ref().map(|e| len("thread depth", e)).transpose()?;
            if let (Some(td), Some(l)) = (tdepth, depth)
                && td > l + 1e-9
            {
                return Err(bad(
                    format!("the thread depth {} is greater than the hole depth {}", fmt(td), fmt(l)),
                    "make the hole at least as deep as the thread",
                ));
            }
            Some(ThreadVals { designation: t.designation.clone(), pitch, depth: tdepth })
        }
        None => None,
    };
    Ok((Dims { diameter, depth, point_angle, counterbore, countersink }, thread))
}

/// The body and face index of a face reference.
fn find_face(feature: &str, r: &FaceRef, bodies: &[Arc<Body>]) -> Result<(usize, u32), EvalError> {
    for (bi, b) in bodies.iter().enumerate().rev() {
        if let Some(fi) = b.names.face_index(r) {
            let kind = b.topology.faces.get(fi as usize).map(|f| f.kind);
            if kind != Some(SurfaceKind::Plane) {
                return Err(EvalError::new(feature, ErrorCode::InvalidReference, format!("face {r} is not planar"))
                    .with_hint("holes start on a flat face"));
            }
            return Ok((bi, fi));
        }
    }
    let mut e = EvalError::new(feature, ErrorCode::MissingReference, format!("face {r} no longer exists"))
        .with_hint("re-select the face the holes start on");
    e.reference = Some(r.to_string());
    e.suggestion = bodies.last().and_then(|b| ops::closest_face(&b.names, r)).map(|f| f.to_string());
    Err(e)
}

/// The world position of every hole, with its id (the stored `e<n>` of an explicit point),
/// in id order for sketches and list order for explicit points. All lie in the plane of the
/// face.
fn hole_positions(
    feature: &str,
    d: &HoleDef,
    frame: &Frame,
    s: &State,
    units: LengthUnit,
    params: &ParamValues,
) -> Result<Vec<(String, V3)>, EvalError> {
    let n = frame.normal();
    match &d.sites {
        HoleSites::Points(list) => {
            let mut out = Vec::with_capacity(list.len());
            for h in list {
                let uv = [
                    eval_expr(feature, &h.at[0], Dim::LENGTH, params, units)?,
                    eval_expr(feature, &h.at[1], Dim::LENGTH, params, units)?,
                ];
                out.push((h.name(), frame.to_world(uv)));
            }
            Ok(out)
        }
        HoleSites::Sketch(sketch) => {
            let (solved, sframe) = sketch_input(feature, sketch, s)?;
            if dot(sframe.normal(), n).abs() < 1.0 - 1e-6 {
                return Err(EvalError::new(
                    feature,
                    ErrorCode::InvalidReference,
                    format!("sketch {sketch} is not parallel to the face {}", d.face),
                )
                .with_hint("sketch the hole points on the face, or on a plane parallel to it"));
            }
            Ok(solved
                .hole_sites
                .iter()
                .map(|(id, uv)| {
                    let w = sframe.to_world(*uv);
                    (id.clone(), sub(w, scale(n, dot(sub(w, frame.origin), n))))
                })
                .collect())
        }
    }
}

fn dist(a: V3, b: V3) -> f64 {
    norm(sub(a, b))
}

/// A world point in the (u, v) coordinates of `frame`.
fn frame_uv(frame: &Frame, p: V3) -> [f64; 2] {
    let r = sub(p, frame.origin);
    [dot(r, frame.x), dot(r, frame.y)]
}

fn corners(b: &gusset_kernel::BBox) -> [V3; 8] {
    let (lo, hi) = (b.min, b.max);
    [
        [lo[0], lo[1], lo[2]],
        [hi[0], lo[1], lo[2]],
        [lo[0], hi[1], lo[2]],
        [hi[0], hi[1], lo[2]],
        [lo[0], lo[1], hi[2]],
        [hi[0], lo[1], hi[2]],
        [lo[0], hi[1], hi[2]],
        [hi[0], hi[1], hi[2]],
    ]
}

/// A mesh position as bits (shared corners of a face's triangles compare equal).
type MeshPoint = [u32; 3];
/// A triangle edge (in (u, v)) and how many triangles of the face have it.
type EdgeUse = (usize, [[f64; 2]; 2]);

/// A face's triangles and boundary, in the (u, v) coordinates of its frame, for the check
/// that a hole's mouth fits on the face (review L7).
struct FaceOutline {
    tris: Vec<[[f64; 2]; 3]>,
    /// The triangle edges that only one triangle of the face has: the face's outer and
    /// inner boundary loops (edges of existing holes and slots included).
    boundary: Vec<[[f64; 2]; 2]>,
}

impl FaceOutline {
    fn new(mesh: &Mesh, fi: u32, frame: &Frame) -> Self {
        let mut tris = Vec::new();
        // Each edge by its end positions (as bits, unordered) -> how many triangles have it.
        let mut uses: BTreeMap<(MeshPoint, MeshPoint), EdgeUse> = BTreeMap::new();
        if let Some(range) = mesh.face_ranges.get(fi as usize) {
            for t in range.start as usize..range.end as usize {
                let Some(idx) = mesh.indices.get(t * 3..t * 3 + 3) else { continue };
                let mut corners = [([0_u32; 3], [0.0; 2]); 3];
                let mut ok = true;
                for (slot, &vi) in corners.iter_mut().zip(idx) {
                    match mesh.positions.get(vi as usize) {
                        Some(p) => {
                            let uv = frame_uv(frame, [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
                            *slot = (p.map(f32::to_bits), uv);
                        }
                        None => ok = false,
                    }
                }
                if !ok {
                    continue;
                }
                tris.push(corners.map(|c| c.1));
                let [c0, c1, c2] = corners;
                for (a, b) in [(c0, c1), (c1, c2), (c2, c0)] {
                    let key = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
                    uses.entry(key).or_insert((0, [a.1, b.1])).0 += 1;
                }
            }
        }
        let boundary = uses.into_values().filter(|(n, _)| *n == 1).map(|(_, seg)| seg).collect();
        FaceOutline { tris, boundary }
    }

    /// Whether the disc of radius `radius` around `c` lies on the face: the centre is on
    /// one of its triangles and no boundary edge comes closer than the radius (less a
    /// little, for the chord error of curved edges and single-precision mesh coordinates).
    fn holds_disc(&self, c: [f64; 2], radius: f64) -> bool {
        let margin = (0.01_f64).min(radius * 0.2);
        let r = radius - margin;
        self.tris.iter().any(|t| in_triangle(c, t)) && self.boundary.iter().all(|[a, b]| point_seg_dist(c, *a, *b) >= r)
    }
}

/// Distance from `p` to the segment `a`-`b`.
fn point_seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    libm::hypot(p[0] - a[0] - t * d[0], p[1] - a[1] - t * d[1])
}

/// Point in triangle (edges included, with a tolerance of 1e-6 mm).
fn in_triangle(p: [f64; 2], t: &[[f64; 2]; 3]) -> bool {
    let side = |a: [f64; 2], b: [f64; 2]| {
        let (e, w) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
        let len = libm::hypot(e[0], e[1]).max(1e-12);
        (e[0] * w[1] - e[1] * w[0]) / len
    };
    let s = [side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0])];
    let eps = 1e-6;
    s.iter().all(|v| *v >= -eps) || s.iter().all(|v| *v <= eps)
}
