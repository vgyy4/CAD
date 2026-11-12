//! Whole-body edits of a design: Move `64E40C8E`, Scale `D45FD6C4` and RemoveBody `4A782808`
//! (ADR 0039 amendment, section 19 of the format notes). Each selects bodies through a
//! `2CA5A1CD` -> `9716F783` selection whose recipe is a `body_recipe_data` (a face recipe there
//! would be a Move of faces, which is not read) and a `D26351F0` body link; the body keeps the
//! name of the feature that made it.

use super::feature::{param_values, point3};
use super::geom::{V3, add, cross, dot, mm, mul, norm, round9, sub, unit};
use super::naming::{self, RecipeKind};
use super::stream::{Design, finite_doubles, ref_ids};
use super::{Ctx, Graph, PARAM, PARAM_HOLDER};
use crate::cover;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Context of one edit: the names the tree gave the features so far and the tree entries written.
pub(super) struct Edit<'a> {
    pub d: &'a Design<'a>,
    pub g: &'a Graph,
    pub names: &'a HashMap<u64, String>,
    pub feats: &'a [Value],
}

fn fail(name: &str, fusion_type: &str, class: &str, fid: u64, why: &str) -> Value {
    json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
        "source_id": fid, "error": why})
}

impl Edit<'_> {
    /// The one body a feature acts on, as the name of the tree feature that made it. Refuses a
    /// selection of faces or edges, several bodies, and a body whose maker is not a single-body
    /// feature of the tree (a pattern makes several bodies under one name).
    pub(super) fn target(&self, fid: u64) -> Result<String, String> {
        let links = self.g.selected_bodies(self.d, fid);
        let [link] = links.as_slice() else {
            return Err(format!("it selects {} bodies, not one", links.len()));
        };
        // Every recipe of the selections must name a body.
        for q in self.selections(fid) {
            for r in ref_ids(self.d, self.d.rec(q)) {
                if self.d.is(r, "7ACC2A03") && naming::recipe(self.d, r).0 != Some(RecipeKind::Body) {
                    return Err("it selects faces or edges, not whole bodies".into());
                }
            }
        }
        let maker = self.g.creator.get(link).copied().ok_or("the selected body has no maker")?;
        if maker == fid {
            return Err("it makes a body (a copy), which is not read".into());
        }
        let name = self.names.get(&maker).ok_or("the maker of the selected body has no name")?;
        let entry = self
            .feats
            .iter()
            .find(|f| f.get("source_id").and_then(Value::as_u64) == Some(maker) && f.get("name").is_some());
        let single = entry.is_some_and(|f| {
            matches!(f.get("type").and_then(Value::as_str), Some("extrude" | "sweep" | "stored_body"))
                && f.get("creates_bodies").and_then(Value::as_u64).is_none_or(|n| n == 1)
        }) || entry.is_some_and(|f| {
            // A mirror of one body into a new body (no merge): the copy is the mirror's body.
            f.get("type").and_then(Value::as_str) == Some("mirror")
                && f.get("merge").and_then(Value::as_bool) == Some(false)
                && f.get("creates_bodies").and_then(Value::as_u64) == Some(1)
                && f.get("features").and_then(Value::as_array).is_some_and(Vec::is_empty)
        });
        if single {
            return Ok(name.clone());
        }
        // The one copy of a body pattern of two: `<pattern>[1]`. Which of several copies a body
        // is, the record does not say.
        let made = self.g.creator.values().filter(|c| **c == maker).count();
        let entries: Vec<&Value> =
            self.feats.iter().filter(|f| f.get("source_id").and_then(Value::as_u64) == Some(maker)).collect();
        if let ([f], 1) = (entries.as_slice(), made)
            && matches!(f.get("type").and_then(Value::as_str), Some("pattern_linear" | "pattern_circular"))
            && f.get("merge").and_then(Value::as_bool) == Some(false)
            && f.get("count").and_then(Value::as_u64) == Some(2)
            && let Some(n) = f.get("name").and_then(Value::as_str)
        {
            return Ok(super::json::copy_name(n, 1));
        }
        Err(format!("the maker of the selected body ({name}) is not a one-body feature of the tree"))
    }

    /// The `9716F783` selections under the feature's selection lists.
    fn selections(&self, fid: u64) -> Vec<u64> {
        let mut out = Vec::new();
        for l in ref_ids(self.d, self.d.rec(fid)) {
            if self.d.is(l, "2CA5A1CD") {
                for s in ref_ids(self.d, self.d.rec(l)) {
                    if self.d.is(s, "9716F783") && !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
        }
        out
    }

    /// Move: the 4 x 4 matrix of the `84E6245F` record the feature starts with (row major, the
    /// translation in the last column, centimetres), as a translation. A turn is not read: the
    /// samples hold translations only, so the matrix convention of a rotation is unverified.
    pub(super) fn move_json(&self, fid: u64, name: &str, fusion_type: &str, class: &str) -> Value {
        let bad = |why: &str| fail(name, fusion_type, class, fid, why);
        let target = match self.target(fid) {
            Ok(t) => t,
            Err(e) => return bad(&e),
        };
        let mats: Vec<Mat> = ref_ids(self.d, self.d.rec(fid))
            .into_iter()
            .filter(|q| self.d.is(*q, "84E6245F"))
            .filter_map(|q| matrix(self.d.rec(q)))
            .collect();
        let Some(&m) = mats.first() else { return bad("the move has no transformation matrix") };
        if mats.iter().any(|o| *o != m) {
            return bad("the move's matrices disagree");
        }
        let (turn, t) = m;
        if turn.iter().flatten().zip([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).any(|(a, b)| (a - b).abs() > 1e-9) {
            return bad("the move turns the body (a rotation is not verified against a sample)");
        }
        let t = [mm(t[0]), mm(t[1]), mm(t[2])];
        if t.iter().all(|v| *v == 0.0) {
            return bad("the move does not move the body");
        }
        json!({"type": "move_body", "name": name, "source_id": fid, "target": target, "translate": t, "copy": false})
    }

    /// Scale about a point: the `ScaleFactor` parameter and the point of the `69EE2FA7` selection
    /// (a vertex: its recipe leads to a `994C518A` point record). Only the uniform scale, the one
    /// parameter the samples hold.
    pub(super) fn scale_json(&self, fid: u64, name: &str, fusion_type: &str, class: &str) -> Value {
        let bad = |why: &str| fail(name, fusion_type, class, fid, why);
        let target = match self.target(fid) {
            Ok(t) => t,
            Err(e) => return bad(&e),
        };
        let mut names: Vec<String> = Vec::new();
        for h in ref_ids(self.d, self.d.rec(fid)) {
            if self.d.is(h, PARAM_HOLDER) {
                for p in ref_ids(self.d, self.d.rec(h)) {
                    if self.d.is(p, PARAM)
                        && let Some(n) = super::stream::param(self.d, p).and_then(|p| p.name)
                        && !names.contains(&n)
                    {
                        names.push(n);
                    }
                }
            }
        }
        if names != ["ScaleFactor"] {
            return bad("the scale has parameters other than one factor (non-uniform scales are not read)");
        }
        let factors = param_values(self.d, fid, &["ScaleFactor"]);
        let [factor] = factors.as_slice() else { return bad("the scale factor is not one value") };
        if !(factor.is_finite() && *factor > 0.0) {
            return bad("the scale factor is not positive");
        }
        let mut centers: Vec<V3> = Vec::new();
        for q in ref_ids(self.d, self.d.rec(fid)) {
            if self.d.is(q, "69EE2FA7") {
                for r in ref_ids(self.d, self.d.rec(q)) {
                    for pt in ref_ids(self.d, self.d.rec(r)) {
                        if self.d.is(pt, "994C518A")
                            && let Some(p) = point3(self.d, pt)
                            && !centers.contains(&p)
                        {
                            centers.push(p);
                        }
                    }
                }
            }
        }
        let [c] = centers.as_slice() else { return bad("the scale has no single centre point") };
        json!({"type": "scale_body", "name": name, "source_id": fid, "target": target,
            "factor": round9(*factor), "center": [mm(c[0]), mm(c[1]), mm(c[2])]})
    }

    /// RemoveBody: the body its selection names.
    pub(super) fn remove_json(&self, fid: u64, name: &str, fusion_type: &str, class: &str) -> Value {
        match self.target(fid) {
            Ok(target) => json!({"type": "remove_body", "name": name, "source_id": fid, "target": target}),
            Err(e) => fail(name, fusion_type, class, fid, &e),
        }
    }
}

/// A rigid transformation: the rotation block (rows) and the translation.
pub(super) type Mat = ([[f64; 3]; 3], V3);

/// The first 4 x 4 matrix of a record: 16 doubles whose last row is exactly (0, 0, 0, 1) and whose
/// 3 x 3 block is orthonormal (the position moves with the record's reference widths).
pub(super) fn matrix(rec: &[u8]) -> Option<Mat> {
    for o in 8..rec.len().saturating_sub(127) {
        let Some(m) = finite_doubles::<16>(rec, o, 1e6) else { continue };
        let [a, b, c, tx, d, e, f, ty, g, h, i, tz, 0.0, 0.0, 0.0, 1.0] = m else { continue };
        let rows = [[a, b, c], [d, e, f], [g, h, i]];
        let ortho = rows.iter().enumerate().all(|(p, rp)| {
            rows.iter().enumerate().all(|(q, rq)| (dot(*rp, *rq) - if p == q { 1.0 } else { 0.0 }).abs() < 1e-6)
        });
        if ortho {
            cover::field(rec, o, 128, "body-op.matrix");
            return Some((rows, [tx, ty, tz]));
        }
    }
    None
}

/// Draft `F4163A07`: `Angle1` (`Angle2` must be 0), the faces to taper (a `2CA5A1CD` list of
/// bounded-face recipes) and the neutral plane (a list with one plain face recipe). The pull is
/// the plane face's outward normal and a positive angle narrows the part along it, a negative
/// one widens it: 355's Draft1 (10 degrees) and Draft2 (-10) give the drafted faces its `.smbh`
/// holds (section 28). The importer's state check refuses a draft that rebuilds to anything but
/// the stored bodies.
pub(super) fn draft_json(cx: &mut Ctx, fid: u64, name: &str, fusion_type: &str, class: &str) -> Value {
    let d = cx.d;
    let bad = |why: &str| fail(name, fusion_type, class, fid, why);
    let angles = param_values(d, fid, &["Angle1"]);
    let angle = match (angles.as_slice(), draft_role_angles(d, fid).as_slice()) {
        ([a], _) => {
            if param_values(d, fid, &["Angle2"]).iter().any(|a| a.abs() > 1e-12) {
                return bad("the draft has two angles");
            }
            *a
        }
        // Older files name both angles `Draft`, in holder order: the second must be 0.
        ([], [a, b]) if b.abs() <= 1e-12 => *a,
        ([], [_, _]) => return bad("the draft has two angles"),
        _ => return bad("the draft has no single angle"),
    };
    // Negative tapers the faces outward along the pull direction (as Gusset's draft does).
    if !angle.is_finite() || angle.abs() <= 1e-12 || angle.abs() >= std::f64::consts::FRAC_PI_2 {
        return bad("the draft angle is zero or not below 90 degrees");
    }
    let mut faces: Vec<Value> = Vec::new();
    // The drafted faces' normals (planes) or axes (cylinders), where known.
    let mut dirs: Vec<(V3, bool)> = Vec::new();
    let mut plane: Option<(V3, V3)> = None;
    let mut seen: Vec<u64> = Vec::new();
    type Pick = (Value, Option<(V3, bool)>, Option<(V3, V3)>);
    type Name = (String, Vec<i32>);
    type Listed = (u64, Vec<Option<RecipeKind>>, Vec<Pick>, Vec<Name>);
    let mut lists: Vec<Listed> = Vec::new();
    for l in ref_ids(d, d.rec(fid)) {
        if !d.is(l, "2CA5A1CD") || seen.contains(&l) {
            continue;
        }
        seen.push(l);
        // Per face: its pick, its normal or axis (`true`: a normal) and, for a plane, a point
        // and the outward normal (a neutral plane candidate).
        let mut picks: Vec<Pick> = Vec::new();
        let mut kinds: Vec<Option<RecipeKind>> = Vec::new();
        let mut names_of: Vec<Name> = Vec::new();
        for s in ref_ids(d, d.rec(l)) {
            if !d.is(s, "5662F619") {
                continue;
            }
            for r in ref_ids(d, d.rec(s)) {
                if !d.is(r, "7ACC2A03") && !d.is(r, "71D91267") {
                    continue;
                }
                let (kind, names) = naming::recipe(d, r);
                kinds.push(kind);
                let Some((g, ops)) = names.as_ref().and_then(|n| n.first()).and_then(|f| f.first()) else {
                    return bad("a face of the draft has no name");
                };
                let own = ops.len() == 1;
                names_of.push((g.clone(), ops.clone()));
                match cx.face_pick(g, ops) {
                    // A cap's modelled point is its plane's (the sketch origin's projection): two
                    // caps on one plane would be one pick. The stored cap's points tell them apart.
                    Some((p, n)) if (g == "1" || g == "2") && own => {
                        let pick = cx.stored_face(g, ops).map_or_else(|| json!({"point": v3(p)}), |sp| sp.json());
                        picks.push((pick, Some((n, true)), Some((p, n))));
                    }
                    Some((p, n)) => {
                        // The normal only where the face is the extrude's own (a shelled wall's
                        // sign is open).
                        let pick =
                            if own { json!({"point": v3(p), "normal": unit9(n)}) } else { json!({"point": v3(p)}) };
                        picks.push((pick, Some((n, true)), Some((p, n))));
                    }
                    None => match cx.general_face(g, ops) {
                        Some(sp) => {
                            let (dir, pl) = match sp.surface {
                                // A stored plane's normal is the face's outward one (section 28).
                                naming::Surface::Plane { origin, normal } => {
                                    (Some((normal, true)), Some((origin, normal)))
                                }
                                naming::Surface::Cylinder { axis, .. } => (Some((axis, false)), None),
                                _ => (None, None),
                            };
                            picks.push((sp.json(), dir, pl));
                        }
                        None => return bad("a face of the draft is neither modelled nor on one stored surface"),
                    },
                }
            }
        }
        if !kinds.is_empty() {
            lists.push((l, kinds, picks, names_of));
        }
    }
    // The faces are bounded-face recipes and the neutral plane one plain face. An older file
    // (324) names the faces by plain face recipes too: of two such lists the faces' has the
    // lower record id (as in the files with bounded faces: 355's 604 and 611, 258's 869 and 876).
    let bounded = |k: &[Option<RecipeKind>]| k.iter().all(|k| *k == Some(RecipeKind::BoundedFace));
    let plain = |k: &[Option<RecipeKind>]| k.iter().all(|k| *k == Some(RecipeKind::Face));
    if let [a, b] = lists.as_mut_slice()
        && plain(&a.1)
        && plain(&b.1)
    {
        let faces_list = if a.0 < b.0 { a } else { b };
        faces_list.1.iter_mut().for_each(|k| *k = Some(RecipeKind::BoundedFace));
    }
    // The drafted faces' planes (a point and the outward normal), where they are planes.
    let mut face_planes: Vec<Option<(V3, V3)>> = Vec::new();
    // The drafted faces' names, with their normals where they are planes.
    let mut drafted: Vec<(Name, Option<(V3, bool)>)> = Vec::new();
    for (_, kinds, picks, names_of) in lists {
        if bounded(&kinds) {
            for ((pick, dir, pl), nm) in picks.into_iter().zip(names_of) {
                drafted.push((nm, dir));
                faces.push(pick);
                dirs.extend(dir);
                face_planes.push(pl);
            }
        } else if let ([Some(RecipeKind::Face)], [(_, _, Some(pl))]) = (kinds.as_slice(), picks.as_slice()) {
            plane = Some(*pl);
        } else {
            return bad("the draft's selections are neither bounded faces nor one plain face");
        }
    }
    let Some((o, n)) = plane else { return bad("the draft has no neutral plane face") };
    if faces.is_empty() {
        return bad("the draft names no faces");
    }
    // The flags after the header of the older layout (`F4163A07` of 2014-2016 files): three
    // bytes, the first the flipped pull (258's Draft1 1, with +45 degrees, where its Draft2 has 0
    // and -45 degrees on the mirror image; 324's and 355's drafts 0), then `00 00 00 01` and the
    // first reference. Newer layouts keep the pull as the face's outward normal.
    let rec = d.rec(fid);
    let old_flags = super::stream::parse_header(rec)
        .map(|h| h.body)
        .filter(|&b| rec.get(b..b + 7).is_some_and(|x| matches!(x, [0 | 1, 1, 1, 0, 0, 0, 1])));
    if let Some(b) = old_flags {
        cover::field(rec, b, 3, "draft.flags");
    }
    let flip = old_flags.is_some_and(|b| rec.get(b) == Some(&1));
    let pull = if flip { [-n[0], -n[1], -n[2]] } else { n };
    let along = |(v, normal): &(V3, bool)| {
        let c = dot(*v, pull).abs() / (norm(*v) * norm(pull)).max(1e-300);
        if *normal { c < 1e-6 } else { (c - 1.0).abs() < 1e-6 }
    };
    if !dirs.iter().all(along) {
        // A plane face at a slant to the pull (258: a pin's start cap drafted about the 45
        // degree chamfer face beside it, by 45 degrees): Fusion turns the face about its line on
        // the neutral plane by the angle, its outward normal towards the pull for a positive
        // angle, as it does a side wall. That is the draft of a side wall about the plane through
        // that line square to the face, whose pull is the part of the pull across the face. The
        // stored pins of 258 end on the chamfer face, as both its drafts give this way.
        let ([face], [Some((fp, fnormal))]) = (faces.as_slice(), face_planes.as_slice()) else {
            return bad("a face of the draft does not stand along the pull direction");
        };
        let (Some(fu), Some(pu)) = (unit(*fnormal), unit(pull)) else {
            return bad("a face of the draft or its neutral plane has no normal");
        };
        let across = sub(pu, mul(fu, dot(pu, fu)));
        let (Some(m), Some(line)) = (unit(across), unit(cross(fu, pu))) else {
            return bad("a face of the draft lies parallel to the neutral plane");
        };
        // A point of the line where the face meets the neutral plane: from the face's point
        // along the face, square to the line, onto the neutral plane.
        let in_face = cross(line, fu);
        let k = dot(in_face, pu);
        if k.abs() < 1e-9 {
            return bad("a face of the draft lies parallel to the neutral plane");
        }
        let on_line = add(*fp, mul(in_face, dot(sub(o, *fp), pu) / k));
        // The stored face's points come from every state that has the face, the drafted one's
        // too (258: on the chamfer face, which they would pick as well): the face before the
        // draft is the points on its plane.
        let mm3 = |p: V3| [mm(p[0]), mm(p[1]), mm(p[2])];
        let (pmm, on_plane) = (mm3(*fp), |q: &V3| dot(sub(*q, mm3(*fp)), fu).abs() < 1e-4);
        let points: Vec<V3> = face
            .get("points")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|q| serde_json::from_value::<V3>(q.clone()).ok())
            .filter(on_plane)
            .collect();
        let point = points.first().copied().unwrap_or(pmm);
        let face = json!({"point": point, "points": points,
            "surface": {"type": "plane", "origin": pmm, "normal": unit9(fu)}});
        return json!({"type": "draft", "name": name, "source_id": fid, "faces": [face],
            "plane": {"origin": v3(on_line), "normal": unit9(m)}, "angle": round9(angle.to_degrees()), "flip": false});
    }
    // Pull direction: the neutral face's outward normal, reversed where the draft flips it.
    // 355's two drafts (10 and -10 degrees) rebuild to the stored drafted faces this way
    // (section 28). Where the stored bodies hold a drafted plane face after the draft (its name
    // carries the draft's operation, negated) and its normal is the one the reversed pull gives,
    // and none is the other, the pull is reversed: 3000's Draft1 (-20 degrees, about a wall
    // whose outward normal is -y) tapers a box's top down towards -y, as only a pull along +y
    // gives (section 45).
    let draft_ops: Vec<i32> =
        cx.opmap.iter().filter(|(_, f)| **f == fid).filter_map(|(k, _)| i32::try_from(*k).ok()).collect();
    let (mut agree, mut reversed) = (0usize, 0usize);
    let (sa, ca) = (libm::sin(angle), libm::cos(angle));
    for ((g, ops), dir) in &drafted {
        let (Some((_, true)), Some(nu), Some(pu)) = (dir, dir.and_then(|d| unit(d.0)), unit(pull)) else { continue };
        for k in &draft_ops {
            let mut after = ops.clone();
            after.push(-k);
            let Some(naming::Surface::Plane { normal, .. }) = cx.stored_face(g, &after).map(|sp| sp.surface) else {
                continue;
            };
            let close = |w: V3| norm(sub(normal, w)) < 1e-6;
            if close(add(mul(nu, ca), mul(pu, sa))) {
                agree += 1;
            } else if close(add(mul(nu, ca), mul(pu, -sa))) {
                reversed += 1;
            }
        }
    }
    let flip = if reversed > 0 && agree == 0 { !flip } else { flip };
    json!({"type": "draft", "name": name, "source_id": fid, "faces": faces,
        "plane": {"origin": v3(o), "normal": unit9(n)}, "angle": round9(angle.to_degrees()), "flip": flip})
}

/// The values of a draft's parameters with the role `Draft` (older files), in the order of
/// their holders (not deduplicated).
fn draft_role_angles(d: &Design, fid: u64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for q in ref_ids(d, d.rec(fid)) {
        if !d.is(q, PARAM_HOLDER) || seen.contains(&q) {
            continue;
        }
        seen.push(q);
        for q2 in ref_ids(d, d.rec(q)) {
            if d.is(q2, PARAM)
                && let Some(p) = super::stream::param(d, q2)
                && p.name.as_deref() == Some("Draft")
                && let Some(v) = p.value
            {
                out.push(v);
            }
        }
    }
    out
}

fn v3(p: V3) -> Value {
    json!([mm(p[0]), mm(p[1]), mm(p[2])])
}

fn unit9(n: V3) -> Value {
    json!([round9(n[0]), round9(n[1]), round9(n[2])])
}

/// Body->Comp `56225BC7` (a body made into a component) and CutPasteBodies `BA3059E4` (a body
/// cut from one component and pasted into another): two off-timeline features do the work, a
/// Base Feature `D7015B81` that holds the body in its new component and a RemoveBody `4A782808`
/// that takes it out of the old one (section 30).
pub(super) const BODY_MOVES: [&str; 2] = ["56225BC7", "BA3059E4"];

/// For every body move of the design whose halves are one Base Feature making N bodies and N
/// RemoveBodies of one body each: each body the Base Feature makes is a removed body carried on,
/// so its links are given to the removed body's maker (later features then act on that body).
/// With more than one body, a made body and a removed one pair by the name of their body
/// records (2831: `Base0` and `Sensor0`, section 43). Returns the moves and their halves, which
/// are not features of the tree.
pub(super) fn carry_moved_bodies(d: &Design, g: &mut Graph) -> HashMap<u64, Result<Vec<u64>, String>> {
    let mut out = HashMap::new();
    for &m in &d.ids {
        if !BODY_MOVES.iter().any(|c| d.is(m, c)) {
            continue;
        }
        let refs = ref_ids(d, d.rec(m));
        let mut bases: Vec<u64> = refs.iter().copied().filter(|q| d.is(*q, "D7015B81")).collect();
        let mut removes: Vec<u64> = refs.iter().copied().filter(|q| d.is(*q, "4A782808")).collect();
        bases.sort_unstable();
        bases.dedup();
        removes.sort_unstable();
        removes.dedup();
        let res = (|| {
            let [base] = bases.as_slice() else {
                return Err(format!(
                    "it names {} Base Features and {} RemoveBodies, not one Base Feature",
                    bases.len(),
                    removes.len()
                ));
            };
            // A copy (CopyPasteBodies, 3221): no RemoveBody, the body stays where it was and the
            // Base Feature holds the copy, a stored body of the tree like any Base Feature's.
            if removes.is_empty() {
                return Ok(Vec::new());
            }
            let mut made: Vec<u64> = g.creator.iter().filter(|(_, f)| **f == *base).map(|(l, _)| *l).collect();
            made.sort_unstable();
            if made.len() != removes.len() {
                return Err(format!("its Base Feature makes {} bodies for {} RemoveBodies", made.len(), removes.len()));
            }
            let mut pairs: Vec<(u64, u64)> = Vec::new();
            for remove in &removes {
                let removed = g.selected_bodies(d, *remove);
                let [link] = removed.as_slice() else {
                    return Err(format!("its RemoveBody selects {} bodies, not one", removed.len()));
                };
                let maker = g.creator.get(link).copied().ok_or("the moved body has no maker")?;
                let to = match made.as_slice() {
                    [one] => *one,
                    _ => {
                        let name = link_body_name(d, *link).ok_or("a moved body has no name")?;
                        let same: Vec<u64> =
                            made.iter().copied().filter(|l| link_body_name(d, *l).as_ref() == Some(&name)).collect();
                        let [one] = same.as_slice() else {
                            return Err("a moved body is not the one body of its name the Base Feature makes".into());
                        };
                        *one
                    }
                };
                if pairs.iter().any(|(l, _)| *l == to) {
                    return Err("two moved bodies pair with one body of the Base Feature".into());
                }
                pairs.push((to, maker));
            }
            for (l, maker) in pairs {
                g.creator.insert(l, maker);
            }
            let mut halves = vec![*base];
            halves.extend(removes.iter().copied());
            Ok(halves)
        })();
        out.insert(m, res);
    }
    out
}

/// The name of the body record (`D3937028`) a body link names.
fn link_body_name(d: &Design, link: u64) -> Option<String> {
    let body = ref_ids(d, d.rec(link)).into_iter().find(|q| d.is(*q, "D3937028"))?;
    super::feature::feature_name(d, body).1
}
