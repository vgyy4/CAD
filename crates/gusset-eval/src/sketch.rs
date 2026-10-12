//! Turning a document sketch into a solved sketch and kernel profiles.

use crate::error::{ErrorCode, EvalError};
use crate::pattern;
use gusset_doc::{SkConstraintKind, SkGeom, SketchDef};
use gusset_kernel::{Frame, Loop2, Profile, ProfileEdge, Region2, Seg2};
use gusset_naming::entity_tag;
use gusset_sketch as sk;
use gusset_units::params::{ParamValues, coerce};
use gusset_units::{Dim, LengthUnit, Quantity};
use std::collections::{BTreeMap, BTreeSet};

/// Result of solving one sketch, exposed to the UI and MCP (`/sketch/{name}/status`).
#[derive(Debug, Clone, PartialEq)]
pub struct SketchResult {
    pub status: SketchStatus,
    pub dof: u32,
    /// Solved point positions (doc id → mm).
    pub points: BTreeMap<String, [f64; 2]>,
    /// Solved circle radii (doc id → mm).
    pub radii: BTreeMap<String, f64>,
    pub free_entities: Vec<String>,
    pub redundant: Vec<String>,
    pub conflicting: Vec<String>,
    /// Closed regions found (see [`RegionInfo::id`] for how they are named), or why none.
    pub regions: Result<Vec<RegionInfo>, String>,
    /// Entities with a free end whose dangling pieces were ignored (they bound no region).
    pub open_ends: Vec<String>,
    /// Where the sketch plane sits in 3D (for drawing and picking in the viewport).
    pub frame: Frame,
    /// Lines (doc id → endpoint ids), circles (id → center id) and arcs (id → center,
    /// start, end ids), for drawing.
    pub lines: Vec<(String, String, String)>,
    pub circles: Vec<(String, String)>,
    pub arcs: Vec<(String, String, String, String)>,
    /// Splines (id → points along the curve in sketch mm), for drawing.
    pub splines: Vec<(String, Vec<[f64; 2]>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchStatus {
    WellConstrained,
    UnderConstrained,
    OverConstrained,
    Failed,
}

/// A bounded face of the sketch's curve arrangement.
///
/// Naming (ADR 0016): the region's *primary entity* is the entity with the smallest id on
/// its outer loop. When no curve of the sketch is cut by another one, the id is just that
/// entity (the classic one-region-per-outline case, unchanged from earlier versions). When
/// curves cross, several regions can share a primary entity: they are ordered by their
/// boundary pieces (entity and piece number along the entity, which only change when the
/// crossing structure changes) and the first keeps the plain id, the others get
/// `<entity>_r1`, `<entity>_r2`, ... Region ids do not change when only dimensions or
/// positions are edited.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionInfo {
    pub id: String,
    pub area: f64,
    /// Entities on the outer loop.
    pub outer: Vec<String>,
    /// A point inside the region (sketch coordinates, mm), to recognise it in a picture.
    pub at: [f64; 2],
}

/// A solved sketch plus everything needed to build kernel profiles from it.
#[derive(Debug, Clone)]
pub(crate) struct Solved {
    pub(crate) result: SketchResult,
    pub(crate) profiles: Vec<(RegionInfo, Region2)>,
    /// Some curve is cut by another one (regions are then named with ranks).
    pub(crate) crossings: bool,
    /// Where the sketch sits in 3D.
    pub(crate) frame: Frame,
    /// Solved endpoints of every line (doc id → (a, b)), e.g. for revolve axes.
    pub(crate) line_points: BTreeMap<String, ([f64; 2], [f64; 2])>,
}

fn id_key(id: &str) -> u64 {
    entity_tag(id).unwrap_or(u64::MAX)
}

/// A document sketch translated to a solver sketch, with the id maps in both directions.
struct Built<'a> {
    s: sk::Sketch,
    ids: BTreeMap<&'a str, sk::EntityId>,
    back: BTreeMap<sk::EntityId, &'a str>,
    cback: BTreeMap<sk::ConstraintId, &'a str>,
}

