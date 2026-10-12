//! Sketch editing beyond adding single items: trim (v0.2, ADR 0016).

use super::*;
use gusset_eval::{PartnerKind, TrimCut, TrimPlan};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<SketchTrim>(), spec::<SketchPattern>()]
}

/// Deletes entities and everything that depends on them. Returns the deleted ids (the
/// requested ones plus the geometry built on them) and how many constraints went with them.
pub(super) fn delete_with_dependents(d: &mut SketchDef, ids: &[String]) -> (Vec<String>, Vec<String>) {
    let mut dead: Vec<String> = ids.to_vec();
    loop {
        let more: Vec<String> = d
            .entities
            .iter()
            .filter(|e| !dead.contains(&e.id) && e.geom.refs().iter().any(|r| dead.iter().any(|x| x == r)))
            .map(|e| e.id.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        dead.extend(more);
    }
    d.entities.retain(|e| !dead.contains(&e.id));
    // Patterns lose deleted sources; a pattern without sources or centre goes away.
    for p in &mut d.patterns {
        p.of.retain(|o| !dead.contains(o));
    }
    d.patterns.retain(|p| {
        let centre_gone = matches!(&p.kind, SkPatternKind::Circular { center, .. } if dead.contains(center));
        !(p.of.is_empty() || centre_gone || dead.contains(&p.id))
    });
    let mut removed = Vec::new();
    d.constraints.retain(|c| {
        let gone = dead.contains(&c.id) || c.entities.iter().any(|e| dead.contains(e));
        if gone {
            removed.push(c.id.clone());
        }
        !gone
    });
    (dead, removed)
}

/// Splits an entity at its intersections and deletes the picked piece.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SketchTrim {
    pub sketch: String,
    /// Line, arc or circle id.
    pub entity: String,
    /// Pick point (sketch mm) on the piece to delete.
    pub x: f64,
    pub y: f64,
}

impl Command for SketchTrim {
    const ID: &'static str = "sketch.trim";
    const TITLE: &'static str = "Trim";
    const DESCRIPTION: &'static str = "Delete the piece of a line/arc/circle at (x, y) between crossing curves. New ends sit on the crossing curve or reuse a point there; a circle becomes an arc, a line cut mid-way two lines. A trimmed line loses length/midpoint/equal constraints. One undo step.";
    const CATEGORY: Category = Category::Sketch;
    const FLAGS: Flags = EDIT;
    const SHORTCUT: Option<&'static str> = Some("T");
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !(self.x.is_finite() && self.y.is_finite()) {
            return Err(CommandError::args("x and y must be finite numbers"));
        }
        let sk = self.sketch.clone();
        let out = edit(s, ctx, "Trim", |doc, _| {
            let values = params::evaluate(&doc.params);
            let units = doc.units;
            let d = sketch_mut(doc, &sk)?;
            if !d.entities.iter().any(|e| e.id == self.entity) {
                return Err(CommandError::not_found(format!("no entity '{}' in {sk}", self.entity)));
            }
            let plan = gusset_eval::trim_plan(&sk, d, &values, units, &self.entity, [self.x, self.y]).map_err(|e| {
                let mut ce = CommandError::new("invalid_sketch", e.message);
                ce.hint = e.hint;
                ce
            })?;
            let report = apply_trim(d, &self.entity, &plan)?;
            store_solution(doc, &sk);
            Ok(report)
        })?;
        let status = sketch_status(s, ctx, &sk, false)?;
        Ok(merge(out, json!({ "sketch_status": status })))
    }
}

/// Everything `apply_trim` changed, for the command result.
#[derive(Default)]
struct Report {
    /// Entities that now cover what is left of the trimmed one.
    kept: Vec<String>,
    created: Vec<String>,
    deleted: Vec<String>,
    constraints_added: Vec<String>,
    constraints_dropped: Vec<String>,
}

fn geom_of<'a>(d: &'a SketchDef, id: &str) -> Option<&'a SkGeom> {
    d.entities.iter().find(|e| e.id == id).map(|e| &e.geom)
}

fn is_used(d: &SketchDef, point: &str) -> bool {
    d.entities.iter().any(|e| e.geom.refs().contains(&point))
        || d.patterns.iter().any(|p| matches!(&p.kind, SkPatternKind::Circular { center, .. } if center == point))
}

