//! Thread feature (ADR 0034 amendment): a thread on a cylindrical face. A modeled thread moves
//! the whole face to the thread's major diameter (a shaft) or minor diameter (a hole), then cuts
//! the groove of the basic profile one turn at a time along exact helices (one helical sweep per
//! turn: OCCT cut a groove of many turns through an end face not at all, one turn at a time it
//! cuts exactly). The groove ends square on the planes across the axis where the thread starts
//! and ends inside the face, and runs out through a face end it reaches unless a face there
//! looks back at the thread (a shoulder, a hole's floor). A cosmetic thread changes nothing.

use super::*;
use gusset_doc::FaceThreadDef;
use gusset_kernel::{
    Loop2, Mesh, Profile, ProfileEdge, Region2, Seg2, SweepOrientation, SweepPath, SweepSpec, TessellationParams, V3,
};
use gusset_naming::entity_tag;
use std::f64::consts::{PI, TAU};

/// The minor diameter of ISO 68-1's basic profile is this many pitches below the major one
/// (5/4 of the fundamental triangle's height sqrt(3)/2 P, on both sides).
pub const ISO_DEPTH_PITCHES: f64 = 1.082_531_754_730_548;
/// Most turns one thread may cut (each is a boolean; tags carry the turn in 12 bits).
pub const MAX_THREAD_TURNS: usize = 4000;

/// The cylindrical face a thread is on, measured on the body (mm).
#[derive(Debug, Clone, Copy)]
struct Cyl {
    /// The axis, from the face's start (the end that comes first along `dir`).
    start: V3,
    dir: V3,
    /// A unit direction across the axis: the thread's angle 0.
    x: V3,
    radius: f64,
    len: f64,
    /// A hole's wall (its material outside).
    internal: bool,
    /// Whether a face at the start or end looks back at the face (a shoulder, a floor).
    blocked: [bool; 2],
}

