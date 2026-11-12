//! The board as 3D bodies for mechanical work (enclosures, fit): the outline extruded to
//! the stackup's thickness with its holes, optional outer copper, and a simple box per
//! component (or its STEP model, placed by [`model_steps`]).
//!
//! Gusset's frame: X right, **Y up** (board Y is negated), Z up; the board's bottom at
//! z = 0 and its top at z = thickness.

use crate::board::{Board, Footprint, Model3d, PadKind, Place, Pt, Shape as Gfx};
use crate::geom::{self, Contour, Piece};
use gusset_kernel::{ExtrudeSpec, Frame, Kernel, Loop2, Profile, ProfileEdge, Region2, Seg2, Shape, Transform};

/// What to build.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Cut pad holes (plated and non-plated).
    pub holes: bool,
    /// Also cut via holes (many small holes: slower, rarely needed for enclosures).
    pub vias: bool,
    /// A box per footprint.
    pub components: bool,
    /// Box height (mm).
    pub component_height: f64,
    /// Outer copper (pads, tracks, zone fills) as thin bodies.
    pub copper: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { holes: true, vias: false, components: true, component_height: 2.0, copper: false }
    }
}

/// One body to add to the document.
#[derive(Debug, Clone)]
pub struct Body {
    pub name: String,
    pub shape: Shape,
    /// `0xRRGGBB`.
    pub color: u32,
    /// The footprint a component box stands for.
    pub footprint: Option<usize>,
}

/// Board colour (solder mask green), copper and component grey.
pub const BOARD_COLOR: u32 = 0x1f_6b_3a;
pub const COPPER_COLOR: u32 = 0xb8_73_33;
pub const COMPONENT_COLOR: u32 = 0x30_30_30;

/// Most holes cut into the board (each is a loop of the profile).
pub const MAX_HOLES: usize = 20_000;
/// Most copper items per layer.
pub const MAX_COPPER_ITEMS: usize = 20_000;

/// A closed loop with its polygon (for inside tests).
type Ring = (Contour, Vec<Pt>);

fn g(p: Pt) -> Pt {
    [p[0], -p[1]]
}

fn seg_of(piece: &Piece) -> Seg2 {
    match *piece {
        Piece::Line { a, b } => Seg2::Line { a: g(a), b: g(b) },
        Piece::Arc { start, mid, end } => {
            let (s, m, e) = (g(start), g(mid), g(end));
            match geom::circle_through(s, m, e) {
                Some((center, _)) => Seg2::Arc { center, start: s, end: e, ccw: geom::arc_ccw(s, m, e) },
                None => Seg2::Line { a: s, b: e },
            }
        }
    }
}

/// A contour as a profile loop in Gusset's frame, counter-clockwise when `ccw`.
fn loop_of(c: &Contour, ccw: bool, tag: &mut u64) -> Loop2 {
    let mut next = || {
        *tag += 1;
        *tag
    };
    match c {
        Contour::Circle { center, radius } => Loop2 {
            edges: vec![ProfileEdge { seg: Seg2::Circle { center: g(*center), radius: *radius }, tag: next() }],
        },
        Contour::Pieces(p) => {
            // Board Y is flipped, so a loop counter-clockwise in board coordinates is
            // clockwise here.
            let poly: Vec<Pt> = c.polygon(0.01).into_iter().map(g).collect();
            let is_ccw = geom::area(&poly) > 0.0;
            let pieces: Vec<Piece> = if is_ccw == ccw {
                p.clone()
            } else {
                p.iter()
                    .rev()
                    .map(|x| match *x {
                        Piece::Line { a, b } => Piece::Line { a: b, b: a },
                        Piece::Arc { start, mid, end } => Piece::Arc { start: end, mid, end: start },
                    })
                    .collect()
            };
            Loop2 { edges: pieces.iter().map(|x| ProfileEdge { seg: seg_of(x), tag: next() }).collect() }
        }
    }
}

