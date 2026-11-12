//! A bend of a flat stretch of sheet along a sketch line (ADR 0037 amendment, 2026-11-12).
//!
//! The sheet is cut across by two planes through the bend zone's ends, both square to the
//! sheet and parallel to the line: the part that stays, the zone, and the part that moves.
//! The zone, `angle (r + k t)` long (its neutral fibre keeps its length), becomes the
//! revolution of its cross-section about the bend axis (inside radius `r`), the moving part
//! is laid back onto the zone's start and turned with it, and the three are united. Faces
//! of the body keep their names (a face cut in two keeps its name on both sides); the bend's
//! own faces are `<bend>/bend_in(<line>)`, `bend_out(<line>)`, and its sides `start(<line>)`
//! and `end(<line>)` (at the low and high end of the line).
//!
//! The zone must be a straight strip of the sheet: one face of the sheet on the sketch plane,
//! its cross-section one rectangle the sheet's thickness deep, the same all across the zone
//! (no cut-out or slanted edge in it). Anything else is refused, naming what is wrong.

use super::*;
use crate::ops::{Tool, replace_body, target_body};
use gusset_doc::{BendPosition, SheetPart};
use gusset_kernel::{Loop2, Profile, ProfileEdge, Region2, Seg2, Transform, add};
use gusset_naming::sheet_tag;

/// The numbers of one bend (mm, radians).
pub(crate) struct BendSpec<'a> {
    pub(crate) body: &'a str,
    pub(crate) sketch: &'a str,
    pub(crate) line: &'a str,
    pub(crate) angle: f64,
    pub(crate) r: f64,
    pub(crate) k: f64,
    pub(crate) down: bool,
    pub(crate) right: bool,
    pub(crate) position: BendPosition,
}

/// The sheet's frame at the line: `a` across the line towards the side that moves, `l` along
/// the line (so that `a`, `l`, `n` are right-handed), `n` the sketch normal.
struct Site {
    o: V3,
    a: V3,
    l: V3,
    n: V3,
}

type V3 = [f64; 3];

impl Site {
    fn at(&self, a: f64, l: f64, n: f64) -> V3 {
        add(self.o, add(scale(self.a, a), add(scale(self.l, l), scale(self.n, n))))
    }
}

/// Axis and angle of the rotation whose matrix has the rows `r` (angle 0 for none).
fn axis_angle(r: [[f64; 3]; 3]) -> (V3, f64) {
    let [[r00, r01, r02], [r10, r11, r12], [r20, r21, r22]] = r;
    let v = [r21 - r12, r02 - r20, r10 - r01];
    let angle = libm::atan2(gusset_kernel::norm(v) * 0.5, (r00 + r11 + r22 - 1.0) * 0.5);
    if angle < 1e-12 {
        return ([0.0, 0.0, 1.0], 0.0);
    }
    if let Some(axis) = normalize(v).filter(|_| angle < std::f64::consts::PI - 1e-6) {
        return (axis, angle);
    }
    // Near a half turn: the axis is the largest column of (R + I) / 2.
    let m = [[r00 + 1.0, r01, r02], [r10, r11 + 1.0, r12], [r20, r21, r22 + 1.0]];
    let col = |i: usize| m.map(|row| row.get(i).copied().unwrap_or(0.0));
    let best = (0..3).map(col).max_by(|a, b| dot(*a, *a).total_cmp(&dot(*b, *b)));
    (best.and_then(normalize).unwrap_or([0.0, 0.0, 1.0]), angle)
}

