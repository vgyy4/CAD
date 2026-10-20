//! 2D drawings (ADR 0032): a drawing of the document and the evaluated model become a
//! [`Scene`] of primitives on the sheet, which [`svg`] and [`dxf`] (and the app) draw.
//!
//! Views are projected with [`Kernel::hidden_lines`] and cached per (shapes, view), so
//! moving a note or a dimension does not project the model again.

pub mod bom;
mod centers;
pub mod detail;
pub mod dims;
pub mod dxf;
pub mod dxf_read;
mod gdt;
pub mod pdf;
pub mod raster;
pub mod scene;
pub mod section;
pub mod svg;

pub use scene::{Anchor, Handle, Item, PlacedView, Prim, Scene, Style};

use gusset_doc::drawing::{Drawing, DrawingItem, Projection, Side, ViewDef, ViewPlacement};
use gusset_eval::{Body, Evaluation};
use gusset_kernel::{HlrClass, HlrCurve, HlrView, Kernel, ProjCurve, Shape};
use gusset_units::Quantity;
use gusset_units::expr::Expr;
use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::sync::Arc;

/// Distance between a projected view and its parent when the drawing gives none (mm).
pub const DEFAULT_GAP: f64 = 20.0;
/// Chordal deflection of curves on the sheet (mm); divided by the scale for the model.
const SHEET_DEFLECTION: f64 = 0.02;
/// Scales a sheet or view may have (review D1: line lengths, dashes, hatch counts and the
/// projection's deflection all follow the scale).
pub const MIN_SCALE: f64 = 1e-4;
pub const MAX_SCALE: f64 = 1e4;
/// Most hatch lines in one section view.
const MAX_HATCH: usize = 200_000;

/// Views kept in a [`HlrCache`].
const CACHE_ENTRIES: usize = 256;

/// What a drawing is built from.
pub struct Context<'a> {
    pub ev: &'a Evaluation,
    pub kernel: &'a dyn Kernel,
    /// The document's length unit: dimension values are written in it.
    pub units: gusset_units::quantity::LengthUnit,
    /// Stops the build between views ("Stop rebuild", deadlines; review D3).
    pub cancel: Option<&'a std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Context({} bodies, kernel {})", self.ev.bodies.len(), self.kernel.name())
    }
}

/// The shapes of a view, its key (axes and deflection as bits) and its curves.
type CacheEntry = (Vec<Shape>, [u64; 7], Arc<Vec<HlrCurve>>);

/// Hidden-line projections by (shapes, view): the most recent [`CACHE_ENTRIES`] views.
#[derive(Default)]
pub struct HlrCache {
    entries: Vec<CacheEntry>,
    /// Sections by (shapes, plane, view X): the cut shapes and their cut faces.
    cuts: Vec<(Vec<Shape>, [u64; 8], Arc<Cut>)>,
    /// Projections computed (not found in the cache) so far.
    misses: usize,
}

/// A section's cut bodies and the triangles of its cut faces (view coordinates).
struct Cut {
    shapes: Vec<Shape>,
    faces: Vec<[[f64; 2]; 3]>,
}

impl std::fmt::Debug for HlrCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HlrCache({} views)", self.entries.len())
    }
}

impl HlrCache {
    /// How many projections were computed rather than reused.
    pub fn misses(&self) -> usize {
        self.misses
    }

    /// Forgets everything (a new document, undo across many edits).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cuts.clear();
    }

    /// Drops entries for shapes the evaluation no longer has, so old model revisions are
    /// not kept alive (review D14).
    fn retain_current(&mut self, ev: &Evaluation) {
        let live = |s: &Shape| ev.bodies.iter().any(|b| b.shape.ptr_eq(s));
        self.entries.retain(|(shapes, _, _)| shapes.iter().all(live));
        self.cuts.retain(|(shapes, _, _)| shapes.iter().all(live));
    }

    fn get(&mut self, k: &dyn Kernel, shapes: &[Shape], view: &HlrView) -> Result<Arc<Vec<HlrCurve>>, String> {
        let key = [
            view.toward[0].to_bits(),
            view.toward[1].to_bits(),
            view.toward[2].to_bits(),
            view.x[0].to_bits(),
            view.x[1].to_bits(),
            view.x[2].to_bits(),
            view.deflection.to_bits(),
        ];
        let same = |s: &[Shape]| s.len() == shapes.len() && s.iter().zip(shapes).all(|(a, b)| a.ptr_eq(b));
        if let Some(i) = self.entries.iter().position(|(s, k, _)| *k == key && same(s)) {
            let e = self.entries.remove(i);
            let out = e.2.clone();
            self.entries.push(e);
            return Ok(out);
        }
        let shape = match shapes {
            [one] => one.clone(),
            many => k.compound(many).map_err(|e| e.message)?.shape,
        };
        let curves = Arc::new(k.hidden_lines(&shape, view).map_err(|e| e.message)?);
        self.misses += 1;
        if self.entries.len() >= CACHE_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push((shapes.to_vec(), key, curves.clone()));
        Ok(curves)
    }

    fn cut(
        &mut self,
        k: &dyn Kernel,
        shapes: &[Shape],
        n: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        // The plane's offset along `n`, and a half section's split along `x`.
        (c, half): (f64, Option<f64>),
    ) -> Result<Arc<Cut>, String> {
        let key = [n[0], n[1], n[2], x[0], x[1], x[2], c, half.unwrap_or(f64::NAN)].map(f64::to_bits);
        let same = |s: &[Shape]| s.len() == shapes.len() && s.iter().zip(shapes).all(|(a, b)| a.ptr_eq(b));
        if let Some((_, _, cut)) = self.cuts.iter().find(|(s, kk, _)| *kk == key && same(s)) {
            return Ok(cut.clone());
        }
        let kept = section::cut(k, shapes, n, x, c, half)?;
        let faces = section::cut_faces(k, &kept, n, x, y, c)?;
        let cut = Arc::new(Cut { shapes: kept, faces });
        if self.cuts.len() >= CACHE_ENTRIES {
            self.cuts.remove(0);
        }
        self.cuts.push((shapes.to_vec(), key, cut.clone()));
        Ok(cut)
    }
}