fn build<'a>(name: &str, def: &'a SketchDef, params: &ParamValues, units: LengthUnit) -> Result<Built<'a>, EvalError> {
    let err = |code, msg: String| EvalError::new(name, code, msg);
    let mut s = sk::Sketch::new();
    let mut ids: BTreeMap<&str, sk::EntityId> = BTreeMap::new();
    let mut back: BTreeMap<sk::EntityId, &str> = BTreeMap::new();
    let get = |ids: &BTreeMap<&str, sk::EntityId>, r: &str| -> Result<sk::EntityId, EvalError> {
        ids.get(r)
            .copied()
            .ok_or_else(|| err(ErrorCode::InvalidReference, format!("sketch refers to unknown entity '{r}'")))
    };
    for e in &def.entities {
        let id = match &e.geom {
            SkGeom::Point { x, y } => s.add_point([*x, *y]),
            SkGeom::Line { a, b } => s.add_line_between(get(&ids, a)?, get(&ids, b)?),
            SkGeom::Circle { center, radius } => s.add_circle_at(get(&ids, center)?, *radius),
            SkGeom::Arc { center, start, end } => {
                s.add_arc_between(get(&ids, center)?, get(&ids, start)?, get(&ids, end)?)
            }
            SkGeom::Spline { points, closed } => {
                let fit = points.iter().map(|p| get(&ids, p)).collect::<Result<Vec<_>, _>>()?;
                s.add_spline(&fit, *closed)
            }
        }
        .map_err(|e2| err(ErrorCode::InvalidSketch, format!("entity '{}': {e2}", e.id)))?;
        if e.construction {
            s.set_construction(id, true).map_err(|e2| err(ErrorCode::InvalidSketch, e2.to_string()))?;
        }
        ids.insert(&e.id, id);
        back.insert(id, &e.id);
    }
    let mut cback: BTreeMap<sk::ConstraintId, &str> = BTreeMap::new();
    for c in &def.constraints {
        let ent = |i: usize| -> Result<sk::EntityId, EvalError> {
            get(&ids, c.entities.get(i).map(String::as_str).unwrap_or(""))
        };
        let value = |dim: Dim| -> Result<f64, EvalError> {
            let Some(expr) = &c.value else {
                return Err(err(ErrorCode::InvalidSketch, format!("constraint '{}' needs a value", c.id)));
            };
            let scope = |n: &str| params.get(n).and_then(|r| r.as_ref().ok().copied());
            let q: Quantity = gusset_units::expr::eval(expr, &scope)
                .map_err(|e| err(ErrorCode::Expression, format!("constraint '{}': {e}", c.id)))?;
            let q =
                coerce(q, dim, units).map_err(|m| err(ErrorCode::Expression, format!("constraint '{}': {m}", c.id)))?;
            Ok(q.value)
        };
        use SkConstraintKind as K;
        let con = match c.kind {
            K::Coincident => sk::Constraint::Coincident(ent(0)?, ent(1)?),
            K::PointOnLine => sk::Constraint::PointOnLine(ent(0)?, ent(1)?),
            K::PointOnCircle => sk::Constraint::PointOnCircle(ent(0)?, ent(1)?),
            K::Midpoint => sk::Constraint::Midpoint(ent(0)?, ent(1)?),
            K::Horizontal => sk::Constraint::Horizontal(ent(0)?),
            K::Vertical => sk::Constraint::Vertical(ent(0)?),
            K::Parallel => sk::Constraint::Parallel(ent(0)?, ent(1)?),
            K::Perpendicular => sk::Constraint::Perpendicular(ent(0)?, ent(1)?),
            K::Tangent => sk::Constraint::Tangent(ent(0)?, ent(1)?),
            K::Equal => sk::Constraint::Equal(ent(0)?, ent(1)?),
            K::Distance => sk::Constraint::Distance { a: ent(0)?, b: ent(1)?, value: value(Dim::LENGTH)? },
            K::Angle => sk::Constraint::Angle { a: ent(0)?, b: ent(1)?, value: value(Dim::ANGLE)? },
            K::Radius => sk::Constraint::Radius { c: ent(0)?, value: value(Dim::LENGTH)? },
            K::Diameter => sk::Constraint::Diameter { c: ent(0)?, value: value(Dim::LENGTH)? },
            K::Fixed => sk::Constraint::Fixed(ent(0)?),
            K::Symmetric => sk::Constraint::Symmetric { a: ent(0)?, b: ent(1)?, axis: ent(2)? },
        };
        let cid =
            s.add_constraint(con).map_err(|e| err(ErrorCode::InvalidSketch, format!("constraint '{}': {e}", c.id)))?;
        cback.insert(cid, &c.id);
    }

    Ok(Built { s, ids, back, cback })
}