impl Evaluator {
    pub(crate) fn run_thread(
        &self,
        s: &mut State,
        name: &str,
        d: &FaceThreadDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let bad = |msg: String, hint: &str| EvalError::new(name, ErrorCode::Expression, msg).with_hint(hint);
        let fmt = |v: f64| Quantity::mm(v).display(units);
        let (bi, fi) = find_cylinder(name, &d.face, &s.bodies)?;
        let body = s.bodies.get(bi).cloned().ok_or_else(|| EvalError::new(name, ErrorCode::NoTargetBody, "no body"))?;
        let along = d.along.as_ref().map(|a| ops::direction(name, a, params, units)).transpose()?;
        let mesh = k
            .tessellate(&body.shape, &TessellationParams { linear_deflection: 0.01, angular_deflection: 0.3 })
            .map_err(|ke| kernel_err(name, "face check", ke))?;
        let cyl = measure(name, &body, fi, &mesh, along)?;

        let len = |what: &str, e: &gusset_units::expr::Expr| -> Result<f64, EvalError> {
            let v = eval_expr(name, e, Dim::LENGTH, params, units)?;
            if !v.is_finite() || v <= 0.0 {
                return Err(bad(format!("the {what} must be positive, not {}", fmt(v)), "enter a length such as 1 mm"));
            }
            Ok(v)
        };
        let p = len("pitch", &d.pitch)?;
        let d0 = 2.0 * cyl.radius;
        let (major, minor) = match (&d.major, &d.minor) {
            (Some(a), Some(b)) => (len("major diameter", a)?, len("minor diameter", b)?),
            (Some(a), None) => {
                let a = len("major diameter", a)?;
                (a, a - ISO_DEPTH_PITCHES * p)
            }
            (None, Some(b)) => {
                let b = len("minor diameter", b)?;
                (b + ISO_DEPTH_PITCHES * p, b)
            }
            (None, None) if cyl.internal => (d0 + ISO_DEPTH_PITCHES * p, d0),
            (None, None) => (d0, d0 - ISO_DEPTH_PITCHES * p),
        };
        if !(minor > 0.0 && minor < major) {
            return Err(bad(
                format!("the minor diameter {} is not between 0 and the major diameter {}", fmt(minor), fmt(major)),
                "give a minor diameter below the major one, or a smaller pitch",
            ));
        }
        let angle = match &d.angle {
            Some(e) => eval_expr(name, e, Dim::ANGLE, params, units)?,
            None => PI / 3.0,
        };
        if !(angle.is_finite() && angle > 1f64.to_radians() && angle < 179f64.to_radians()) {
            return Err(bad(format!("the flank angle is {:.1} deg", angle.to_degrees()), "use 60 deg (metric, UN)"));
        }
        let offset = match &d.offset {
            Some(e) => eval_expr(name, e, Dim::LENGTH, params, units)?,
            None => 0.0,
        };
        if !offset.is_finite() || offset < 0.0 {
            return Err(bad(format!("the offset is {}", fmt(offset)), "give an offset of 0 or more"));
        }
        let t0 = offset;
        let t1 = match &d.length {
            Some(e) => (offset + len("length", e)?).min(cyl.len),
            None => cyl.len,
        };
        if t1 - t0 < 1e-6 {
            return Err(EvalError::new(
                name,
                ErrorCode::InvalidReference,
                format!("the thread does not reach the face ({} long)", fmt(cyl.len)),
            )
            .with_hint("make the offset smaller than the face's length"));
        }
        if !d.modeled {
            // A cosmetic thread: checked, recorded in the document, no geometry.
            return Ok(());
        }
        let (rmaj, rmin, r0) = (major / 2.0, minor / 2.0, cyl.radius);
        let tn = libm::tan(angle / 2.0);
        let w = (rmaj - rmin) * tn;
        if 2.0 * w >= 7.0 * p / 8.0 {
            return Err(bad("the flanks meet before the root".into(), "use a larger pitch or a smaller depth"));
        }
        let turns_est = ((t1 - t0) / p).ceil() + 3.0;
        if turns_est > MAX_THREAD_TURNS as f64 {
            return Err(bad(format!("{turns_est} turns"), &format!("a thread cuts at most {MAX_THREAD_TURNS} turns")));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let turns_est = turns_est as usize;
        self.charge(s, name, "the thread's turns", turns_est, body.topology.faces.len() as u64 * turns_est as u64)?;

        // A point on the axis `t` along it from the face's start, a frame there.
        let at = |t: f64| -> V3 {
            [cyl.start[0] + t * cyl.dir[0], cyl.start[1] + t * cyl.dir[1], cyl.start[2] + t * cyl.dir[2]]
        };
        let frame = |t: f64| Frame { origin: at(t), x: cyl.x, y: cyl.dir };
        let poly = |pts: &[[f64; 2]], tag: &dyn Fn(usize) -> u64| -> Vec<ProfileEdge> {
            (0..pts.len())
                .filter_map(|j| {
                    let (a, b) = (*pts.get(j)?, *pts.get((j + 1) % pts.len())?);
                    Some(ProfileEdge { seg: Seg2::Line { a, b }, tag: tag(j) })
                })
                .collect()
        };
        let revolve = |at_t: f64, pts: &[[f64; 2]], tag: &dyn Fn(usize) -> u64| -> Result<OpResult, EvalError> {
            let profile = Profile {
                frame: frame(at_t),
                regions: vec![Region2 { outer: Loop2 { edges: poly(pts, tag) }, holes: Vec::new() }],
            };
            k.revolve(&profile, &RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: TAU })
                .map_err(|ke| kernel_err(name, "the thread's tools", ke))
        };
        let named = |res: &OpResult, ctx: &OpContext<'_>| -> Result<(Topology, BodyNames), EvalError> {
            let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
            let names = name_body(ctx, &res.face_origins, &topo)
                .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
            Ok((topo, names))
        };
        let sweep_ctx = OpContext::Sweep { feature: name, sketch: name, regions: &[] };
        let tag = |id: String| entity_tag(&id).unwrap_or(u64::MAX);
        let mut cur: (Shape, BodyNames) = (body.shape.clone(), body.names.clone());

        // 1. The face moved to the thread's diameter: a shaft to the major diameter, a hole to
        // the minor one, cut down or built up (3692's 6 mm shaft carries a 6.279 mm thread) by
        // a ring over the face's length. The new wall keeps the face's name.
        let moved_to = if cyl.internal { rmin } else { rmaj };
        if (r0 - moved_to).abs() > 1e-6 {
            // Material on the face's outside (a hole) or inside (a shaft); the ring reaches past
            // the old wall into the air, or stops on it where it adds material.
            let grows = cyl.internal == (moved_to < r0);
            let air = p.max(0.1 * r0);
            let (inner, outer) = match (cyl.internal, grows) {
                (true, true) => (moved_to, r0),
                (true, false) => ((r0 - air).max(0.5 * r0), moved_to),
                (false, true) => (r0, moved_to),
                (false, false) => (moved_to, r0 + air),
            };
            let new_wall = if grows == cyl.internal { "r4" } else { "r2" };
            let pts = [[inner, 0.0], [outer, 0.0], [outer, cyl.len], [inner, cyl.len]];
            let res = revolve(0.0, &pts, &|j| tag(format!("r{}", j + 1)))?;
            let (_, mut names) = named(&res, &sweep_ctx)?;
            let wall = FaceRef::new(name, gusset_doc::FaceKey::Side { sketch: name.into(), entity: new_wall.into() });
            for f in &mut names.faces {
                if *f == wall {
                    *f = d.face.clone();
                }
            }
            let op = if grows { BoolOp::Union } else { BoolOp::Cut };
            let what = "moving the face to the thread's diameter";
            let moved = k.boolean(op, &cur.0, &res.shape).map_err(|ke| kernel_err(name, what, ke))?;
            let (topo, n) = named(&moved, &OpContext::Modify { feature: name, inputs: &[&cur.1, &names] })?;
            if topo.faces.is_empty() {
                return Err(EvalError::new(name, ErrorCode::EmptyResult, format!("{what} removed the whole body")));
            }
            cur = (moved.shape, n);
        }

