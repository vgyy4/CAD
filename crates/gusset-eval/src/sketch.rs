//! Turning a document sketch into a solved sketch and kernel profiles.

use crate::error::{ErrorCode, EvalError};
use gusset_doc::{SkConstraintKind, SkGeom, SketchDef};
use gusset_kernel::{Frame, Loop2, Profile, ProfileEdge, Region2, Seg2};
use gusset_naming::entity_tag;
use gusset_sketch as sk;
use gusset_units::params::{ParamValues, coerce};
use gusset_units::{Dim, LengthUnit, Quantity};
use std::collections::BTreeMap;

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
    /// Closed regions found (identified by their identifying entity), or why none.
    pub regions: Result<Vec<RegionInfo>, String>,
    /// Where the sketch plane sits in 3D (for drawing and picking in the viewport).
    pub frame: Frame,
    /// Lines (doc id → endpoint ids), circles (id → center id) and arcs (id → center,
    /// start, end ids), for drawing.
    pub lines: Vec<(String, String, String)>,
    pub circles: Vec<(String, String)>,
    pub arcs: Vec<(String, String, String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchStatus {
    WellConstrained,
    UnderConstrained,
    OverConstrained,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionInfo {
    /// The identifying entity: the smallest id on the outer loop.
    pub id: String,
    pub area: f64,
    /// Entities on the outer loop.
    pub outer: Vec<String>,
}

/// A solved sketch plus everything needed to build kernel profiles from it.
#[derive(Debug, Clone)]
pub(crate) struct Solved {
    pub(crate) result: SketchResult,
    pub(crate) profiles: Vec<(RegionInfo, Region2)>,
    /// Where the sketch sits in 3D.
    pub(crate) frame: Frame,
    /// Solved endpoints of every line (doc id → (a, b)), e.g. for revolve axes.
    pub(crate) line_points: BTreeMap<String, ([f64; 2], [f64; 2])>,
}

fn id_key(id: &str) -> u64 {
    entity_tag(id).unwrap_or(u64::MAX)
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
    };
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
    let mut profiles = Vec::new();
    match sk::profiles(&r.sketch, 1e-6) {
        Ok(ps) => {
            let seg_of = |seg: &sk::Segment| -> ProfileEdge {
                let tag = back.get(&seg.entity).and_then(|d| entity_tag(d)).unwrap_or(u64::MAX);
                let seg2 = match seg.curve {
                    sk::Curve::Line { start, end } => Seg2::Line { a: start, b: end },
                    sk::Curve::Arc { center, start, end, ccw, .. } => Seg2::Arc { center, start, end, ccw },
                    sk::Curve::Circle { center, radius, .. } => Seg2::Circle { center, radius },
                };
                ProfileEdge { seg: seg2, tag }
            };
            for p in &ps {
                let mut outer: Vec<String> =
                    p.outer.entities().iter().filter_map(|e| back.get(e).map(|s| (*s).to_string())).collect();
                outer.sort_by_key(|i| id_key(i));
                outer.dedup();
                let id = outer.first().cloned().unwrap_or_default();
                let region = Region2 {
                    outer: Loop2 { edges: p.outer.segments.iter().map(seg_of).collect() },
                    holes: p.holes.iter().map(|h| Loop2 { edges: h.segments.iter().map(seg_of).collect() }).collect(),
                };
                profiles.push((RegionInfo { id, area: p.area, outer }, region));
            }
            // Deterministic region order: by identifying entity.
            profiles.sort_by_key(|(info, _)| id_key(&info.id));
            result.regions = Ok(profiles.iter().map(|(i, _)| i.clone()).collect());
        }
        Err(e) => {
            let msg = match &e {
                sk::ProfileError::Open { entities } => {
                    format!("the profile is not closed (open at {})", names(entities).join(", "))
                }
                sk::ProfileError::SelfIntersecting { a, b, .. } => {
                    format!("{} and {} cross each other", names(&[*a]).join(""), names(&[*b]).join(""))
                }
                other => other.to_string(),
            };
            result.regions = Err(msg);
        }
    }
    Ok(Solved { result, profiles, frame, line_points })
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
    let mut regions = Vec::new();
    let mut ids = Vec::new();
    for (info, region) in &solved.profiles {
        if select.is_empty() || select.iter().any(|s| info.outer.contains(s)) {
            regions.push(region.clone());
            ids.push(info.id.clone());
        }
    }
    for s in select {
        if !solved.profiles.iter().any(|(i, _)| i.outer.contains(s)) {
            return Err(EvalError::new(
                feature,
                ErrorCode::InvalidReference,
                format!("{sketch_name} has no closed region bounded by '{s}'"),
            ));
        }
    }
    if regions.is_empty() {
        return Err(EvalError::new(feature, ErrorCode::OpenProfile, format!("{sketch_name} has no closed regions"))
            .with_hint("draw a closed outline"));
    }
    Ok((Profile { frame, regions }, ids))
}