/// Solves `def` (feature `name`) with parameter values `params`.
pub(crate) fn solve_sketch(
    name: &str,
    def: &SketchDef,
    params: &ParamValues,
    units: LengthUnit,
    frame: Frame,
) -> Result<Solved, EvalError> {
    let err = |code, msg: String| EvalError::new(name, code, msg);
    let Built { s, back, cback, .. } = build(name, def, params, units)?;
    let r = sk::solve(&s, &sk::SolveOptions::default());
    let names = |v: &[sk::EntityId]| v.iter().filter_map(|i| back.get(i).map(|s| (*s).to_string())).collect::<Vec<_>>();
    let cnames =
        |v: &[sk::ConstraintId]| v.iter().filter_map(|i| cback.get(i).map(|s| (*s).to_string())).collect::<Vec<_>>();
    let status = match r.status {
        sk::SolveStatus::WellConstrained => SketchStatus::WellConstrained,
        sk::SolveStatus::UnderConstrained => SketchStatus::UnderConstrained,
        sk::SolveStatus::OverConstrained => SketchStatus::OverConstrained,
        sk::SolveStatus::Failed => SketchStatus::Failed,
    };
    let mut points = BTreeMap::new();
    let mut radii = BTreeMap::new();
    for e in r.sketch.entities() {
        let Some(doc_id) = back.get(&e.id) else { continue };
        match e.geom {
            sk::Geom::Point { p } => {
                points.insert((*doc_id).to_string(), p);
            }
            sk::Geom::Circle { radius, .. } => {
                radii.insert((*doc_id).to_string(), radius);
            }
            _ => {}
        }
    }
    let mut result = SketchResult {
        status,
        dof: r.dof,
        points,
        radii,
        free_entities: names(&r.free_entities),
        redundant: cnames(&r.redundant),
        conflicting: cnames(&r.conflicting),
        regions: Ok(Vec::new()),
        open_ends: Vec::new(),
        frame,
        lines: def
            .entities
            .iter()
            .filter_map(|e| match &e.geom {
                SkGeom::Line { a, b } => Some((e.id.clone(), a.clone(), b.clone())),
                _ => None,
            })
            .collect(),
        circles: def
            .entities
            .iter()
            .filter_map(|e| match &e.geom {
                SkGeom::Circle { center, .. } => Some((e.id.clone(), center.clone())),
                _ => None,
            })
            .collect(),
        arcs: def
            .entities
            .iter()
            .filter_map(|e| match &e.geom {
                SkGeom::Arc { center, start, end } => Some((e.id.clone(), center.clone(), start.clone(), end.clone())),
                _ => None,
            })
            .collect(),
        splines: Vec::new(),
    };
    result.splines = def
        .entities
        .iter()
        .filter_map(|e| match &e.geom {
            SkGeom::Spline { points, closed } => {
                let fit: Option<Vec<[f64; 2]>> = points.iter().map(|p| result.points.get(p).copied()).collect();
                fit.map(|f| (e.id.clone(), sk::spline_polyline(&f, *closed, 16)))
            }
            _ => None,
        })
        .collect();
    if status == SketchStatus::Failed {
        let why = r.error.map_or_else(|| "the solver did not converge".to_string(), |e| e.to_string());
        result.regions = Err(why.clone());
        return Err(err(ErrorCode::SketchSolve, format!("sketch could not be solved: {why}"))
            .with_hint("remove or relax a constraint, or move geometry closer to the intended shape"));
    }
    if !result.conflicting.is_empty() {
        return Err(err(
            ErrorCode::SketchConflict,
            format!("constraints {} conflict and cannot all be satisfied", result.conflicting.join(", ")),
        )
        .with_hint("delete one of the conflicting constraints or change its value"));
    }

    let mut line_points = BTreeMap::new();
    for e in &def.entities {
        if let SkGeom::Line { a, b } = &e.geom
            && let (Some(pa), Some(pb)) = (result.points.get(a), result.points.get(b))
        {
            line_points.insert(e.id.clone(), (*pa, *pb));
        }
    }
    // Pattern copies: derived from the solved sources (ADR 0016 §4), part of the arrangement.
    let derived = pattern::derive(name, def, params, units, &result.points, &result.radii)?;
    let mut arr_sketch = r.sketch.clone();
    let mut dback: BTreeMap<sk::EntityId, String> = back.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
    let mut dids: BTreeMap<String, sk::EntityId> = BTreeMap::new();
    let bad = |what: &str, e: sk::SketchError| err(ErrorCode::InvalidSketch, format!("pattern copy {what}: {e}"));
    for (id, pos) in &derived.points {
        let e = arr_sketch.add_point(*pos).map_err(|e| bad(id, e))?;
        dids.insert(id.clone(), e);
        dback.insert(e, id.clone());
        result.points.insert(id.clone(), *pos);
    }
    for c in &derived.curves {
        let get = |dids: &BTreeMap<String, sk::EntityId>, p: &String| {
            dids.get(p)
                .copied()
                .ok_or_else(|| err(ErrorCode::InvalidSketch, format!("pattern copy {}: no point {p}", c.id)))
        };
        let added = match &c.geom {
            SkGeom::Line { a, b } => {
                result.lines.push((c.id.clone(), a.clone(), b.clone()));
                arr_sketch.add_line_between(get(&dids, a)?, get(&dids, b)?)
            }
            SkGeom::Circle { center, radius } => {
                result.circles.push((c.id.clone(), center.clone()));
                result.radii.insert(c.id.clone(), *radius);
                arr_sketch.add_circle_at(get(&dids, center)?, *radius)
            }
            SkGeom::Arc { center, start, end } => {
                result.arcs.push((c.id.clone(), center.clone(), start.clone(), end.clone()));
                arr_sketch.add_arc_between(get(&dids, center)?, get(&dids, start)?, get(&dids, end)?)
            }
            SkGeom::Spline { points, closed } => {
                let fit = points.iter().map(|p| get(&dids, p)).collect::<Result<Vec<_>, _>>()?;
                let pts: Vec<[f64; 2]> = points.iter().filter_map(|p| result.points.get(p).copied()).collect();
                result.splines.push((c.id.clone(), sk::spline_polyline(&pts, *closed, 16)));
                arr_sketch.add_spline(&fit, *closed)
            }
            SkGeom::Point { .. } => continue,
        }
        .map_err(|e| bad(&c.id, e))?;
        if c.construction {
            arr_sketch.set_construction(added, true).map_err(|e| bad(&c.id, e))?;
        }
        dback.insert(added, c.id.clone());
    }
    let dnames = |v: &[sk::EntityId]| v.iter().filter_map(|i| dback.get(i).cloned()).collect::<Vec<_>>();
    let mut profiles = Vec::new();
    let mut crossings = false;
    match sk::arrange(&arr_sketch, 1e-6) {
        Ok(arr) if arr.profiles.is_empty() && !arr.open_ends.is_empty() => {
            result.open_ends = dnames(&arr.open_ends);
            result.regions = Err(format!("the profile is not closed (open at {})", result.open_ends.join(", ")));
        }
        Ok(arr) => {
            crossings = arr.crossings;
            result.open_ends = dnames(&arr.open_ends);
            let seg_of = |seg: &sk::Segment| -> ProfileEdge {
                let tag = dback.get(&seg.entity).and_then(|d| entity_tag(d)).unwrap_or(u64::MAX);
                let tag = if matches!(seg.curve, sk::Curve::Bezier { .. }) {
                    gusset_naming::with_segment(tag, seg.sub)
                } else {
                    tag
                };
                let seg2 = match seg.curve {
                    sk::Curve::Line { start, end } => Seg2::Line { a: start, b: end },
                    sk::Curve::Arc { center, start, end, ccw, .. } => Seg2::Arc { center, start, end, ccw },
                    sk::Curve::Circle { center, radius, .. } => Seg2::Circle { center, radius },
                    sk::Curve::Bezier { p } => Seg2::Bezier { p },
                };
                ProfileEdge { seg: seg2, tag }
            };
            // (primary entity, ranking key within it, profile index)
            type Rank = Vec<(u64, u32)>;
            let mut order: Vec<(String, Rank, usize)> = Vec::new();
            let mut outers: Vec<Vec<String>> = Vec::new();
            for (pi, p) in arr.profiles.iter().enumerate() {
                let mut outer: Vec<String> = p.outer.entities().iter().filter_map(|e| dback.get(e).cloned()).collect();
                outer.sort_by_key(|i| id_key(i));
                outer.dedup();
                let primary = outer.first().cloned().unwrap_or_default();
                let mut rank: Vec<(u64, u32)> =
                    p.pieces.iter().map(|(e, piece)| (dback.get(e).map_or(u64::MAX, |d| id_key(d)), *piece)).collect();
                rank.sort_unstable();
                order.push((primary, rank, pi));
                outers.push(outer);
            }
            order.sort_by(|a, b| id_key(&a.0).cmp(&id_key(&b.0)).then_with(|| a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
            let mut seen: BTreeMap<String, usize> = BTreeMap::new();
            for (primary, _, pi) in order {
                let (Some(p), Some(outer)) = (arr.profiles.get(pi), outers.get(pi)) else { continue };
                let n = seen.entry(primary.clone()).or_insert(0);
                let id = if *n == 0 || !crossings { primary.clone() } else { format!("{primary}_r{n}") };
                *n += 1;
                let region = Region2 {
                    outer: Loop2 { edges: p.outer.segments.iter().map(seg_of).collect() },
                    holes: p.holes.iter().map(|h| Loop2 { edges: h.segments.iter().map(seg_of).collect() }).collect(),
                };
                profiles.push((RegionInfo { id, area: p.area, outer: outer.clone(), at: p.label }, region));
            }
            result.regions = Ok(profiles.iter().map(|(i, _)| i.clone()).collect());
        }
        Err(e) => {
            let msg = match &e {
                sk::ProfileError::Open { entities } => {
                    format!("the profile is not closed (open at {})", dnames(entities).join(", "))
                }
                sk::ProfileError::SelfIntersecting { a, b, .. } => {
                    format!("{} and {} overlap along a stretch", dnames(&[*a]).join(""), dnames(&[*b]).join(""))
                }
                other => other.to_string(),
            };
            result.regions = Err(msg);
        }
    }
    Ok(Solved { result, profiles, crossings, frame, line_points })
}

/// Builds the kernel profile for the selected regions (empty selection = all).
pub(crate) fn kernel_profile(
    feature: &str,
    sketch_name: &str,
    solved: &Solved,
    frame: Frame,
    select: &[String],
) -> Result<(Profile, Vec<String>), EvalError> {
    if let Err(why) = &solved.result.regions {
        return Err(EvalError::new(feature, ErrorCode::OpenProfile, format!("{sketch_name}: {why}"))
            .with_hint("close the outline so it forms a region"));
    }
    // A token is a region id (`l5`, `l5_r1`) or an entity on the outer boundary. In a sketch
    // without crossings an entity selects every region it bounds (the classic behaviour); with
    // crossings an exact region id wins, and an entity that bounds several regions is ambiguous.
    let mut picked: BTreeSet<usize> = BTreeSet::new();
    for tok in select {
        let infos = &solved.profiles;
        let by_entity: Vec<usize> =
            infos.iter().enumerate().filter(|(_, (i, _))| i.outer.contains(tok)).map(|(k, _)| k).collect();
        let exact = infos.iter().position(|(i, _)| &i.id == tok);
        match (solved.crossings, exact) {
            (true, Some(k)) => {
                picked.insert(k);
            }
            (true, None) if by_entity.len() > 1 => {
                let ids: Vec<&str> =
                    by_entity.iter().filter_map(|&k| infos.get(k)).map(|(i, _)| i.id.as_str()).collect();
                return Err(EvalError::new(
                    feature,
                    ErrorCode::InvalidReference,
                    format!("'{tok}' bounds several regions of {sketch_name}: {}", ids.join(", ")),
                )
                .with_hint("name one region (query_sketch lists region ids)"));
            }
            _ if !by_entity.is_empty() => picked.extend(by_entity),
            _ => {
                let ids: Vec<&str> = infos.iter().map(|(i, _)| i.id.as_str()).collect();
                return Err(EvalError::new(
                    feature,
                    ErrorCode::InvalidReference,
                    format!("{sketch_name} has no closed region '{tok}'"),
                )
                .with_hint(format!("regions now: {}", ids.join(", "))));
            }
        }
    }
    let mut regions = Vec::new();
    let mut ids = Vec::new();
    for (k, (info, region)) in solved.profiles.iter().enumerate() {
        if select.is_empty() || picked.contains(&k) {
            regions.push(region.clone());
            ids.push(info.id.clone());
        }
    }
    if regions.is_empty() {
        return Err(EvalError::new(feature, ErrorCode::OpenProfile, format!("{sketch_name} has no closed regions"))
            .with_hint("draw a closed outline"));
    }
    Ok((Profile { frame, regions }, ids))
}

/// What a cut can be tied to: used to keep a trimmed end on the curve that cut it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartnerKind {
    Line,
    /// A circle or arc.
    Circle,
    /// A curve the solver has no point-on-curve constraint for.
    Other,
}

/// One cut of an entity, in document ids.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimCut {
    /// Where the other curve meets the entity (sketch coordinates, mm).
    pub pos: [f64; 2],
    /// The other curve's id and kind.
    pub partner: String,
    pub partner_kind: PartnerKind,
    /// An existing point at this place, to use as the new end instead of a fresh point.
    pub at_point: Option<String>,
}