        // 2. The groove, as (radius, height) from the thread's start, run past the surface it
        // cuts so that no edge of it lies there.
        let ext = 0.03 * p / tn;
        let prof: [[f64; 2]; 4] = if cyl.internal {
            [
                [rmin - ext, p / 2.0 - ext * tn],
                [rmaj, p / 2.0 + w],
                [rmaj, 11.0 * p / 8.0 - w],
                [rmin - ext, 11.0 * p / 8.0 + ext * tn],
            ]
        } else {
            [[rmaj + ext, -ext * tn], [rmin, w], [rmin, 7.0 * p / 8.0 - w], [rmaj + ext, 7.0 * p / 8.0 + ext * tn]]
        };
        if prof.iter().any(|q| q[0] <= 0.0) {
            return Err(bad("the groove reaches the axis".into(), "check the diameters and the pitch"));
        }
        let outer = rmaj + ext + p;
        // The groove's section in the material and its centroid radius, for the cut check.
        let section = (rmaj - rmin) * (7.0 * p / 8.0 - w);
        let rc = (rmaj + rmin) / 2.0;
        let low = prof.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min);
        let (lo, hi) = (0.0, t1 - t0);
        let (face_lo, face_hi) = (-t0, cyl.len - t0);
        let clip_lo = t0 > 1e-6 || cyl.blocked[0];
        let clip_hi = t1 < cyl.len - 1e-6 || cyl.blocked[1];
        let pitch = if d.left { -p } else { p };
        let high = prof.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max);
        // Where the groove may end: at the thread's ends, or past the face's (a run-out).
        let (lo_end, hi_end) = (if clip_lo { lo } else { face_lo }, if clip_hi { hi } else { face_hi });
        // One turn a sweep (the helix climbs the axis whichever its hand), from two pitches
        // below the thread's start on, so that every angle is covered; turns that reach no
        // material between the ends are left out.
        let mut zs: Vec<f64> = Vec::new();
        let mut z = ((lo - 2.0 * p - low) / p).floor() * p;
        while low + z <= hi_end + 1e-9 {
            if high + z + p > lo_end + 1e-9 && low + z < hi_end - 1e-9 {
                zs.push(z);
            }
            if zs.len() > MAX_THREAD_TURNS {
                return Err(bad("too many turns".into(), &format!("a thread cuts at most {MAX_THREAD_TURNS} turns")));
            }
            z += p;
        }
        let mut tools: Vec<ops::Tool> = Vec::with_capacity(zs.len());
        for (i, z) in zs.into_iter().enumerate() {
            if self.is_cancelled() {
                return Err(budget::cancelled(name));
            }
            let turn = i + 1;
            let (bottom, top) = (low + z, high + z + p);
            let pts: Vec<[f64; 2]> = prof.iter().map(|q| [q[0], q[1] + z]).collect();
            let profile = Profile {
                frame: frame(t0),
                regions: vec![Region2 {
                    outer: Loop2 { edges: poly(&pts, &|j| tag(format!("g{}_{turn}", j + 1))) },
                    holes: Vec::new(),
                }],
            };
            // The helix starts level with the section (as a sweep's does): started elsewhere,
            // the section was swept up the axis where the helix ran down it.
            let helix = SweepPath::Helix {
                origin: at(t0 + z + (low + high) / 2.0),
                axis: cyl.dir,
                x: cyl.x,
                radius: rmaj,
                pitch,
                height: p,
            };
            let res = k
                .sweep(&profile, &helix, &SweepSpec { orientation: SweepOrientation::Fixed, twist: 0.0 })
                .map_err(|ke| kernel_err(name, &format!("the groove's turn {turn}"), ke))?;
            let (_, names) = named(&res, &sweep_ctx)?;
            let (cl, ch) = (clip_lo && bottom < lo, clip_hi && top > hi);
            let (tool, tool_names) = if cl || ch {
                let from = if cl { lo } else { bottom - p };
                let to = if ch { hi } else { top + p };
                let disc = revolve(t0, &[[0.0, from], [outer, from], [outer, to], [0.0, to]], &|j| {
                    tag(format!("e{}_{turn}", j + 1))
                })?;
                let (_, disc_names) = named(&disc, &sweep_ctx)?;
                let cut = k
                    .boolean(BoolOp::Intersect, &res.shape, &disc.shape)
                    .map_err(|ke| kernel_err(name, &format!("ending the groove's turn {turn}"), ke))?;
                let (_, n) = named(&cut, &OpContext::Modify { feature: name, inputs: &[&names, &disc_names] })?;
                (cut.shape, n)
            } else {
                (res.shape, names)
            };
            tools.push(ops::Tool { shape: tool, names: tool_names });
        }
        let v0 = volume(k, name, &cur.0)?;
        let on_error = |i: Option<usize>, ke: KernelError| {
            let what =
                i.map_or_else(|| "cutting the groove".to_string(), |i| format!("cutting the groove's turn {}", i + 1));
            kernel_err(name, &what, ke)
        };
        let (shape, names, t) = self.boolean_tools(
            name,
            BoolOp::Cut,
            (&cur.0, &cur.1),
            &tools,
            "the groove removed the whole body",
            &on_error,
        )?;
        // The groove must take out about its share of the face's material: a long helical sweep
        // has left a body unchanged, without an error.
        let reach = hi.min(face_hi) - lo.max(face_lo);
        let taken = v0 - volume(k, name, &shape)?;
        let expect = section * TAU * rc / p * reach;
        if taken < 0.5 * expect {
            return Err(EvalError::new(
                name,
                ErrorCode::KernelFailure,
                format!("the kernel did not cut the thread's groove ({taken:.4} of {expect:.4} mm3 taken)"),
            )
            .with_hint("try a slightly different offset or length"));
        }
        let (cur, topo) = ((shape, names), t);
        if let Some(slot) = s.bodies.get_mut(bi) {
            *slot = Arc::new(Body {
                name: body.name.clone(),
                feature: body.feature.clone(),
                shape: cur.0,
                names: cur.1,
                topology: topo,
                holes: body.holes.clone(),
                sheet: body.sheet.clone(),
            });
        }
        Ok(())
    }
}