fn norm(v: [f64; 3]) -> Option<[f64; 3]> {
    let l = libm::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    (l > 1e-12).then(|| [v[0] / l, v[1] / l, v[2] / l])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn neg(a: [f64; 3]) -> [f64; 3] {
    [-a[0], -a[1], -a[2]]
}

/// Orthonormal (toward, x, y) from a direction towards the viewer and a rough X axis.
fn frame(toward: [f64; 3], x: [f64; 3]) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    let t = norm(toward)?;
    let y = norm(cross(t, x))?;
    Some((t, cross(y, t), y))
}

/// The sheet's width and height in mm.
pub fn sheet_size(d: &Drawing) -> [f64; 2] {
    let [w, h] = d.size.mm();
    if d.landscape { [w, h] } else { [h, w] }
}

/// A scale as a ratio: 0.5 is "1:2", 2 is "2:1".
pub fn scale_text(s: f64) -> String {
    let n = |v: f64| {
        let r = libm::round(v * 1000.0) / 1000.0;
        if libm::fabs(r - libm::round(r)) < 1e-9 { format!("{}", r as i64) } else { format!("{r}") }
    };
    if s >= 1.0 { format!("{}:1", n(s)) } else { format!("1:{}", n(1.0 / s)) }
}

/// An expression as a plain number (a length in mm, or a number without unit).
/// A model length (section offsets, detail circles) in mm: a plain number is in the
/// document's unit (review N4, as for parameters).
fn model_len(e: &Expr, ev: &Evaluation, units: gusset_units::quantity::LengthUnit) -> Result<f64, String> {
    let scope = |n: &str| ev.params.get(n).and_then(|r| r.as_ref().ok().copied());
    let q: Quantity = gusset_units::expr::eval(e, &scope).map_err(|err| format!("'{e}': {}", err.message))?;
    if q.dim.is_none() {
        Ok(q.value * units.unit().factor)
    } else if q.dim == gusset_units::Dim::LENGTH {
        Ok(q.value)
    } else {
        Err(format!("'{e}' must be a length, not {}", q.dim.name()))
    }
}

fn number(e: &Expr, ev: &Evaluation) -> Result<f64, String> {
    let scope = |n: &str| ev.params.get(n).and_then(|r| r.as_ref().ok().copied());
    let q: Quantity = gusset_units::expr::eval(e, &scope).map_err(|err| format!("'{e}': {}", err.message))?;
    if q.dim.is_none() || q.dim == gusset_units::Dim::LENGTH {
        Ok(q.value)
    } else {
        Err(format!("'{e}' must be a number or a length, not {}", q.dim.name()))
    }
}

fn find_body<'a>(ev: &'a Evaluation, name: &str) -> Option<&'a Arc<Body>> {
    ev.bodies.iter().find(|b| b.name == name).or_else(|| ev.bodies.iter().find(|b| b.feature == name))
}

/// A view before placement: its bodies, axes and projected curves (model mm, view axes).
struct Projected {
    bodies: Vec<Arc<Body>>,
    toward: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    scale: f64,
    curves: Arc<Vec<HlrCurve>>,
    /// A section: its cut faces, and where its plane crosses the parent view.
    section: Option<SectionInfo>,
    /// A detail: its parent and circle (view coordinates, model mm).
    detail: Option<(String, [f64; 2], f64)>,
    /// A detail of a section: the parent's cut, hatched inside the circle (review D8).
    detail_cut: Option<Arc<Cut>>,
}

struct SectionInfo {
    cut: Arc<Cut>,
    label: String,
    parent: String,
    /// The plane in the parent view: along the sheet's Y (vertical) at this sheet X, or
    /// along X at this sheet Y; and the viewing direction on the sheet.
    vertical: bool,
    pos: f64,
    look: [f64; 2],
}

/// Normalized arc angles: counterclockwise from `s` to `e > s`.
fn ccw_angles(start: f64, end: f64, ccw: bool) -> (f64, f64) {
    let (s, e) = if ccw { (start, end) } else { (end, start) };
    // e in (s, s + TAU]; no loop, so non-finite input cannot spin (review D14).
    let d = (e - s).rem_euclid(TAU);
    (s, s + if d <= 0.0 || !d.is_finite() { TAU } else { d })
}

fn arc_points(center: [f64; 2], radius: f64, s: f64, e: f64, n: usize) -> impl Iterator<Item = [f64; 2]> {
    (0..=n).map(move |i| {
        let a = s + (e - s) * i as f64 / n as f64;
        [center[0] + radius * libm::cos(a), center[1] + radius * libm::sin(a)]
    })
}

fn bounds(curves: &[HlrCurve], with_hidden: bool) -> Option<([f64; 2], [f64; 2])> {
    let mut b: Option<([f64; 2], [f64; 2])> = None;
    let mut add = |p: [f64; 2]| {
        b = Some(match b {
            None => (p, p),
            Some((lo, hi)) => ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]),
        });
    };
    for c in curves.iter().filter(|c| c.visible || with_hidden) {
        match &c.curve {
            ProjCurve::Line(a, z) => {
                add(*a);
                add(*z);
            }
            ProjCurve::Arc { center, radius, start, end, ccw } => {
                let (s, e) = ccw_angles(*start, *end, *ccw);
                arc_points(*center, *radius, s, e, 64).for_each(&mut add);
            }
            ProjCurve::Polyline(p) => p.iter().copied().for_each(&mut add),
        }
    }
    b
}

/// The child view's axes when seen from `side` of a view with these axes.
fn side_axes(side: Side, t: [f64; 3], x: [f64; 3], y: [f64; 3]) -> ([f64; 3], [f64; 3], [f64; 3]) {
    match side {
        Side::Top => (y, x, neg(t)),
        Side::Bottom => (neg(y), x, t),
        Side::Right => (x, cross(y, x), y),
        Side::Left => (neg(x), cross(y, neg(x)), y),
    }
}