/// The piece of an entity that a pick selects, and the cuts around it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimPlan {
    /// The picked piece runs from `lo` to `hi` along the entity (counter-clockwise for
    /// circles and arcs; for a circle it may wrap around). `None` is the entity's own end;
    /// both `None` means the whole entity is the piece.
    pub lo: Option<TrimCut>,
    pub hi: Option<TrimCut>,
    /// Number of cuts the entity has in total (for reporting).
    pub cut_count: usize,
}

/// Finds the piece of `entity` nearest to `at` (sketch coordinates, mm) between the cuts
/// made by the other curves of the sketch. Solves the sketch first.
pub fn trim_plan(
    name: &str,
    def: &SketchDef,
    params: &ParamValues,
    units: LengthUnit,
    entity: &str,
    at: [f64; 2],
) -> Result<TrimPlan, EvalError> {
    let err = |code, msg: String| EvalError::new(name, code, msg);
    let Built { s, ids, back, .. } = build(name, def, params, units)?;
    let Some(&eid) = ids.get(entity) else {
        return Err(err(ErrorCode::InvalidReference, format!("sketch has no entity '{entity}'")));
    };
    let wrong = || {
        err(ErrorCode::InvalidReference, format!("'{entity}' cannot be trimmed: it is not a line, arc or circle"))
            .with_hint("pick a line, arc or circle that is not construction geometry (splines cannot be trimmed)")
    };
    let solved = sk::solve(&s, &sk::SolveOptions::default());
    if solved.status == sk::SolveStatus::Failed {
        return Err(err(ErrorCode::SketchSolve, "the sketch could not be solved".to_string()));
    }
    match solved.sketch.entity(eid).map(|e| e.geom) {
        Some(sk::Geom::Line { .. } | sk::Geom::Arc { .. } | sk::Geom::Circle { .. }) => {}
        _ => return Err(wrong()),
    }
    if solved.sketch.entity(eid).is_some_and(|e| e.construction) {
        return Err(wrong());
    }
    let c = sk::cuts(&solved.sketch, eid, at, 1e-6).map_err(|e| match e {
        sk::ProfileError::SelfIntersecting { a, b, .. } => err(
            ErrorCode::InvalidSketch,
            format!(
                "{} and {} overlap along a stretch, so the cut points are not defined",
                back.get(&a).copied().unwrap_or("?"),
                back.get(&b).copied().unwrap_or("?")
            ),
        )
        .with_hint("delete one of the overlapping curves first"),
        other => err(ErrorCode::InvalidSketch, other.to_string()),
    })?;
    let kind_of = |id: &str| match def.entities.iter().find(|e| e.id == id).map(|e| &e.geom) {
        Some(SkGeom::Line { .. }) => PartnerKind::Line,
        Some(SkGeom::Circle { .. } | SkGeom::Arc { .. }) => PartnerKind::Circle,
        _ => PartnerKind::Other,
    };
    let cuts: Vec<TrimCut> = c
        .cuts
        .iter()
        .map(|k| {
            let partner = back.get(&k.partner).copied().unwrap_or("").to_string();
            TrimCut {
                pos: k.pos,
                partner_kind: kind_of(&partner),
                partner,
                at_point: k.at_point.and_then(|p| back.get(&p).map(|s| (*s).to_string())),
            }
        })
        .collect();
    let is_circle = matches!(solved.sketch.entity(eid).map(|e| e.geom), Some(sk::Geom::Circle { .. }));
    let n = cuts.len();
    let (lo, hi) = if is_circle {
        if n < 2 {
            (None, None)
        } else {
            // Cyclic: the piece containing the pick runs from the last cut at or before it
            // to the next one, wrapping around.
            let below = c.cuts.iter().rposition(|k| k.param <= c.pick);
            let i = below.unwrap_or(n - 1);
            (cuts.get(i).cloned(), cuts.get((i + 1) % n).cloned())
        }
    } else {
        let below = c.cuts.iter().rposition(|k| k.param <= c.pick);
        let above = c.cuts.iter().position(|k| k.param > c.pick);
        (below.and_then(|i| cuts.get(i).cloned()), above.and_then(|i| cuts.get(i).cloned()))
    };
    Ok(TrimPlan { lo, hi, cut_count: n })
}