/// The point that ends a remaining piece at `cut`: an existing point if one is there,
/// otherwise a new one tied to the curve that cuts.
fn cut_point(d: &mut SketchDef, cut: &TrimCut, rep: &mut Report) -> Result<String, CommandError> {
    if let Some(p) = &cut.at_point {
        return Ok(p.clone());
    }
    let id = add_point(d, cut.pos[0], cut.pos[1])?;
    rep.created.push(id.clone());
    let kind = match cut.partner_kind {
        PartnerKind::Line => Some(SkConstraintKind::PointOnLine),
        PartnerKind::Circle => Some(SkConstraintKind::PointOnCircle),
        PartnerKind::Other => None,
    };
    if let Some(k) = kind {
        let c = add_constraint(d, k, &[&id, &cut.partner], None);
        rep.constraints_added.push(c);
    }
    Ok(id)
}

/// Removes a point that nothing uses any more, with the constraints on it.
fn drop_orphan_point(d: &mut SketchDef, point: &str, rep: &mut Report) {
    if is_used(d, point) || !matches!(geom_of(d, point), Some(SkGeom::Point { .. })) {
        return;
    }
    d.entities.retain(|e| e.id != point);
    d.constraints.retain(|c| {
        let gone = c.entities.iter().any(|e| e == point);
        if gone {
            rep.constraints_dropped.push(c.id.clone());
        }
        !gone
    });
    rep.deleted.push(point.to_string());
}

fn set_geom(d: &mut SketchDef, id: &str, g: SkGeom) {
    if let Some(e) = d.entities.iter_mut().find(|e| e.id == id) {
        e.geom = g;
    }
    define_points_first(d, id);
}

/// The document requires a curve to come after the points it refers to: move any of its
/// points that were defined later to just before it (points depend on nothing).
fn define_points_first(d: &mut SketchDef, id: &str) {
    let Some(at) = d.entities.iter().position(|e| e.id == id) else { return };
    let refs: Vec<String> =
        d.entities.get(at).map(|e| e.geom.refs().iter().map(|r| (*r).to_string()).collect()).unwrap_or_default();
    let mut at = at;
    for r in refs {
        if let Some(pos) = d.entities.iter().position(|e| e.id == r)
            && pos > at
        {
            let p = d.entities.remove(pos);
            d.entities.insert(at, p);
            at += 1;
        }
    }
}

