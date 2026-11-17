//! Edges named by their faces (ADR 0042 amendment, round sw9 of ADR 0043): a source that
//! records an edge as the two faces it joins (and the faces at its ends) instead of a point
//! lets the importer find it among the model's edges by Gusset's own face names, where a
//! midpoint would need the shape of every earlier feature (the faces of earlier fillets).

use super::{EdgePick, Gen, V3, cross, dot, norm, sub};
use gusset_doc::{FaceKey, FaceRef};
use gusset_kernel::{FaceInfo, SurfaceKind};
use serde::Deserialize;

/// One face of a source edge: a side of an extrude or revolve (the source sketch curves that
/// sweep it), a cap of an extrude, or the face a fillet or chamfer made from the `index`-th
/// (1-based) edge of its selection. A flat side or cap may give its plane (`point`, `normal`).
#[derive(Debug, Clone, Deserialize)]
pub(super) struct FaceSel {
    feature: String,
    kind: String,
    #[serde(default)]
    sketch: Option<String>,
    #[serde(default)]
    curves: Vec<String>,
    #[serde(default)]
    point: Option<V3>,
    #[serde(default)]
    normal: Option<V3>,
    #[serde(default)]
    index: Option<u32>,
}

/// What a [`FaceSel`] stands for in the document's names.
enum Want {
    Side { feature: String, sketch: String, entities: Vec<String>, plane: Option<(V3, V3)> },
    Cap { feature: String, plane: Option<(V3, V3)> },
    Blend { fillet: String, edge: String },
}

/// Most source curves one side face may name.
const MAX_CURVES: usize = 64;

fn want(g: &Gen, f: &FaceSel) -> Option<Want> {
    let feature = g.features.get(&f.feature)?.clone();
    let plane = match (f.point, f.normal) {
        (Some(p), Some(n)) if p.iter().chain(&n).all(|v| v.is_finite()) && norm(n) > 1e-9 => {
            Some((p, n.map(|v| v / norm(n))))
        }
        _ => None,
    };
    match f.kind.as_str() {
        "side" => {
            let sm = g.sketches.get(f.sketch.as_deref()?)?;
            let entities: Vec<String> =
                f.curves.iter().take(MAX_CURVES).filter_map(|c| sm.ids.get(c).cloned()).collect();
            (!entities.is_empty()).then(|| Want::Side { feature, sketch: sm.name.clone(), entities, plane })
        }
        "cap" => Some(Want::Cap { feature, plane }),
        "blend" => {
            let k = f.index?;
            let (fillet, edge) =
                g.blends.get(&f.feature)?.iter().find(|b| b.0 == k).map(|b| (b.1.clone(), b.2.clone()))?;
            Some(Want::Blend { fillet, edge })
        }
        _ => None,
    }
}

/// Whether a face of the model is flat and lies on a plane (a point and a unit normal).
fn on_plane(info: Option<&FaceInfo>, (p, n): (V3, V3)) -> bool {
    info.is_some_and(|fi| {
        fi.kind == SurfaceKind::Plane && dot(sub(fi.centroid, p), n).abs() < 1e-3 && norm(cross(fi.normal, n)) < 1e-6
    })
}

/// Whether the face named `r` is the one `w` wants. `loose`: a flat face given with its plane
/// is any flat face of the model on that plane, whichever feature Gusset names it after (a
/// later feature's coplanar face merged with it).
fn holds(w: &Want, r: &FaceRef, info: Option<&FaceInfo>, loose: bool) -> bool {
    match w {
        Want::Side { plane: Some(pl), .. } | Want::Cap { plane: Some(pl), .. } if loose => on_plane(info, *pl),
        Want::Side { feature, sketch, entities, .. } => {
            r.feature == *feature
                && matches!(&r.key, FaceKey::Side { sketch: s, entity } if s == sketch && entities.contains(entity))
        }
        Want::Cap { feature, plane } => {
            r.feature == *feature && matches!(r.key, FaceKey::Cap { .. }) && plane.is_none_or(|pl| on_plane(info, pl))
        }
        Want::Blend { fillet, edge } => {
            r.feature == *fillet && matches!(&r.key, FaceKey::Blend(e) if e.to_string() == *edge)
        }
    }
}

/// The model's edge a pick names by its faces, as (body feature, edge name): the one edge
/// between faces that hold the pick's two faces, its ends touching the end faces where
/// several do; by Gusset's names first, then flat faces by their planes. `None` when the pick
/// names no faces, a face is not one the importer made, or no single edge fits.
pub(super) fn named_edge(g: &Gen, ev: &gusset_eval::Evaluation, pick: &EdgePick) -> Option<(String, String)> {
    let [a, b] = pick.faces.as_slice() else { return None };
    let (wa, wb) = (want(g, a)?, want(g, b)?);
    let ends: Vec<Option<Want>> = pick.ends.iter().take(2).map(|e| e.as_ref().and_then(|e| want(g, e))).collect();
    let strict = edge_between(ev, (&wa, &wb), &ends, false);
    let found = if strict.is_empty() { edge_between(ev, (&wa, &wb), &ends, true) } else { strict };
    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// The edges between faces that `wa` and `wb` hold, their ends touching the end faces where
/// several edges are.
fn edge_between(
    ev: &gusset_eval::Evaluation,
    (wa, wb): (&Want, &Want),
    ends: &[Option<Want>],
    loose: bool,
) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for body in &ev.bodies {
        let t = &body.topology;
        let face_holds = |w: &Want, f: u32| {
            body.names.faces.get(f as usize).is_some_and(|r| holds(w, r, t.faces.get(f as usize), loose))
        };
        // The faces around a vertex: those of every edge that ends there.
        let around = |v: u32| -> Vec<u32> {
            let mut fs: Vec<u32> =
                t.edges.iter().filter(|e| e.vertices.contains(&v)).flat_map(|e| e.faces.iter().copied()).collect();
            fs.sort_unstable();
            fs.dedup();
            fs
        };
        let mut here: Vec<usize> = Vec::new();
        for (i, e) in t.edges.iter().enumerate() {
            let [f0, f1] = e.faces.as_slice() else { continue };
            if (face_holds(wa, *f0) && face_holds(wb, *f1)) || (face_holds(wa, *f1) && face_holds(wb, *f0)) {
                here.push(i);
            }
        }
        if here.len() > 1 && ends.iter().any(Option::is_some) {
            here.retain(|&i| {
                let Some(e) = t.edges.get(i) else { return false };
                let at: Vec<Vec<u32>> = e.vertices.iter().map(|v| around(*v)).collect();
                let touches = |w: &Option<Want>, k: usize| {
                    w.as_ref().is_none_or(|w| at.get(k).is_some_and(|fs| fs.iter().any(|f| face_holds(w, *f))))
                };
                let (e0, e1) = (ends.first().unwrap_or(&None), ends.get(1).unwrap_or(&None));
                match at.len() {
                    2 => (touches(e0, 0) && touches(e1, 1)) || (touches(e0, 1) && touches(e1, 0)),
                    _ => touches(e0, 0) && touches(e1, 0),
                }
            });
        }
        for i in here {
            if let Some(n) = body.names.edges.get(i) {
                found.push((body.feature.clone(), n.to_string()));
            }
        }
    }
    found
}