struct Builder<'a, 'c> {
    d: &'a Drawing,
    cx: &'a Context<'c>,
    cache: &'a mut HlrCache,
    sheet_scale: f64,
    placed: BTreeMap<String, Result<(Projected, PlacedView), String>>,
}

impl Builder<'_, '_> {
    fn view_scale(&self, v: &ViewDef, parent: Option<f64>) -> Result<f64, String> {
        let s = match &v.scale {
            Some(e) => number(e, self.cx.ev)?,
            None => parent.unwrap_or(self.sheet_scale),
        };
        if (MIN_SCALE..=MAX_SCALE).contains(&s) {
            Ok(s)
        } else {
            Err(format!("view {}: the scale must be between 1:10000 and 10000:1", v.name))
        }
    }

    /// Projects and places view `name` (its parents first).
    fn place(&mut self, name: &str, depth: usize) -> Result<(), String> {
        if self.placed.contains_key(name) {
            return Ok(());
        }
        let v = self.d.view(name).ok_or_else(|| format!("no view '{name}'"))?.clone();
        if depth > self.d.items.len() {
            return Err(format!("view {name} is projected from itself"));
        }
        let r = self.place_one(&v, depth);
        self.placed.insert(name.to_string(), r);
        Ok(())
    }

    /// A detail view: the parent's curves inside a circle, enlarged (default twice the
    /// parent's scale), centred at `at`.
    fn place_detail(&mut self, v: &ViewDef, depth: usize) -> Result<(Projected, PlacedView), String> {
        let ViewPlacement::Detail { of, center, radius, at } = &v.placement else {
            return Err(format!("view {}: not a detail", v.name));
        };
        let ev = self.cx.ev;
        self.place(of, depth + 1)?;
        let (bodies, t, x, y, pscale, curves, cut) = match self.placed.get(of.as_str()) {
            Some(Ok((p, _))) => (
                p.bodies.clone(),
                p.toward,
                p.x,
                p.y,
                p.scale,
                p.curves.clone(),
                p.section.as_ref().map(|s| s.cut.clone()).or_else(|| p.detail_cut.clone()),
            ),
            Some(Err(e)) => return Err(format!("view {}: its parent {of} failed: {e}", v.name)),
            None => return Err(format!("view {}: no view '{of}'", v.name)),
        };
        let scale = self.view_scale(v, Some(2.0 * pscale))?;
        let u = self.cx.units;
        let c = [model_len(&center[0], ev, u)?, model_len(&center[1], ev, u)?];
        let r = model_len(radius, ev, u)?;
        if !(r > 0.0 && r.is_finite()) {
            return Err(format!("view {}: the radius must be positive", v.name));
        }
        let at = [number(&at[0], ev)?, number(&at[1], ev)?];
        let clipped = detail::clip(&curves, c, r, SHEET_DEFLECTION / scale);
        let origin = [at[0] - scale * c[0], at[1] - scale * c[1]];
        let pv = PlacedView {
            name: v.name.clone(),
            min: [at[0] - scale * r, at[1] - scale * r],
            max: [at[0] + scale * r, at[1] + scale * r],
            scale,
            origin,
            toward: t,
            x,
            y,
            error: None,
        };
        let p = Projected {
            bodies,
            toward: t,
            x,
            y,
            scale,
            curves: Arc::new(clipped),
            section: None,
            detail: Some((of.clone(), c, r)),
            detail_cut: cut,
        };
        Ok((p, pv))
    }