/// A slot (stadium) between two centres.
fn slot(a: Pt, b: Pt, r: f64) -> Contour {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = libm::hypot(dx, dy);
    if l < 1e-9 {
        return Contour::Circle { center: a, radius: r };
    }
    let (nx, ny) = (-dy / l * r, dx / l * r);
    let (ux, uy) = (dx / l * r, dy / l * r);
    Contour::Pieces(vec![
        Piece::Line { a: [a[0] + nx, a[1] + ny], b: [b[0] + nx, b[1] + ny] },
        Piece::Arc { start: [b[0] + nx, b[1] + ny], mid: [b[0] + ux, b[1] + uy], end: [b[0] - nx, b[1] - ny] },
        Piece::Line { a: [b[0] - nx, b[1] - ny], b: [a[0] - nx, a[1] - ny] },
        Piece::Arc { start: [a[0] - nx, a[1] - ny], mid: [a[0] - ux, a[1] - uy], end: [a[0] + nx, a[1] + ny] },
    ])
}

fn dist_to_poly(poly: &[Pt], q: Pt) -> f64 {
    let n = poly.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 { (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min(libm::hypot(q[0] - a[0] - t * dx, q[1] - a[1] - t * dy));
    }
    best
}

/// The board's profile (outline regions with cut-outs and holes) in Gusset's frame, and
/// notes on what was left out.
///
/// # Errors
/// When the outline has no closed loop.
pub fn board_profile(board: &Board, opts: &Options) -> Result<(Profile, Vec<String>), String> {
    let (mut loops, gaps) = geom::outline_loops(board, crate::check::OUTLINE_TOL);
    let mut notes = Vec::new();
    if !gaps.is_empty() {
        notes.push(format!("{} outline pieces do not close and were left out", gaps.len() / 2));
    }
    if loops.is_empty() {
        return Err("the board outline (Edge.Cuts) has no closed loop".into());
    }
    // Largest first: outer outlines, then the cut-outs inside them.
    let mut polys: Vec<(Contour, Vec<Pt>)> = loops
        .drain(..)
        .map(|c| {
            let p = c.polygon(0.01);
            (c, p)
        })
        .collect();
    polys.sort_by(|a, b| {
        geom::area(&b.1).abs().partial_cmp(&geom::area(&a.1).abs()).unwrap_or(std::cmp::Ordering::Equal)
    });
    // (outer contour, outer polygon, cut-outs (contour, polygon))
    let mut regions: Vec<(Contour, Vec<Pt>, Vec<Ring>)> = Vec::new();
    for (c, p) in polys {
        let probe = p.first().copied().unwrap_or([0.0, 0.0]);
        match regions.iter_mut().find(|r| geom::inside(&r.1, probe)) {
            Some(r) => r.2.push((c, p)),
            None => regions.push((c, p, Vec::new())),
        }
    }
    // Holes: (centre for the tests, radius, contour).
    let mut holes: Vec<(Pt, Option<Pt>, f64, Contour)> = Vec::new();
    if opts.holes {
        let (plated, npth) = crate::drill::holes(board);
        for h in plated.iter().chain(&npth) {
            if h.via && !opts.vias {
                continue;
            }
            let r = h.diameter / 2.0;
            if r <= 0.0 {
                continue;
            }
            let c = match h.slot_end {
                None => Contour::Circle { center: h.at, radius: r },
                Some(e) => slot(h.at, e, r),
            };
            holes.push((h.at, h.slot_end, r, c));
        }
    }
    let mut skipped = 0usize;
    let mut tag = 0u64;
    let mut out = Vec::new();
    let mut placed: Vec<(Pt, Option<Pt>, f64)> = Vec::new();
    let mut region_holes: Vec<Vec<Loop2>> = vec![Vec::new(); regions.len()];
    for (at, end, r, c) in holes {
        if placed.len() >= MAX_HOLES {
            skipped += 1;
            continue;
        }
        let pts: Vec<Pt> = match end {
            Some(e) => vec![at, e],
            None => vec![at],
        };
        let fits = |poly: &[Pt]| pts.iter().all(|q| geom::inside(poly, *q) && dist_to_poly(poly, *q) > r + 0.01);
        let clear_of = |poly: &[Pt]| pts.iter().all(|q| !geom::inside(poly, *q) && dist_to_poly(poly, *q) > r + 0.01);
        let Some(ri) = regions.iter().position(|reg| fits(&reg.1) && reg.2.iter().all(|cut| clear_of(&cut.1))) else {
            skipped += 1;
            continue;
        };
        // Overlapping holes would make the profile invalid: keep the first. Compared by
        // centres (slots by both ends), which is enough for real boards.
        let overlaps = placed.iter().any(|(q, qe, qr)| {
            let ends_a: Vec<Pt> = std::iter::once(at).chain(end).collect();
            let ends_b: Vec<Pt> = std::iter::once(*q).chain(*qe).collect();
            ends_a.iter().any(|a| ends_b.iter().any(|b| libm::hypot(a[0] - b[0], a[1] - b[1]) < r + qr + 0.01))
        });
        if overlaps {
            skipped += 1;
            continue;
        }
        placed.push((at, end, r));
        if let Some(v) = region_holes.get_mut(ri) {
            v.push(loop_of(&c, false, &mut tag));
        }
    }
    if skipped > 0 {
        notes.push(format!("{skipped} holes were not cut (outside the outline, crossing it, or overlapping)"));
    }
    for ((outer, _, cuts), hs) in regions.iter().zip(region_holes) {
        let mut h: Vec<Loop2> = cuts.iter().map(|c| loop_of(&c.0, false, &mut tag)).collect();
        h.extend(hs);
        out.push(Region2 { outer: loop_of(outer, true, &mut tag), holes: h });
    }
    Ok((Profile { frame: Frame::XY, regions: out }, notes))
}

/// A footprint's component box in board coordinates: the extents of its fabrication
/// drawing (`F.Fab`/`B.Fab`, the part's body) when it has one, else of its pads, in the
/// footprint's own frame. `None` without either.
pub fn component_box(board: &Board, index: usize) -> Option<[Pt; 4]> {
    let fp = board.footprints.get(index)?;
    let inv = |p: Pt| {
        // Board → footprint frame: the inverse of `Place::apply`.
        let (s, c) = (libm::sin(fp.angle.to_radians()), libm::cos(fp.angle.to_radians()));
        let (dx, dy) = (p[0] - fp.at[0], p[1] - fp.at[1]);
        [dx * c - dy * s, dx * s + dy * c]
    };
    let fab = if fp.bottom { "B.Fab" } else { "F.Fab" };
    let mut pts: Vec<Pt> = Vec::new();
    for gr in board.graphics.iter().filter(|gr| gr.footprint == Some(index) && gr.layer == fab) {
        match &gr.shape {
            Gfx::Line { a, b } => pts.extend([*a, *b]),
            Gfx::Arc { start, mid, end } => pts.extend(geom::arc_points(*start, *mid, *end, 0.05)),
            Gfx::Circle { center, radius } => {
                pts.extend([[center[0] - radius, center[1] - radius], [center[0] + radius, center[1] + radius]]);
            }
            Gfx::Poly { pts: p } => pts.extend(p.iter().copied()),
        }
    }
    let mut local: Vec<Pt> = pts.into_iter().map(inv).collect();
    if local.len() < 2 {
        local.clear();
        for &i in &fp.pads {
            let Some(pad) = board.pads.get(i) else { continue };
            if pad.kind == PadKind::Npth {
                continue;
            }
            if let Some(c) = geom::pad_contour(pad, 0.0) {
                local.extend(c.polygon(0.05).into_iter().map(inv));
            }
        }
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in &local {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    if !(hi[0] - lo[0] > 1e-6 && hi[1] - lo[1] > 1e-6) {
        return None;
    }
    let place = Place { at: fp.at, angle: fp.angle };
    Some([place.apply(lo), place.apply([hi[0], lo[1]]), place.apply(hi), place.apply([lo[0], hi[1]])])
}

fn extrude_poly(k: &dyn Kernel, pts: &[Pt], z: f64, height: f64) -> Option<Shape> {
    let n = pts.len();
    let mut tag = 0u64;
    let pieces: Vec<Piece> =
        (0..n).filter_map(|i| Some(Piece::Line { a: *pts.get(i)?, b: *pts.get((i + 1) % n)? })).collect();
    let lp = loop_of(&Contour::Pieces(pieces), true, &mut tag);
    extrude_loops(k, lp, Vec::new(), z, height)
}

fn extrude_loops(k: &dyn Kernel, outer: Loop2, holes: Vec<Loop2>, z: f64, height: f64) -> Option<Shape> {
    let frame = Frame { origin: [0.0, 0.0, z], ..Frame::XY };
    let p = Profile { frame, regions: vec![Region2 { outer, holes }] };
    k.extrude(&p, &ExtrudeSpec { distance: height, symmetric: false }).ok().map(|r| r.shape)
}

/// Builds the board body, the copper and the component boxes.
///
/// # Errors
/// When the outline does not close or the kernel cannot extrude the board.
pub fn build(board: &Board, k: &dyn Kernel, opts: &Options) -> Result<(Vec<Body>, Vec<String>), String> {
    let (profile, mut notes) = board_profile(board, opts)?;
    let t = board.thickness;
    let mut bodies = Vec::new();
    let board_shape = match k.extrude(&profile, &ExtrudeSpec { distance: t, symmetric: false }) {
        Ok(r) => r.shape,
        Err(e) if profile.regions.iter().any(|r| !r.holes.is_empty()) => {
            // Retry without holes rather than lose the board (kernels without hole support).
            notes.push(format!("the board's holes could not be cut ({}); the board came in without them", e.message));
            let plain = Profile {
                frame: profile.frame,
                regions: profile
                    .regions
                    .iter()
                    .map(|r| Region2 { outer: r.outer.clone(), holes: Vec::new() })
                    .collect(),
            };
            k.extrude(&plain, &ExtrudeSpec { distance: t, symmetric: false }).map_err(|e| e.message)?.shape
        }
        Err(e) => return Err(e.message),
    };
    bodies.push(Body { name: "Board".into(), shape: board_shape, color: BOARD_COLOR, footprint: None });
    if opts.copper {
        let cu_t =
            board.stackup.iter().find(|l| l.kind == "copper" && l.thickness > 0.0).map_or(0.035, |l| l.thickness);
        for (layer, z, h) in [("F.Cu", t, cu_t), ("B.Cu", 0.0, -cu_t)] {
            let (shapes, skipped) = copper_shapes(board, k, layer, z, h);
            if skipped > 0 {
                notes.push(format!("{layer}: {skipped} copper items could not be built"));
            }
            if shapes.is_empty() {
                continue;
            }
            match k.compound(&shapes) {
                Ok(c) => {
                    bodies.push(Body { name: layer.to_string(), shape: c.shape, color: COPPER_COLOR, footprint: None })
                }
                Err(e) => notes.push(format!("{layer}: {}", e.message)),
            }
        }
    }
    if opts.components {
        let h = opts.component_height.clamp(0.01, 1000.0);
        let mut missing = 0usize;
        for (i, fp) in board.footprints.iter().enumerate() {
            if fp.attrs.iter().any(|a| a == "board_only") {
                continue;
            }
            let Some(b) = component_box(board, i) else {
                missing += 1;
                continue;
            };
            let (z, height) = if fp.bottom { (0.0, -h) } else { (t, h) };
            match extrude_poly(k, &b, z, height) {
                Some(s) => bodies.push(Body {
                    name: component_name(fp, i),
                    shape: s,
                    color: COMPONENT_COLOR,
                    footprint: Some(i),
                }),
                None => missing += 1,
            }
        }
        if missing > 0 {
            notes.push(format!("{missing} footprints have no pads or body drawing to size a box from"));
        }
    }
    Ok((bodies, notes))
}

/// A component body's name: its reference (untrusted text, cleaned) or `Part<n>`.
pub fn component_name(fp: &Footprint, i: usize) -> String {
    let r: String =
        fp.reference.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')).take(40).collect();
    if r.is_empty() { format!("Part{}", i + 1) } else { r }
}

fn copper_shapes(board: &Board, k: &dyn Kernel, layer: &str, z: f64, h: f64) -> (Vec<Shape>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0usize;
    let mut tag = 0u64;
    let mut add = |c: Contour, out: &mut Vec<Shape>| {
        if out.len() >= MAX_COPPER_ITEMS {
            skipped += 1;
            return;
        }
        match extrude_loops(k, loop_of(&c, true, &mut tag), Vec::new(), z, h) {
            Some(s) => out.push(s),
            None => skipped += 1,
        }
    };
    for pad in board.pads.iter().filter(|p| p.kind != PadKind::Npth && board.on_layer(&p.layers, layer)) {
        if let Some(c) = geom::pad_contour(pad, 0.0) {
            add(c, &mut out);
        }
    }
    for tr in board.tracks.iter().filter(|tr| tr.layer == layer && tr.width > 0.0) {
        let r = tr.width / 2.0;
        match tr.mid {
            None => add(slot(tr.start, tr.end, r), &mut out),
            Some(m) => {
                let pts = geom::arc_points(tr.start, m, tr.end, 0.02);
                for w in pts.windows(2) {
                    if let [a, b] = w {
                        add(slot(*a, *b, r), &mut out);
                    }
                }
            }
        }
    }
    for v in
        board.vias.iter().filter(|v| v.size > 0.0 && board.span(&v.layers[0], &v.layers[1]).iter().any(|l| l == layer))
    {
        add(Contour::Circle { center: v.at, radius: v.size / 2.0 }, &mut out);
    }
    for zone in board.zones.iter().filter(|z| !z.keepout) {
        for (l, poly) in zone.fills.iter().filter(|f| f.0 == layer) {
            let _ = l;
            if poly.len() < 3 {
                continue;
            }
            let n = poly.len();
            let pieces: Vec<Piece> =
                (0..n).filter_map(|i| Some(Piece::Line { a: *poly.get(i)?, b: *poly.get((i + 1) % n)? })).collect();
            add(Contour::Pieces(pieces), &mut out);
        }
    }
    (out, skipped)
}

/// The kernel transforms that place a footprint's 3D model (read in the model's own frame,
/// mm) on the board: the model's scale, rotation (about X, then Y, then Z) and offset, the
/// flip to the bottom side, the footprint's rotation and its position on the board surface.
pub fn model_steps(board: &Board, fp: &Footprint, m: &Model3d) -> Vec<Transform> {
    let o = [0.0; 3];
    let mut v = Vec::new();
    let positive = m.scale.iter().all(|s| s.is_finite() && *s > 0.0);
    if positive && m.scale.iter().any(|s| (s - 1.0).abs() > 1e-9) {
        v.push(Transform::Scale { center: o, factors: m.scale });
    }
    for (i, axis) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].into_iter().enumerate() {
        let a = m.rotate.get(i).copied().unwrap_or(0.0);
        if a.abs() > 1e-9 {
            v.push(Transform::Rotate { origin: o, axis, angle: a.to_radians() });
        }
    }
    if m.offset.iter().any(|x| x.abs() > 1e-12) {
        v.push(Transform::Translate(m.offset));
    }
    if fp.bottom {
        v.push(Transform::Rotate { origin: o, axis: [1.0, 0.0, 0.0], angle: std::f64::consts::PI });
    }
    if fp.angle.abs() > 1e-9 {
        // Counter-clockwise on screen is counter-clockwise about +Z once Y points up.
        v.push(Transform::Rotate { origin: o, axis: [0.0, 0.0, 1.0], angle: fp.angle.to_radians() });
    }
    let z = if fp.bottom { 0.0 } else { board.thickness };
    v.push(Transform::Translate([fp.at[0], -fp.at[1], z]));
    v
}