fn volume(k: &dyn Kernel, name: &str, shape: &Shape) -> Result<f64, EvalError> {
    k.mass_properties(shape).map(|m| m.volume).map_err(|ke| kernel_err(name, "volume", ke))
}

/// The body and face index of a cylindrical face reference.
fn find_cylinder(feature: &str, r: &FaceRef, bodies: &[Arc<Body>]) -> Result<(usize, u32), EvalError> {
    for (bi, b) in bodies.iter().enumerate().rev() {
        if let Some(fi) = b.names.face_index(r) {
            let kind = b.topology.faces.get(fi as usize).map(|f| f.kind);
            if kind != Some(SurfaceKind::Cylinder) {
                return Err(EvalError::new(
                    feature,
                    ErrorCode::InvalidReference,
                    format!("face {r} is not cylindrical"),
                )
                .with_hint("a thread goes on a shaft's side or a hole's wall"));
            }
            return Ok((bi, fi));
        }
    }
    let mut e = EvalError::new(feature, ErrorCode::MissingReference, format!("face {r} no longer exists"))
        .with_hint("re-select the cylindrical face the thread is on");
    e.reference = Some(r.to_string());
    e.suggestion = bodies.last().and_then(|b| ops::closest_face(&b.names, r)).map(|f| f.to_string());
    Err(e)
}