fn apply_trim(d: &mut SketchDef, id: &str, plan: &TrimPlan) -> Result<Value, CommandError> {
    let mut rep = Report::default();
    let geom = geom_of(d, id).cloned().ok_or_else(|| CommandError::not_found(format!("no entity '{id}'")))?;
    let construction = d.entities.iter().find(|e| e.id == id).is_some_and(|e| e.construction);
    match (&geom, &plan.lo, &plan.hi) {
        (SkGeom::Point { .. }, _, _) => return Err(CommandError::args(format!("'{id}' is a point, not a curve"))),
        (SkGeom::Spline { .. }, _, _) => {
            return Err(CommandError::args(format!("'{id}' is a spline, which cannot be trimmed"))
                .hint("trim the curves that cross it, or delete it and draw a spline through other points"));
        }
        // The piece is the whole entity: nothing crosses it (or a circle is cut only once).
        (SkGeom::Line { a, b }, None, None) => {
            let (dead, gone) = delete_with_dependents(d, &[id.to_string()]);
            rep.deleted.extend(dead);
            rep.constraints_dropped.extend(gone);
            for p in [a, b] {
                drop_orphan_point(d, p, &mut rep);
            }
        }
        (SkGeom::Arc { center, start, end }, None, None) => {
            let (dead, gone) = delete_with_dependents(d, &[id.to_string()]);
            rep.deleted.extend(dead);
            rep.constraints_dropped.extend(gone);
            for p in [center, start, end] {
                drop_orphan_point(d, p, &mut rep);
            }
        }
        (SkGeom::Circle { center, .. }, None, None) => {
            let (dead, gone) = delete_with_dependents(d, &[id.to_string()]);
            rep.deleted.extend(dead);
            rep.constraints_dropped.extend(gone);
            drop_orphan_point(d, center, &mut rep);
        }
        (SkGeom::Line { a, b }, lo, hi) => {
            let (a, b) = (a.clone(), b.clone());
            let new_end = lo.as_ref().map(|c| cut_point(d, c, &mut rep)).transpose()?;
            let new_start = hi.as_ref().map(|c| cut_point(d, c, &mut rep)).transpose()?;
            // Length-like constraints stop making sense; orientation ones stay on `id`.
            let mut kept_points: Vec<String> = Vec::new();
            if let Some(nl) = &new_end {
                set_geom(d, id, SkGeom::Line { a: a.clone(), b: nl.clone() });
                kept_points.push(a.clone());
                rep.kept.push(id.to_string());
                if let Some(ns) = &new_start {
                    let other = add_entity(d, SkGeom::Line { a: ns.clone(), b: b.clone() }, construction);
                    define_points_first(d, &other);
                    rep.created.push(other.clone());
                    rep.kept.push(other);
                    // The two pieces stay on one line.
                    for p in [ns, &b] {
                        let c = add_constraint(d, SkConstraintKind::PointOnLine, &[p, id], None);
                        rep.constraints_added.push(c);
                    }
                    kept_points.push(b.clone());
                } else {
                    drop_orphan_point(d, &b, &mut rep);
                }
            } else if let Some(ns) = &new_start {
                set_geom(d, id, SkGeom::Line { a: ns.clone(), b: b.clone() });
                kept_points.push(b.clone());
                rep.kept.push(id.to_string());
                drop_orphan_point(d, &a, &mut rep);
            }
            retarget_line_constraints(d, id, &kept_points, &mut rep);
        }
        (SkGeom::Arc { center, start, end }, lo, hi) => {
            let (center, start, end) = (center.clone(), start.clone(), end.clone());
            let new_end = lo.as_ref().map(|c| cut_point(d, c, &mut rep)).transpose()?;
            let new_start = hi.as_ref().map(|c| cut_point(d, c, &mut rep)).transpose()?;
            let mut kept_points = vec![center.clone()];
            if let Some(nl) = &new_end {
                set_geom(d, id, SkGeom::Arc { center: center.clone(), start: start.clone(), end: nl.clone() });
                kept_points.push(start.clone());
                rep.kept.push(id.to_string());
                if let Some(ns) = &new_start {
                    let other = add_entity(
                        d,
                        SkGeom::Arc { center: center.clone(), start: ns.clone(), end: end.clone() },
                        construction,
                    );
                    define_points_first(d, &other);
                    rep.created.push(other.clone());
                    rep.kept.push(other);
                    // Same circle: the new start lies on it (the end follows from the arc itself).
                    let c = add_constraint(d, SkConstraintKind::PointOnCircle, &[ns, id], None);
                    rep.constraints_added.push(c);
                    kept_points.push(end.clone());
                } else {
                    drop_orphan_point(d, &end, &mut rep);
                }
            } else if let Some(ns) = &new_start {
                set_geom(d, id, SkGeom::Arc { center: center.clone(), start: ns.clone(), end: end.clone() });
                kept_points.push(end.clone());
                rep.kept.push(id.to_string());
                drop_orphan_point(d, &start, &mut rep);
            }
            retarget_fixed(d, id, &kept_points, &mut rep);
        }
        (SkGeom::Circle { center, radius }, Some(lo), Some(hi)) => {
            // The remainder runs counter-clockwise from the end of the removed piece to its start.
            let center = center.clone();
            let radius = *radius;
            let start = cut_point(d, hi, &mut rep)?;
            let end = cut_point(d, lo, &mut rep)?;
            let arc = add_entity(d, SkGeom::Arc { center: center.clone(), start, end }, construction);
            define_points_first(d, &arc);
            rep.created.push(arc.clone());
            rep.kept.push(arc.clone());
            let mut was_fixed = false;
            for c in &mut d.constraints {
                if c.entities.iter().any(|e| e == id) {
                    if c.kind == SkConstraintKind::Fixed {
                        // A fixed circle fixes its centre and radius: keep both.
                        c.entities = vec![center.clone()];
                        was_fixed = true;
                    } else {
                        for e in &mut c.entities {
                            if e == id {
                                *e = arc.clone();
                            }
                        }
                    }
                }
            }
            if was_fixed && let Ok(expr) = gusset_units::expr::parse(&format!("{radius} mm")) {
                let c = add_constraint(d, SkConstraintKind::Radius, &[&arc], Some(expr));
                rep.constraints_added.push(c);
            }
            d.entities.retain(|e| e.id != id);
            rep.deleted.push(id.to_string());
        }
        (SkGeom::Circle { .. }, _, _) => {
            return Err(
                CommandError::args("a circle needs two crossings to trim").hint("draw the cutting curves across it")
            );
        }
    }
    // Pieces made from a pattern source join its pattern; a deleted source leaves it.
    let pieces: Vec<String> = rep.created.iter().filter(|c| !c.starts_with('p')).cloned().collect();
    for p in &mut d.patterns {
        if p.of.iter().any(|o| o == id) {
            if rep.deleted.iter().any(|x| x == id) {
                p.of.retain(|o| o != id);
            }
            p.of.extend(pieces.iter().cloned());
        }
    }
    d.patterns.retain(|p| !p.of.is_empty());
    rep.constraints_dropped.sort();
    rep.constraints_dropped.dedup();
    Ok(json!({
        "kept": rep.kept,
        "created": rep.created,
        "deleted": rep.deleted,
        "constraints_added": rep.constraints_added,
        "constraints_dropped": rep.constraints_dropped,
    }))
}