impl Evaluator {
    /// A box of the half-space or slab `lo <= a <= hi` of the site, wider than the body.
    fn bend_block(&self, name: &str, site: &Site, (lo, hi): (f64, f64), reach: f64) -> Result<Tool, EvalError> {
        let k = self.kernel.as_ref();
        // The profile lies across `a`: its frame's normal `l x n` is `a`.
        let frame = Frame { origin: site.at(lo, 0.0, 0.0), x: site.l, y: site.n };
        let sq = [[-reach, -reach], [reach, -reach], [reach, reach], [-reach, reach]];
        let edges: Vec<ProfileEdge> = sq
            .iter()
            .zip(sq.iter().cycle().skip(1))
            .map(|(p, q)| ProfileEdge { seg: Seg2::Line { a: *p, b: *q }, tag: 0 })
            .collect();
        let block = k
            .extrude(
                &Profile { frame, regions: vec![Region2 { outer: Loop2 { edges }, holes: Vec::new() }] },
                &ExtrudeSpec { distance: hi - lo, symmetric: false },
            )
            .map_err(|ke| kernel_err(name, "bend", ke))?;
        let topo = k.topology(&block.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let names = name_body(&OpContext::Import { feature: name }, &block.face_origins, &topo)
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        Ok(Tool { shape: block.shape, names })
    }

    /// The part of `body` inside the block, named after the body's faces.
    fn bend_piece(
        &self,
        name: &str,
        body: &Body,
        block: &Tool,
        empty: &str,
    ) -> Result<(Shape, BodyNames, Topology), EvalError> {
        let on_error = |_: Option<usize>, ke: KernelError| kernel_err(name, "bend", ke);
        self.boolean_tools_refined(
            name,
            BoolOp::Intersect,
            (&body.shape, &body.names),
            std::slice::from_ref(block),
            empty,
            &on_error,
            true,
        )
    }

    /// The box of a shape in the site's frame: (a, l, n) ranges.
    fn site_box(&self, name: &str, site: &Site, shape: &Shape) -> Result<[(f64, f64); 3], EvalError> {
        let k = self.kernel.as_ref();
        let (axis, angle) = axis_angle([site.a, site.l, site.n]);
        let mut local = k
            .transform(shape, &Transform::Translate(scale(site.o, -1.0)))
            .map_err(|ke| kernel_err(name, "bend", ke))?;
        if angle > 0.0 {
            local = k
                .transform(&local.shape, &Transform::Rotate { origin: [0.0; 3], axis, angle })
                .map_err(|ke| kernel_err(name, "bend", ke))?;
        }
        let bb = k.bbox(&local.shape).map_err(|ke| kernel_err(name, "bend", ke))?;
        Ok([(bb.min[0], bb.max[0]), (bb.min[1], bb.max[1]), (bb.min[2], bb.max[2])])
    }

    pub(crate) fn run_bend(&self, s: &mut State, name: &str, b: &BendSpec) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let bad = |m: String| EvalError::new(name, ErrorCode::InvalidReference, m);
        let expr = |m: &str| EvalError::new(name, ErrorCode::Expression, m.to_string());
        if !(b.angle > 1e-6 && b.angle <= std::f64::consts::PI + 1e-9) {
            return Err(expr("the bend angle must be between 0 and 180 deg").with_hint("90 deg for a right angle"));
        }
        if !(b.r >= 0.0 && b.r.is_finite()) {
            return Err(expr("the bend radius must not be negative"));
        }
        if !(0.0..=1.0).contains(&b.k) {
            return Err(expr("k_factor must be between 0 and 1"));
        }
        let (bi, body) = target_body(s, name, Some(b.body))?;
        let sv = s.solved.get(b.sketch).ok_or_else(|| bad(format!("'{}' is not an evaluated sketch", b.sketch)))?;
        let (p, q) =
            *sv.line_points.get(b.line).ok_or_else(|| bad(format!("'{}.{}' is not a line", b.sketch, b.line)))?;
        let frame = sv.frame;
        let (o, end) = (frame.to_world(p), frame.to_world(q));
        let n = normalize(frame.normal()).ok_or_else(|| bad("the sketch has no normal".into()))?;
        let along = normalize(sub(end, o)).ok_or_else(|| bad(format!("line {} has no length", b.line)))?;
        // Left of the line, seen from the sketch normal, is n x along.
        let left = cross(n, along);
        let a = if b.right { scale(left, -1.0) } else { left };
        let l = cross(n, a);
        let site = Site { o, a, l, n };
        let bb = k.bbox(&body.shape).map_err(|ke| kernel_err(name, "bounding box", ke))?;
        let centre = scale(add(bb.min, bb.max), 0.5);
        let reach = 2.0 * (gusset_kernel::norm(bb.size()) + gusset_kernel::norm(sub(centre, o))) + 1.0;
        // The sheet at the line: its thickness and run along the line.
        let probe = 1e-3;
        let thin = self.bend_block(name, &site, (-probe, probe), reach)?;
        let at_line = self
            .bend_piece(name, &body, &thin, "the line does not cross the body")
            .map_err(|e| e.with_hint("draw the bend line on a face of the sheet, across it"))?;
        let [_, (l0, l1), (m0, m1)] = self.site_box(name, &site, &at_line.0)?;
        let t = m1 - m0;
        if t.is_nan() || t <= 1e-6 || (m0.abs() > 1e-6 && m1.abs() > 1e-6) {
            return Err(bad(format!("line {} does not lie on a face of the sheet", b.line))
                .with_hint("sketch the bend line on the sheet's flat face"));
        }
        let ba = b.angle * (b.r + b.k * t);
        let a0 = match b.position {
            BendPosition::Start => 0.0,
            BendPosition::Center => -ba / 2.0,
            BendPosition::End => -ba,
        };
        let a1 = a0 + ba;
        // The zone: a strip of the sheet, the same all across.
        let zone_block = self.bend_block(name, &site, (a0, a1), reach)?;
        let zone = self.bend_piece(name, &body, &zone_block, "the bend zone holds no material")?;
        let zb = self.site_box(name, &site, &zone.0)?;
        let zone_vol = k.mass_properties(&zone.0).map_err(|ke| kernel_err(name, "bend", ke))?.volume;
        let strip = (l1 - l0) * t * ba;
        let near = |x: f64, y: f64| (x - y).abs() <= 1e-6 * (1.0 + x.abs().max(y.abs()));
        let [(za0, za1), (zl0, zl1), (zm0, zm1)] = zb;
        if !(near(za0, a0) && near(za1, a1) && near(zl0, l0) && near(zl1, l1) && near(zm0, m0) && near(zm1, m1))
            || (zone_vol - strip).abs() > 1e-6 * strip.max(1e-9)
        {
            return Err(bad(format!(
                "the bend zone of {} is not one straight strip of the sheet (an edge, cut-out or step lies in it)",
                b.line
            ))
            .with_hint("move the line, or bend where the sheet is a plain strip across the whole zone"));
        }
        let kept = self.bend_block(name, &site, (a0 - reach, a0), reach)?;
        let moved = self.bend_block(name, &site, (a1, a1 + reach), reach)?;
        let stays = self.bend_piece(name, &body, &kept, "nothing stays on the other side of the bend")?;
        let moves = self.bend_piece(name, &body, &moved, "nothing lies beyond the bend zone to bend")?;
        // The axis: on the bend's side, `r` from the inside face, at the zone's start; the
        // part beyond turns about it so that `a` turns towards that side.
        let (n_axis, u) = if b.down { (m0 - b.r, l) } else { (m1 + b.r, scale(l, -1.0)) };
        let pivot = site.at(a0, 0.0, n_axis);
        let laid =
            k.transform(&moves.0, &Transform::Translate(scale(a, -ba))).map_err(|ke| kernel_err(name, "bend", ke))?;
        let turned = k
            .transform(&laid.shape, &Transform::Rotate { origin: pivot, axis: u, angle: b.angle })
            .map_err(|ke| kernel_err(name, "bend", ke))?;
        let topo = k.topology(&turned.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let turned_names =
            name_body(&OpContext::Modify { feature: name, inputs: &[&moves.1] }, &turned.face_origins, &topo)
                .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        // The zone's cross-section at its start, revolved: the frame's normal `l x n` is `a`.
        let pf = Frame { origin: site.at(a0, 0.0, 0.0), x: l, y: n };
        let tag = |part| sheet_tag(part, 0);
        let (inner, outer) = if b.down { (m0, m1) } else { (m1, m0) };
        let seg = |p: [f64; 2], q: [f64; 2], part| ProfileEdge { seg: Seg2::Line { a: p, b: q }, tag: tag(part) };
        let mut edges = vec![
            seg([l0, inner], [l1, inner], SheetPart::BendIn),
            seg([l1, inner], [l1, outer], SheetPart::End),
            seg([l1, outer], [l0, outer], SheetPart::BendOut),
            seg([l0, outer], [l0, inner], SheetPart::Start),
        ];
        if (l1 - l0) * (outer - inner) < 0.0 {
            edges.reverse();
            for e in &mut edges {
                if let Seg2::Line { a: p, b: q } = e.seg {
                    e.seg = Seg2::Line { a: q, b: p };
                }
            }
        }
        let section = Profile { frame: pf, regions: vec![Region2 { outer: Loop2 { edges }, holes: Vec::new() }] };
        // Profile `x` is `l`: the axis runs along `u` (+x or -x).
        let dir = if dot(u, l) > 0.0 { [1.0, 0.0] } else { [-1.0, 0.0] };
        let rev = k
            .revolve(&section, &RevolveSpec { axis_origin: [0.0, n_axis], axis_dir: dir, angle: b.angle })
            .map_err(|ke| kernel_err(name, "bend", ke))?;
        let rtopo = k.topology(&rev.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let at = [b.line.to_string()];
        let rev_names = name_body(&OpContext::Sheet { feature: name, at: &at }, &rev.face_origins, &rtopo)
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        let tools = [Tool { shape: rev.shape, names: rev_names }, Tool { shape: turned.shape, names: turned_names }];
        let on_error = |_: Option<usize>, ke: KernelError| kernel_err(name, "bend", ke);
        let (shape, names, topology) = self.boolean_tools_refined(
            name,
            BoolOp::Union,
            (&stays.0, &stays.1),
            &tools,
            "the bend left nothing",
            &on_error,
            true,
        )?;
        replace_body(s, bi, &body, shape, names, topology);
        // A sheet-metal body's flat pattern keeps its outline (the zone keeps its developed
        // length) and gains the bend; flanges can no longer be bent off it where it was.
        if let Some(flat) = body.sheet.as_ref()
            && let Some(slot) = s.bodies.get_mut(bi)
        {
            let mut nf = (**flat).clone();
            // The pattern is in the tab's sketch frame: the bend line's sketch must share it.
            let same = |f: &Frame| {
                let close = |x: V3, y: V3| x.iter().zip(y).all(|(p, q)| (p - q).abs() < 1e-9);
                close(f.origin, frame.origin) && close(f.x, frame.x) && close(f.y, frame.y)
            };
            let same_plane =
                flat.base.as_ref().and_then(|tb| s.solved.get(&tb.sketch)).is_some_and(|tv| same(&tv.frame));
            if same_plane {
                let d2 = [q[0] - p[0], q[1] - p[1]];
                let len = libm::hypot(d2[0], d2[1]);
                let side = if b.right { -1.0 } else { 1.0 };
                let across = [-d2[1] / len * side, d2[0] / len * side];
                let shift = |x: f64| {
                    [[p[0] + across[0] * x, p[1] + across[1] * x], [q[0] + across[0] * x, q[1] + across[1] * x]]
                };
                nf.bends.push(sheet::FlatBend {
                    line: b.line.to_string(),
                    angle: if b.down { -b.angle } else { b.angle },
                    inner_radius: b.r,
                    k_factor: b.k,
                    allowance: ba,
                    start: shift(a0),
                    end: shift(a1),
                });
            } else {
                nf.changed_by.push(name.to_string());
            }
            nf.base = None;
            nf.contour = None;
            nf.flange_ends.clear();
            let old = &**slot;
            *slot = Arc::new(Body {
                name: old.name.clone(),
                feature: old.feature.clone(),
                shape: old.shape.clone(),
                names: old.names.clone(),
                topology: old.topology.clone(),
                holes: old.holes.clone(),
                sheet: Some(Arc::new(nf)),
            });
        }
        Ok(())
    }
}