    fn place_one(&mut self, v: &ViewDef, depth: usize) -> Result<(Projected, PlacedView), String> {
        if matches!(v.placement, ViewPlacement::Detail { .. }) {
            return self.place_detail(v, depth);
        }
        let ev = self.cx.ev;
        let mut section_info: Option<SectionInfo> = None;
        let (bodies, axes, scale, parent) = match &v.placement {
            ViewPlacement::Base { bodies, from, .. } => {
                let list: Vec<Arc<Body>> = if bodies.is_empty() {
                    ev.bodies.clone()
                } else {
                    bodies
                        .iter()
                        .map(|b| find_body(ev, b).cloned().ok_or_else(|| format!("view {}: no body '{b}'", v.name)))
                        .collect::<Result<_, _>>()?
                };
                let (t, x) = from.axes();
                let axes = frame(t, x).ok_or_else(|| format!("view {}: bad direction", v.name))?;
                (list, axes, self.view_scale(v, None)?, None)
            }
            ViewPlacement::Detail { .. } => return Err(format!("view {}: a detail is placed apart", v.name)),
            ViewPlacement::Projected { of, side, gap } => {
                self.place(of, depth + 1)?;
                let parent = match self.placed.get(of.as_str()) {
                    Some(Ok((p, pv))) => (p.bodies.clone(), p.toward, p.x, p.y, p.scale, pv.clone()),
                    Some(Err(e)) => return Err(format!("view {}: its parent {of} failed: {e}", v.name)),
                    None => return Err(format!("view {}: no view '{of}'", v.name)),
                };
                let (bodies, t, x, y, pscale, mut pv) = parent;
                // Earlier views projected from the same parent to the same side: this one
                // goes beyond them, not over them (dogfooding F5-4).
                let siblings: Vec<String> = self
                    .d
                    .items
                    .iter()
                    .filter_map(|i| match i {
                        DrawingItem::View(w) => Some(w),
                        _ => None,
                    })
                    .take_while(|w| w.name != v.name)
                    .filter(|w| matches!(&w.placement, ViewPlacement::Projected { of: o, side: sd, .. } if o == of && sd == side))
                    .map(|w| w.name.clone())
                    .collect();
                for sib in siblings {
                    if self.place(&sib, depth + 1).is_ok()
                        && let Some(Ok((_, b))) = self.placed.get(sib.as_str())
                    {
                        // Grow the parent's box along the stacking axis only, so the
                        // alignment across it is kept.
                        let (lo, hi) = (b.min, b.max);
                        match side {
                            Side::Top | Side::Bottom => {
                                pv.min[1] = pv.min[1].min(lo[1]);
                                pv.max[1] = pv.max[1].max(hi[1]);
                            }
                            Side::Left | Side::Right => {
                                pv.min[0] = pv.min[0].min(lo[0]);
                                pv.max[0] = pv.max[0].max(hi[0]);
                            }
                        }
                    }
                }
                let (ct, cxx, _) = side_axes(*side, t, x, y);
                let axes = frame(ct, cxx).ok_or_else(|| format!("view {}: bad direction", v.name))?;
                if let Some(sec) = &v.section {
                    // The plane is perpendicular to the parent axis pointing at `side` (the
                    // new view's `toward`); it crosses the parent view as a line.
                    let shapes: Vec<Shape> = bodies.iter().map(|b| b.shape.clone()).collect();
                    let c = match &sec.cut_at {
                        Some(e) => model_len(e, ev, self.cx.units)?,
                        None => {
                            let (lo, hi, _, _) = section::extent(self.cx.kernel, &shapes, ct)
                                .map_err(|e| format!("view {}: {e}", v.name))?;
                            (lo + hi) / 2.0
                        }
                    };
                    let (st, sx, sy) = axes;
                    // A half section splits at the part's middle along the view's X.
                    let half = if sec.half {
                        let (lo, hi, _, _) = section::extent(self.cx.kernel, &shapes, sx)
                            .map_err(|e| format!("view {}: {e}", v.name))?;
                        Some((lo + hi) / 2.0)
                    } else {
                        None
                    };
                    let cut = self
                        .cache
                        .cut(self.cx.kernel, &shapes, st, sx, sy, (c, half))
                        .map_err(|e| format!("view {}: {e}", v.name))?;
                    // In the parent, `ct` is ±(its x) or ±(its y).
                    let vertical = matches!(side, Side::Left | Side::Right);
                    let sign = if matches!(side, Side::Top | Side::Right) { 1.0 } else { -1.0 };
                    let (o, s) = (if vertical { pv.origin[0] } else { pv.origin[1] }, pv.scale);
                    let pos = o + s * sign * c;
                    let look = if vertical { [-sign, 0.0] } else { [0.0, -sign] };
                    section_info =
                        Some(SectionInfo { cut, label: sec.label.clone(), parent: of.clone(), vertical, pos, look });
                }
                let g = match gap {
                    Some(e) => number(e, ev)?,
                    None => DEFAULT_GAP,
                };
                (bodies, axes, self.view_scale(v, Some(pscale))?, Some((pv, *side, g, pscale)))
            }
        };
        if bodies.is_empty() {
            return Err(format!("view {}: there is no body to show", v.name));
        }
        let (t, x, y) = axes;
        let shapes: Vec<Shape> = match &section_info {
            Some(s) => s.cut.shapes.clone(),
            None => bodies.iter().map(|b| b.shape.clone()).collect(),
        };
        let hv = HlrView { toward: t, x, exact: true, deflection: SHEET_DEFLECTION / scale };
        let curves = self.cache.get(self.cx.kernel, &shapes, &hv).map_err(|e| format!("view {}: {e}", v.name))?;
        let (min, max) = bounds(&curves, true).ok_or_else(|| format!("view {}: nothing to show", v.name))?;
        let size = [scale * (max[0] - min[0]), scale * (max[1] - min[1])];
        let centre = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0];
        let origin = match (&v.placement, parent) {
            (ViewPlacement::Base { at, .. }, _) => {
                let at = [number(&at[0], ev)?, number(&at[1], ev)?];
                [at[0] - scale * centre[0], at[1] - scale * centre[1]]
            }
            (ViewPlacement::Projected { .. }, Some((pv, side, gap, pscale))) => {
                // Placed on the side the projection gives (ISO 128-30): first angle puts
                // the view of the top below its parent, third angle above.
                let first = self.d.projection == Projection::First;
                let (below, left) = match side {
                    Side::Top => (first, false),
                    Side::Bottom => (!first, false),
                    Side::Right => (false, first),
                    Side::Left => (false, !first),
                };
                let aligned = (scale - pscale).abs() < 1e-12;
                let pc = [(pv.min[0] + pv.max[0]) / 2.0, (pv.min[1] + pv.max[1]) / 2.0];
                match side {
                    Side::Top | Side::Bottom => {
                        let ox = if aligned { pv.origin[0] } else { pc[0] - scale * centre[0] };
                        let lo = if below { pv.min[1] - gap - size[1] } else { pv.max[1] + gap };
                        [ox, lo - scale * min[1]]
                    }
                    Side::Left | Side::Right => {
                        let oy = if aligned { pv.origin[1] } else { pc[1] - scale * centre[1] };
                        let lo = if left { pv.min[0] - gap - size[0] } else { pv.max[0] + gap };
                        [lo - scale * min[0], oy]
                    }
                }
            }
            (ViewPlacement::Detail { .. }, _) | (ViewPlacement::Projected { .. }, None) => {
                return Err(format!("view {}: no parent", v.name));
            }
        };
        let pv = PlacedView {
            name: v.name.clone(),
            min: [origin[0] + scale * min[0], origin[1] + scale * min[1]],
            max: [origin[0] + scale * max[0], origin[1] + scale * max[1]],
            scale,
            origin,
            toward: t,
            x,
            y,
            error: None,
        };
        Ok((
            Projected { bodies, toward: t, x, y, scale, curves, section: section_info, detail: None, detail_cut: None },
            pv,
        ))
    }
}