/// A fixed line/arc keeps only its surviving original points fixed.
fn retarget_fixed(d: &mut SketchDef, id: &str, kept_points: &[String], rep: &mut Report) {
    let mut extra: Vec<SkConstraint> = Vec::new();
    for c in &mut d.constraints {
        if c.kind == SkConstraintKind::Fixed && c.entities == [id.to_string()] {
            let mut it = kept_points.iter();
            if let Some(first) = it.next() {
                c.entities = vec![first.clone()];
            }
            for p in it {
                extra.push(SkConstraint {
                    id: String::new(),
                    kind: SkConstraintKind::Fixed,
                    entities: vec![p.clone()],
                    value: None,
                });
            }
        }
    }
    for mut c in extra {
        c.id = new_id(d, 'k');
        rep.constraints_added.push(c.id.clone());
        d.constraints.push(c);
    }
}

/// After a line was shortened: drop the constraints that were about its length.
fn retarget_line_constraints(d: &mut SketchDef, id: &str, kept_points: &[String], rep: &mut Report) {
    d.constraints.retain(|c| {
        let about_length = c.entities.iter().any(|e| e == id)
            && match c.kind {
                SkConstraintKind::Distance => c.entities.iter().all(|e| e == id),
                SkConstraintKind::Equal | SkConstraintKind::Midpoint => true,
                _ => false,
            };
        if about_length {
            rep.constraints_dropped.push(c.id.clone());
        }
        !about_length
    });
    retarget_fixed(d, id, kept_points, rep);
}

/// Most copies one sketch pattern may make (the count includes the original).
const MAX_COPIES: f64 = 500.0;

/// Repeats sketch curves in a line or round a point.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SketchPattern {
    pub sketch: String,
    /// `linear` (dx, dy) or `circular` (center, angle).
    pub kind: String,
    /// Source lines, circles, arcs or splines (one pattern each).
    pub entities: Vec<String>,
    /// Copies including the original, 2..=500 (expression).
    pub count: String,
    /// Linear: shift per copy (expressions, default 0).
    #[serde(default)]
    pub dx: Option<String>,
    #[serde(default)]
    pub dy: Option<String>,
    /// Circular: centre point id.
    #[serde(default)]
    pub center: Option<String>,
    /// Circular: total angle (default 360 deg).
    #[serde(default)]
    pub angle: Option<String>,
    /// Id of an existing pattern to replace.
    #[serde(default)]
    pub id: Option<String>,
}