/// The face's axis, radius, extent and side, from the body's topology (exact) and mesh (the
/// side, from the normals, and the extent of faces whose edges are not circles).
fn measure(name: &str, body: &Body, fi: u32, mesh: &Mesh, along: Option<V3>) -> Result<Cyl, EvalError> {
    let bad = |m: &str| EvalError::new(name, ErrorCode::InvalidReference, m.to_string());
    let info = body.topology.faces.get(fi as usize).ok_or_else(|| bad("the face is not on the body"))?;
    let axis = info.axis.ok_or_else(|| bad("the face has no axis"))?;
    let a = normalize(axis.dir).ok_or_else(|| bad("the face has no axis"))?;
    let o = axis.origin;
    let t_of = |q: V3| dot(sub(q, o), a);
    let off = |q: V3| {
        let w = sub(q, o);
        let r = sub(w, scale(a, dot(w, a)));
        dot(r, r).sqrt()
    };
    let edges: Vec<&gusset_kernel::EdgeInfo> = body.topology.edges.iter().filter(|e| e.faces.contains(&fi)).collect();
    let mut pts: Vec<V3> = Vec::new();
    for e in &edges {
        pts.push(e.midpoint);
        pts.extend(e.vertices.iter().filter_map(|v| body.topology.vertices.get(*v as usize)).map(|v| v.point));
    }
    let mut radius = edges
        .iter()
        .filter_map(|e| e.circle)
        .find(|c| off(c.center) < 1e-6 && dot(c.normal, a).abs() > 1.0 - 1e-9)
        .map(|c| c.radius);
    if radius.is_none() {
        radius = pts.first().map(|q| off(*q));
    }
    let radius = radius.filter(|r| *r > 1e-9).ok_or_else(|| bad("the face's radius is not known"))?;
    // The side and the mesh's extent.
    let (mut outward, mut tmin, mut tmax) = (0.0, f64::INFINITY, f64::NEG_INFINITY);
    if let Some(range) = mesh.face_ranges.get(fi as usize) {
        for t in range.start as usize..range.end as usize {
            for &vi in mesh.indices.get(t * 3..t * 3 + 3).unwrap_or(&[]) {
                let (Some(q), Some(n)) = (mesh.positions.get(vi as usize), mesh.normals.get(vi as usize)) else {
                    continue;
                };
                let q = q.map(f64::from);
                let n = n.map(f64::from);
                let w = sub(q, o);
                let r = sub(w, scale(a, dot(w, a)));
                outward += dot(n, r);
                tmin = tmin.min(t_of(q));
                tmax = tmax.max(t_of(q));
            }
        }
    }
    if outward == 0.0 {
        return Err(bad("the face has no mesh"));
    }
    for q in &pts {
        tmin = tmin.min(t_of(*q));
        tmax = tmax.max(t_of(*q));
    }
    // Mesh points are single precision: an end the edges give exactly wins.
    let snap = |t: f64| pts.iter().map(|q| t_of(*q)).find(|u| (u - t).abs() < 1e-4 * (1.0 + t.abs())).unwrap_or(t);
    let (tmin, tmax) = (snap(tmin), snap(tmax));
    if tmax - tmin < 1e-9 {
        return Err(bad("the face has no length"));
    }
    let sign = match along {
        Some(v) if dot(v, a).abs() < 1.0 - 1e-6 => {
            return Err(EvalError::new(name, ErrorCode::InvalidReference, "'along' is not along the face's axis")
                .with_hint("give the axis direction the thread runs in, e.g. Z or [0, 0, -1]"));
        }
        Some(v) => dot(v, a).signum(),
        None => 1.0,
    };
    let dir = scale(a, sign);
    let start = if sign > 0.0 { scale(a, tmin) } else { scale(a, tmax) };
    let start = [o[0] + start[0], o[1] + start[1], o[2] + start[2]];
    // Angle 0: world X across the axis, else world Y.
    let x = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        .into_iter()
        .find_map(|e: V3| (dot(e, a).abs() < 0.9).then(|| normalize(sub(e, scale(a, dot(e, a))))).flatten())
        .ok_or_else(|| bad("the face's axis has no direction across it"))?;
    // A face at an end that looks back at this one (its normal against the way out).
    let tol = 1e-6 * (1.0 + radius + (tmax - tmin));
    let mut blocked = [false; 2];
    for e in &edges {
        let ts: Vec<f64> = std::iter::once(e.midpoint)
            .chain(e.vertices.iter().filter_map(|v| body.topology.vertices.get(*v as usize)).map(|v| v.point))
            .map(t_of)
            .collect();
        for (end, (t_end, out)) in [(tmin, -1.0), (tmax, 1.0)].into_iter().enumerate() {
            if !ts.iter().all(|t| (t - t_end).abs() < tol) {
                continue;
            }
            for f in e.faces.iter().filter(|f| **f != fi) {
                if let Some(nf) = body.topology.faces.get(*f as usize)
                    && dot(nf.normal, scale(a, out)) < -0.3
                {
                    // The end's index along `dir`.
                    let along_end = if sign > 0.0 { end } else { 1 - end };
                    if let Some(b) = blocked.get_mut(along_end) {
                        *b = true;
                    }
                }
            }
        }
    }
    Ok(Cyl { start, dir, x, radius, len: tmax - tmin, internal: outward < 0.0, blocked })
}