/// Draws the projected curves of a placed view.
fn draw_view(scene: &mut Scene, v: &ViewDef, p: &Projected, pv: &PlacedView, iso: bool) {
    let hidden = v.hidden.unwrap_or(!iso);
    let at = |q: [f64; 2]| [pv.origin[0] + pv.scale * q[0], pv.origin[1] + pv.scale * q[1]];
    for c in p.curves.iter() {
        let style = match (c.visible, c.class) {
            (true, HlrClass::Smooth) if v.tangent => Style::Tangent,
            (_, HlrClass::Smooth) => continue,
            (true, _) => Style::Visible,
            (false, _) if hidden => Style::Hidden,
            (false, _) => continue,
        };
        let prim = match &c.curve {
            ProjCurve::Line(a, b) => Prim::Line { a: at(*a), b: at(*b) },
            ProjCurve::Arc { center, radius, start, end, ccw } => {
                let (s, e) = ccw_angles(*start, *end, *ccw);
                Prim::Arc { center: at(*center), radius: pv.scale * radius, start: s, end: e }
            }
            ProjCurve::Polyline(q) => Prim::Polyline { points: q.iter().map(|q| at(*q)).collect(), closed: false },
        };
        scene.push(style, prim);
    }
}

/// The sheet frame (ISO 5457: 20 mm filing margin on the left, 10 mm elsewhere).
fn frame_box(size: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    ([20.0, 10.0], [size[0] - 10.0, size[1] - 10.0])
}

/// The ISO 7200-style title block in the frame's lower-right corner.
fn title_block(scene: &mut Scene, d: &Drawing, sheet_scale: f64, frame_max_x: f64, frame_min_y: f64) {
    let (w, rh) = (180.0, 8.0);
    let (x0, y0) = (frame_max_x - w, frame_min_y);
    let top = y0 + 5.0 * rh;
    scene.rect(Style::Border, [x0, y0], [frame_max_x, top]);
    for r in 1..4 {
        let y = y0 + r as f64 * rh;
        scene.line(Style::Thin, [x0, y], [frame_max_x, y]);
    }
    let split = x0 + 110.0;
    scene.line(Style::Thin, [split, y0], [split, y0 + 3.0 * rh]);
    let label = |scene: &mut Scene, x: f64, y: f64, t: &str| scene.text([x + 1.5, y + rh - 2.8], 1.8, t, Anchor::Left);
    let value = |scene: &mut Scene, x: f64, y: f64, t: &str| scene.text([x + 18.0, y + 1.8], 3.5, t, Anchor::Left);
    // Title over the two top rows.
    scene.text([x0 + 3.0, y0 + 3.0 * rh + 4.0], 7.0, d.title.clone().unwrap_or_else(|| d.name.clone()), Anchor::Left);
    let rows: [(&str, String, &str, String); 3] = [
        ("Number", d.number.clone().unwrap_or_default(), "Scale", scale_text(sheet_scale)),
        ("Material", d.material.clone().unwrap_or_default(), "Size", d.size.keyword().replace('_', " ")),
        ("Author", d.author.clone().unwrap_or_default(), "Projection", String::new()),
    ];
    for (i, (l1, v1, l2, v2)) in rows.iter().enumerate() {
        let y = y0 + (2 - i) as f64 * rh;
        label(scene, x0, y, l1);
        value(scene, x0, y, v1);
        label(scene, split, y, l2);
        if !v2.is_empty() {
            scene.text([split + 18.0, y + 1.8], 3.5, v2.clone(), Anchor::Left);
        }
    }
    projection_symbol(scene, d.projection, [split + 40.0, y0 + rh / 2.0]);
}

/// The ISO 128-30 projection symbol: a truncated cone and its end view, centred at `c`.
/// The cone's small end is on the left; first angle draws the end view on the right,
/// third angle on the left.
fn projection_symbol(scene: &mut Scene, p: Projection, c: [f64; 2]) {
    let (small, big, len) = (1.5, 3.0, 6.0);
    let (cone_x, circle_x) = match p {
        Projection::First => (c[0] - 7.0, c[0] + 6.0),
        Projection::Third => (c[0] + 1.0, c[0] - 6.0),
    };
    let pts =
        vec![[cone_x, c[1] - small], [cone_x + len, c[1] - big], [cone_x + len, c[1] + big], [cone_x, c[1] + small]];
    scene.push(Style::Visible, Prim::Polyline { points: pts, closed: true });
    scene.line(Style::Center, [cone_x - 1.5, c[1]], [cone_x + len + 1.5, c[1]]);
    for r in [small, big] {
        scene.push(Style::Visible, Prim::Arc { center: [circle_x, c[1]], radius: r, start: 0.0, end: TAU });
    }
    scene.line(Style::Center, [circle_x - big - 1.0, c[1]], [circle_x + big + 1.0, c[1]]);
    scene.line(Style::Center, [circle_x, c[1] - big - 1.0], [circle_x, c[1] + big + 1.0]);
}

/// A drag handle for `item` around the text it drew from item `start` on.
fn handle(scene: &mut Scene, start: usize, item: &str, at: [f64; 2]) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for it in scene.items.get(start..).unwrap_or(&[]) {
        let Prim::Text { at: p, height, text, anchor, angle } = &it.prim else { continue };
        // A rough box: 0.75 of the height per character, the cap height plus a margin.
        let w = text.chars().count() as f64 * height * 0.75;
        let x0 = match anchor {
            Anchor::Left => 0.0,
            Anchor::Center => -w / 2.0,
            Anchor::Right => -w,
        };
        let (s, c) = (libm::sin(*angle), libm::cos(*angle));
        for (x, y) in [(x0, -1.0), (x0 + w, -1.0), (x0, height + 1.0), (x0 + w, height + 1.0)] {
            let q = [p[0] + x * c - y * s, p[1] + x * s + y * c];
            lo = [lo[0].min(q[0]), lo[1].min(q[1])];
            hi = [hi[0].max(q[0]), hi[1].max(q[1])];
        }
    }
    if lo[0].is_finite() {
        scene.handles.push(Handle { item: item.to_string(), at, min: lo, max: hi });
    }
}