impl Command for SketchPattern {
    const ID: &'static str = "sketch.pattern";
    const TITLE: &'static str = "Sketch pattern";
    const DESCRIPTION: &'static str = "Repeat sketch curves linearly or round a point; copies (ids like l5_2) follow parameters and take part in regions and extrusions. Count and spacing are expressions. Replace one with id.";
    const CATEGORY: Category = Category::Sketch;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let sk = self.sketch.clone();
        let count = parse_expr("count", &self.count)?;
        let kind_name = self.kind.as_str();
        let out = edit(s, ctx, "Sketch pattern", |doc, _| {
            let values = params::evaluate(&doc.params);
            let units = doc.units;
            let scope = |n: &str| values.get(n).and_then(|r| r.as_ref().ok().copied());
            let eval =
                |what: &str, e: &gusset_units::expr::Expr, dim: gusset_units::Dim| -> Result<f64, CommandError> {
                    let q = gusset_units::expr::eval(e, &scope)
                        .map_err(|err| CommandError::args(format!("{what}: {}", err.message)))?;
                    gusset_units::params::coerce(q, dim, units)
                        .map(|q| q.value)
                        .map_err(|m| CommandError::args(format!("{what}: {m}")))
                };
            let n = eval("count", &count, gusset_units::Dim::NONE)?;
            if !n.is_finite() || (n - n.round()).abs() > 1e-6 || !(2.0..=MAX_COPIES).contains(&n.round()) {
                return Err(CommandError::args(format!(
                    "count is {n}; it must be a whole number from 2 to {MAX_COPIES}"
                ))
                .hint("the count includes the original"));
            }
            let d = sketch_mut(doc, &sk)?;
            for e in &self.entities {
                match d.entities.iter().find(|x| &x.id == e).map(|x| &x.geom) {
                    None => return Err(CommandError::not_found(format!("no entity '{e}' in {sk}"))),
                    Some(SkGeom::Point { .. }) => {
                        return Err(CommandError::args(format!(
                            "'{e}' is a point; patterns repeat lines, circles, arcs and splines"
                        )));
                    }
                    Some(_) => {}
                }
                if let Some(p) = d.patterns.iter().find(|p| p.of.contains(e) && Some(&p.id) != self.id.as_ref()) {
                    return Err(CommandError::args(format!("'{e}' already belongs to pattern {}", p.id))
                        .hint("pass that id to replace it, or delete it first"));
                }
            }
            let mut seen = std::collections::BTreeSet::new();
            if self.entities.is_empty() || !self.entities.iter().all(|e| seen.insert(e)) {
                return Err(CommandError::args("entities must be a non-empty list without repeats"));
            }
            let kind = match kind_name {
                "linear" => {
                    let dx = parse_expr("dx", self.dx.as_deref().unwrap_or("0 mm"))?;
                    let dy = parse_expr("dy", self.dy.as_deref().unwrap_or("0 mm"))?;
                    let (x, y) =
                        (eval("dx", &dx, gusset_units::Dim::LENGTH)?, eval("dy", &dy, gusset_units::Dim::LENGTH)?);
                    if x.hypot(y) < 1e-9 {
                        return Err(CommandError::args("dx and dy are both zero, so the copies would overlap")
                            .hint("give a spacing, for example dx: 10 mm"));
                    }
                    SkPatternKind::Linear { dx, dy }
                }
                "circular" => {
                    let center = self
                        .center
                        .clone()
                        .ok_or_else(|| CommandError::args("a circular pattern needs a center point"))?;
                    if !matches!(
                        d.entities.iter().find(|x| x.id == center).map(|x| &x.geom),
                        Some(SkGeom::Point { .. })
                    ) {
                        return Err(CommandError::args(format!("center '{center}' is not a point of {sk}")));
                    }
                    let angle = parse_expr("angle", self.angle.as_deref().unwrap_or("360 deg"))?;
                    if eval("angle", &angle, gusset_units::Dim::ANGLE)?.abs() < 1e-9 {
                        return Err(
                            CommandError::args("the angle is zero").hint("enter the total angle, for example 360 deg")
                        );
                    }
                    SkPatternKind::Circular { center, angle }
                }
                other => return Err(CommandError::args(format!("kind must be linear or circular, not '{other}'"))),
            };
            let id = match &self.id {
                Some(id) => {
                    if !d.patterns.iter().any(|p| &p.id == id) {
                        return Err(CommandError::not_found(format!("no pattern '{id}' in {sk}")));
                    }
                    id.clone()
                }
                None => new_id(d, 'g'),
            };
            let pattern = SkPattern { id: id.clone(), kind, of: self.entities.clone(), count: count.clone() };
            match d.patterns.iter_mut().find(|p| p.id == id) {
                Some(slot) => *slot = pattern,
                None => d.patterns.push(pattern),
            }
            store_solution(doc, &sk);
            Ok(json!({ "pattern": id, "copies": n.round() - 1.0 }))
        })?;
        let status = sketch_status(s, ctx, &sk, false)?;
        Ok(merge(out, json!({ "sketch_status": status })))
    }
}