/// Builds the scene of drawing `d`. Problems (a view that cannot be projected, an
/// expression that does not evaluate) are listed in `Scene::issues`, and the rest of the
/// sheet is still drawn.
pub fn build(d: &Drawing, cx: &Context<'_>, cache: &mut HlrCache) -> Scene {
    let size = sheet_size(d);
    let mut scene = Scene { size, ..Scene::default() };
    let sheet_scale = match &d.scale {
        Some(e) => match number(e, cx.ev) {
            Ok(s) if (MIN_SCALE..=MAX_SCALE).contains(&s) => s,
            Ok(_) => {
                scene.issues.push("the sheet scale must be between 1:10000 and 10000:1; using 1:1".into());
                1.0
            }
            Err(e) => {
                scene.issues.push(format!("sheet scale: {e}; using 1:1"));
                1.0
            }
        },
        None => 1.0,
    };
    let (fmin, fmax) = frame_box(size);
    scene.rect(Style::Border, fmin, fmax);
    title_block(&mut scene, d, sheet_scale, fmax[0], fmin[1]);

    cache.retain_current(cx.ev);
    let mut b = Builder { d, cx, cache, sheet_scale, placed: BTreeMap::new() };
    for item in &d.items {
        if cx.cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
            scene.issues.push("the drawing was stopped before all views were projected".into());
            break;
        }
        if let DrawingItem::View(v) = item
            && let Err(e) = b.place(&v.name, 0)
        {
            scene.issues.push(e);
        }
    }
    for item in &d.items {
        match item {
            DrawingItem::View(v) => match b.placed.get(v.name.as_str()) {
                Some(Ok((p, pv))) => {
                    let iso = matches!(&v.placement, ViewPlacement::Base { from, .. } if from.is_iso());
                    draw_view(&mut scene, v, p, pv, iso);
                    if let Some((parent, c, r)) = &p.detail {
                        let ctr = |o: [f64; 2], s: f64| [o[0] + s * c[0], o[1] + s * c[1]];
                        scene.circle(Style::Thin, ctr(pv.origin, pv.scale), pv.scale * r);
                        let label = format!("{} ({})", v.name, scale_text(pv.scale));
                        let below = [(pv.min[0] + pv.max[0]) / 2.0, pv.min[1] - 6.0 - section::LABEL_H];
                        scene.text(below, section::LABEL_H, &label, Anchor::Center);
                        if let Some(Ok((_, ppv))) = b.placed.get(parent.as_str()) {
                            let pc = ctr(ppv.origin, ppv.scale);
                            let pr = ppv.scale * r;
                            scene.circle(Style::Thin, pc, pr);
                            let k = std::f64::consts::FRAC_1_SQRT_2;
                            let tp = [pc[0] + (pr + 2.0) * k, pc[1] + (pr + 2.0) * k];
                            scene.text(tp, section::LABEL_H, &v.name, Anchor::Left);
                        }
                    }
                    if let (Some(cut), Some((_, c, r))) = (&p.detail_cut, &p.detail) {
                        let at = |q: [f64; 2]| [pv.origin[0] + pv.scale * q[0], pv.origin[1] + pv.scale * q[1]];
                        let (lines, complete) = section::hatch(&cut.faces, section::HATCH_SPACING / pv.scale);
                        let lines: Vec<_> =
                            lines.into_iter().filter_map(|(a, b)| detail::clip_segment(a, b, *c, *r)).collect();
                        if !complete || lines.len() > MAX_HATCH {
                            scene.issues.push(format!("view {}: the cut is too large to hatch", v.name));
                        }
                        for (a, b) in lines.into_iter().take(MAX_HATCH) {
                            scene.line(Style::Thin, at(a), at(b));
                        }
                    }
                    if v.centers && p.section.is_none() && p.detail.is_none() {
                        centers::draw(&mut scene, &p.bodies, pv);
                    }
                    if let Some(s) = &p.section {
                        let at = |q: [f64; 2]| [pv.origin[0] + pv.scale * q[0], pv.origin[1] + pv.scale * q[1]];
                        let (lines, complete) = section::hatch(&s.cut.faces, section::HATCH_SPACING / pv.scale);
                        if !complete || lines.len() > MAX_HATCH {
                            scene.issues.push(format!("view {}: the cut is too large to hatch", v.name));
                        }
                        for (a, b) in lines.into_iter().take(MAX_HATCH) {
                            scene.line(Style::Thin, at(a), at(b));
                        }
                        let label = format!("{0}–{0}", s.label);
                        let below = [(pv.min[0] + pv.max[0]) / 2.0, pv.min[1] - 6.0 - section::LABEL_H];
                        scene.text(below, section::LABEL_H, &label, Anchor::Center);
                        if let Some(Ok((_, ppv))) = b.placed.get(s.parent.as_str()) {
                            section::cutting_plane(&mut scene, ppv.min, ppv.max, s.vertical, s.pos, s.look, &s.label);
                        }
                    }
                    scene.views.push(pv.clone());
                }
                Some(Err(e)) => {
                    scene.issues.push(e.clone());
                    scene.views.push(PlacedView {
                        name: v.name.clone(),
                        min: [0.0; 2],
                        max: [0.0; 2],
                        scale: 0.0,
                        origin: [0.0; 2],
                        toward: [0.0; 3],
                        x: [0.0; 3],
                        y: [0.0; 3],
                        error: Some(e.clone()),
                    });
                }
                None => {}
            },
            DrawingItem::Note(n) => match (number(&n.at[0], cx.ev), number(&n.at[1], cx.ev)) {
                (Ok(x), Ok(y)) => {
                    let start = scene.items.len();
                    for (i, line) in n.text.lines().enumerate() {
                        scene.text([x, y - i as f64 * 6.0], 3.5, line, Anchor::Left);
                    }
                    handle(&mut scene, start, &n.name, [x, y]);
                }
                (Err(e), _) | (_, Err(e)) => scene.issues.push(format!("note {}: {e}", n.name)),
            },
            DrawingItem::Balloon(bl) => {
                let placed = match b.placed.get(bl.view.as_str()) {
                    Some(Ok((_, pv))) => Ok(pv.clone()),
                    _ => Err(format!("view {} is not shown", bl.view)),
                };
                let r = (|| -> Result<(), String> {
                    let pv = placed?;
                    let at = [number(&bl.at[0], cx.ev)?, number(&bl.at[1], cx.ev)?];
                    let body = cx.ev.body(&bl.body).ok_or_else(|| format!("no body '{}'", bl.body))?;
                    let rows = bom::rows(cx.kernel, cx.ev)?;
                    let item = rows
                        .iter()
                        .find(|r| r.bodies.contains(&bl.body))
                        .map(|r| r.item)
                        .ok_or_else(|| format!("'{}' is not in the bill of materials", bl.body))?;
                    let c = cx.kernel.mass_properties(&body.shape).map_err(|e| e.to_string())?.centroid;
                    let start = scene.items.len();
                    balloon(&mut scene, at, pv.to_sheet(c), item);
                    handle(&mut scene, start, &bl.name, at);
                    Ok(())
                })();
                if let Err(e) = r {
                    scene.issues.push(format!("balloon {}: {e}", bl.name));
                }
            }
            DrawingItem::Gdt(g) => {
                let r = (|| -> Result<(), String> {
                    let Some(Ok((p, pv))) = b.placed.get(g.view.as_str()) else {
                        return Err(format!("view {} is not shown", g.view));
                    };
                    let at = [number(&g.at[0], cx.ev)?, number(&g.at[1], cx.ev)?];
                    let target = pv.to_sheet(dims::edge_anchor(&p.bodies, &g.a)?);
                    let start = scene.items.len();
                    gdt::frame(&mut scene, at, g.symbol, &g.tolerance, &g.datums, target);
                    handle(&mut scene, start, &g.name, at);
                    Ok(())
                })();
                if let Err(e) = r {
                    scene.issues.push(format!("gdt {}: {e}", g.name));
                }
            }
            DrawingItem::Datum(dd) => {
                let r = (|| -> Result<(), String> {
                    let Some(Ok((p, pv))) = b.placed.get(dd.view.as_str()) else {
                        return Err(format!("view {} is not shown", dd.view));
                    };
                    let at = [number(&dd.at[0], cx.ev)?, number(&dd.at[1], cx.ev)?];
                    let target = pv.to_sheet(dims::edge_anchor(&p.bodies, &dd.a)?);
                    let start = scene.items.len();
                    gdt::datum(&mut scene, at, &dd.name, target);
                    handle(&mut scene, start, &dd.name, at);
                    Ok(())
                })();
                if let Err(e) = r {
                    scene.issues.push(format!("datum {}: {e}", dd.name));
                }
            }
            DrawingItem::PartsList(pl) => match (number(&pl.at[0], cx.ev), number(&pl.at[1], cx.ev)) {
                (Ok(x), Ok(y)) => match bom::rows(cx.kernel, cx.ev) {
                    Ok(rows) => {
                        let start = scene.items.len();
                        parts_list(&mut scene, [x, y], &rows);
                        handle(&mut scene, start, &pl.name, [x, y]);
                    }
                    Err(e) => scene.issues.push(format!("parts list {}: {e}", pl.name)),
                },
                (Err(e), _) | (_, Err(e)) => scene.issues.push(format!("parts list {}: {e}", pl.name)),
            },
            DrawingItem::Dim(dim) => {
                let Some(Ok((p, pv))) = b.placed.get(dim.view.as_str()) else {
                    let e = format!("view {} is not shown", dim.view);
                    scene.issues.push(format!("dimension {}: {e}", dim.name));
                    scene.dims.push(dims::DimResult {
                        name: dim.name.clone(),
                        value: None,
                        text: String::new(),
                        error: Some(e),
                        text_at: None,
                    });
                    continue;
                };
                let eval = |e: &Expr| number(e, cx.ev);
                let offset = dim.offset.as_ref().map(eval).transpose();
                let text_at = dim.text_at.as_ref().map(|[x, y]| Ok::<_, String>([eval(x)?, eval(y)?])).transpose();
                let r = match (offset, text_at) {
                    (Ok(offset), Ok(text_at)) => {
                        let v = dims::ViewCtx {
                            bodies: &p.bodies,
                            view: pv,
                            units: d.units.unwrap_or(cx.units),
                            decimals: d.decimals,
                        };
                        let start = scene.items.len();
                        let handles = scene.handles.len();
                        let mut r = dims::draw(&mut scene, dim, &v, offset, text_at);
                        if let Some(at) = r.text_at {
                            handle(&mut scene, start, &dim.name, at);
                        }
                        // Automatic placement steps outwards while its text lands on an
                        // earlier text (dogfooding F5-3: parallel dimensions stacked on one
                        // line). A placement the user chose stays.
                        if offset.is_none() && text_at.is_none() {
                            let mut off = dims::DEFAULT_OFFSET;
                            for _ in 0..MAX_RESTACK {
                                let Some(me) = scene.handles.get(handles) else { break };
                                let hit = scene.handles.get(..handles).unwrap_or(&[]).iter().any(|h| {
                                    h.min[0] < me.max[0]
                                        && me.min[0] < h.max[0]
                                        && h.min[1] < me.max[1]
                                        && me.min[1] < h.max[1]
                                });
                                if !hit {
                                    break;
                                }
                                off += RESTACK_STEP;
                                scene.items.truncate(start);
                                scene.handles.truncate(handles);
                                r = dims::draw(&mut scene, dim, &v, Some(off), None);
                                if let Some(at) = r.text_at {
                                    handle(&mut scene, start, &dim.name, at);
                                }
                            }
                        }
                        r
                    }
                    (Err(e), _) | (_, Err(e)) => dims::DimResult {
                        name: dim.name.clone(),
                        value: None,
                        text: String::new(),
                        error: Some(e),
                        text_at: None,
                    },
                };
                if let Some(e) = &r.error {
                    scene.issues.push(format!("dimension {}: {e}", dim.name));
                }
                scene.dims.push(r);
            }
        }
    }
    layout_issues(&mut scene, d, cx);
    scene
}

/// How far an automatically placed dimension steps out when its text meets another one,
/// sheet mm, and how many steps it tries.
const RESTACK_STEP: f64 = 7.0;
const MAX_RESTACK: usize = 6;

/// Radius of a balloon, sheet mm.
const BALLOON_R: f64 = 5.0;

/// A balloon at `at` with `item` inside and a leader to `target`, ending in a dot.
fn balloon(scene: &mut Scene, at: [f64; 2], target: [f64; 2], item: usize) {
    scene.push(Style::Thin, Prim::Arc { center: at, radius: BALLOON_R, start: 0.0, end: TAU });
    let (dx, dy) = (target[0] - at[0], target[1] - at[1]);
    let len = libm::hypot(dx, dy);
    if len > BALLOON_R {
        let from = [at[0] + dx / len * BALLOON_R, at[1] + dy / len * BALLOON_R];
        scene.line(Style::Thin, from, target);
        scene.push(Style::Thin, Prim::Arc { center: target, radius: 0.6, start: 0.0, end: TAU });
    }
    scene.text([at[0], at[1] - 1.75], 3.5, item.to_string(), Anchor::Center);
}

/// Column widths of a parts list (item, part, quantity), its row height and most rows.
const PARTS_COLS: [f64; 3] = [14.0, 70.0, 14.0];
const PARTS_ROW: f64 = 7.0;
const MAX_PARTS_ROWS: usize = 60;

/// A parts list with its top-left corner at `at`: a header row, then one row per part.
fn parts_list(scene: &mut Scene, at: [f64; 2], rows: &[bom::BomRow]) {
    let shown = rows.len().min(MAX_PARTS_ROWS);
    let w: f64 = PARTS_COLS.iter().sum();
    let bottom = at[1] - PARTS_ROW * (shown + 1) as f64;
    scene.rect(Style::Border, [at[0], bottom], [at[0] + w, at[1]]);
    for i in 1..=shown {
        let y = at[1] - PARTS_ROW * i as f64;
        scene.line(Style::Thin, [at[0], y], [at[0] + w, y]);
    }
    let mut x = at[0];
    for c in PARTS_COLS.iter().take(2) {
        x += c;
        scene.line(Style::Thin, [x, bottom], [x, at[1]]);
    }
    let cell = |scene: &mut Scene, row: usize, col: usize, t: &str| {
        let x0 = at[0] + PARTS_COLS.iter().take(col).sum::<f64>();
        scene.text([x0 + 1.5, at[1] - PARTS_ROW * (row + 1) as f64 + 2.0], 3.0, t, Anchor::Left);
    };
    for (col, h) in ["ITEM", "PART", "QTY"].iter().enumerate() {
        cell(scene, 0, col, h);
    }
    for (i, r) in rows.iter().take(shown).enumerate() {
        let name: String = r.part.chars().take(32).collect();
        cell(scene, i + 1, 0, &r.item.to_string());
        cell(scene, i + 1, 1, &name);
        cell(scene, i + 1, 2, &r.bodies.len().to_string());
    }
    if rows.len() > shown {
        scene.issues.push(format!("the parts list shows {shown} of {} parts", rows.len()));
    }
}

/// A box on the sheet: (min, max) corners, mm.
type SheetBox = ([f64; 2], [f64; 2]);

/// Most layout problems listed per sheet.
const MAX_LAYOUT_ISSUES: usize = 20;

/// Views outside the frame, over the title block or over each other, and notes outside the
/// frame (dogfooding F5-4: these showed only on a rendered sheet).
fn layout_issues(scene: &mut Scene, d: &Drawing, cx: &Context<'_>) {
    let (fmin, fmax) = frame_box(scene.size);
    let title = ([fmax[0] - 180.0, fmin[1]], [fmax[0], fmin[1] + 40.0]);
    let meets = |a: SheetBox, b: SheetBox| {
        a.0[0] < b.1[0] - 0.5 && b.0[0] < a.1[0] - 0.5 && a.0[1] < b.1[1] - 0.5 && b.0[1] < a.1[1] - 0.5
    };
    let inside = |p: [f64; 2]| p[0] >= fmin[0] && p[1] >= fmin[1] && p[0] <= fmax[0] && p[1] <= fmax[1];
    let boxes: Vec<(&str, SheetBox)> =
        scene.views.iter().filter(|v| v.error.is_none()).map(|v| (v.name.as_str(), (v.min, v.max))).collect();
    let mut out = Vec::new();
    for (i, (name, b)) in boxes.iter().enumerate() {
        if !inside(b.0) || !inside(b.1) {
            out.push(format!("view {name} lies outside the frame"));
        } else if meets(*b, title) {
            out.push(format!("view {name} covers the title block"));
        }
        for (other, c) in boxes.iter().skip(i + 1) {
            if meets(*b, *c) {
                out.push(format!("views {name} and {other} overlap"));
            }
        }
    }
    // Dimension and note texts on top of each other (dogfooding F5-3).
    for (i, a) in scene.handles.iter().enumerate() {
        for b in scene.handles.iter().skip(i + 1) {
            if meets((a.min, a.max), (b.min, b.max)) {
                out.push(format!(
                    "the texts of {} and {} overlap: move one (text_at, or the note's at)",
                    a.item, b.item
                ));
            }
        }
    }
    for item in &d.items {
        if let DrawingItem::Note(n) = item
            && let (Ok(x), Ok(y)) = (number(&n.at[0], cx.ev), number(&n.at[1], cx.ev))
            && !inside([x, y])
        {
            out.push(format!("note {} lies outside the frame", n.name));
        }
    }
    scene.issues.extend(out.into_iter().take(MAX_LAYOUT_ISSUES));
}

#[cfg(test)]
mod tests;
