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
    /// A face of a pattern's copy: the pattern (by the source's name) and the copy's number
    /// (1 the first after the seed); the rest names the seed's face.
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    copy: Option<u32>,
}

/// What a [`FaceSel`] stands for in the document's names.
enum Want {
    Side {
        feature: String,
        sketch: String,
        entities: Vec<String>,
        plane: Option<(V3, V3)>,
    },
    Cap {
        feature: String,
        plane: Option<(V3, V3)>,
    },
    /// The blends made on the edges of that selection number, with those edges' names (one
    /// source edge may be several of Gusset's).
    Blend {
        fillet: Vec<(String, String)>,
    },
    /// The face pattern copy `k` made from the face the inner want names.
    Instance {
        pattern: String,
        k: u32,
        face: Box<Want>,
    },
}

/// Most source curves one side face may name.
const MAX_CURVES: usize = 64;

fn want(g: &Gen, f: &FaceSel) -> Option<Want> {
    if let Some(p) = &f.pattern {
        let pattern = g.features.get(p)?.clone();
        let k = f.copy.filter(|k| *k > 0)?;
        let seed = FaceSel { pattern: None, copy: None, ..f.clone() };
        return Some(Want::Instance { pattern, k, face: Box::new(want(g, &seed)?) });
    }
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
            let fillet: Vec<(String, String)> =
                g.blends.get(&f.feature)?.iter().filter(|b| b.0 == k).map(|b| (b.1.clone(), b.2.clone())).collect();
            (!fillet.is_empty()).then_some(Want::Blend { fillet })
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
        Want::Instance { pattern, k, face } => match &r.key {
            FaceKey::Instance { k: rk, face: inner } => {
                r.feature == *pattern && rk == k && holds(face, inner, info, loose)
            }
            _ => false,
        },
        Want::Blend { fillet } => match &r.key {
            FaceKey::Blend(e) => {
                let e = e.to_string();
                fillet.iter().any(|(f, edge)| r.feature == *f && e == *edge)
            }
            _ => false,
        },
    }
}

/// The model's edges a pick names by its faces, as (body feature, edge name): the edge
/// between faces that hold the pick's two faces, its ends touching the end faces where
/// several do; by Gusset's names first, then flat faces by their planes. Gusset may split
/// what the source keeps as one edge (a closed curve cut at its seam): pieces joined at
/// vertices where only those two faces meet count as one edge. Empty when the pick names no
/// faces, a face is not one the importer made, or no single edge fits.
pub(super) fn named_edge(g: &Gen, ev: &gusset_eval::Evaluation, pick: &EdgePick) -> Vec<(String, String)> {
    let [a, b] = pick.faces.as_slice() else { return Vec::new() };
    let (Some(wa), Some(wb)) = (want(g, a), want(g, b)) else { return Vec::new() };
    let ends: Vec<Option<Want>> = pick.ends.iter().take(2).map(|e| e.as_ref().and_then(|e| want(g, e))).collect();
    let strict = edge_between(ev, (&wa, &wb), &ends, false);
    let mut found = if strict.is_empty() { edge_between(ev, (&wa, &wb), &ends, true) } else { strict };
    match found.len() {
        1 => found.pop().unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The edges between faces that `wa` and `wb` hold, in runs that are one source edge, those
/// whose ends touch the end faces where there are several.
fn edge_between(
    ev: &gusset_eval::Evaluation,
    (wa, wb): (&Want, &Want),
    ends: &[Option<Want>],
    loose: bool,
) -> Vec<Vec<(String, String)>> {
    let mut found: Vec<Vec<(String, String)>> = Vec::new();
    for body in &ev.bodies {
        let t = &body.topology;
        let named = |w: &Want, f: u32| {
            body.names.faces.get(f as usize).is_some_and(|r| holds(w, r, t.faces.get(f as usize), loose))
        };
        // Loose: a cylindrical face named is also every face that continues its cylinder
        // across edges (round sw17: SolidWorks keeps one face where a cut's arc carries on a
        // boss's arc of the same circle, and names it after one of the arcs; Gusset keeps a
        // face per arc).
        let (grown_a, grown_b) = if loose {
            (same_cylinder(t, |f| named(wa, f)), same_cylinder(t, |f| named(wb, f)))
        } else {
            (Vec::new(), Vec::new())
        };
        let face_holds = |w: &Want, f: u32| {
            named(w, f)
                || (std::ptr::eq(w, wa) && grown_a.contains(&f))
                || (std::ptr::eq(w, wb) && grown_b.contains(&f))
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
        if here.len() > MAX_RUN_EDGES {
            continue;
        }
        // Runs: edges joined at a vertex where only faces of the two meet.
        let inner = |v: u32| around(v).iter().all(|f| face_holds(wa, *f) || face_holds(wb, *f));
        let mut runs: Vec<Vec<usize>> = Vec::new();
        for &i in &here {
            let verts = t.edges.get(i).map(|e| e.vertices.clone()).unwrap_or_default();
            let joined: Vec<usize> = (0..runs.len())
                .filter(|&r| {
                    runs.get(r).is_some_and(|run| {
                        run.iter().any(|&j| {
                            t.edges.get(j).is_some_and(|e| e.vertices.iter().any(|v| verts.contains(v) && inner(*v)))
                        })
                    })
                })
                .collect();
            let mut run = vec![i];
            for r in joined.into_iter().rev() {
                if r < runs.len() {
                    run.extend(runs.remove(r));
                }
            }
            runs.push(run);
        }
        // A run's ends: the vertices only one of its edges has.
        let run_ends = |run: &[usize]| -> Vec<u32> {
            let all: Vec<u32> = run.iter().filter_map(|&i| t.edges.get(i)).flat_map(|e| e.vertices.clone()).collect();
            let mut once: Vec<u32> =
                all.iter().copied().filter(|v| all.iter().filter(|w| *w == v).count() == 1).collect();
            // A single closed edge has one vertex, listed once.
            if run.len() == 1 && once.len() == 1 {
                once.push(once.first().copied().unwrap_or_default());
            }
            once
        };
        if runs.len() > 1 && ends.iter().any(Option::is_some) {
            runs.retain(|run| {
                let at: Vec<Vec<u32>> = run_ends(run).iter().map(|v| around(*v)).collect();
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
        for run in runs {
            let names: Vec<(String, String)> = run
                .iter()
                .filter_map(|&i| body.names.edges.get(i).map(|n| (body.feature.clone(), n.to_string())))
                .collect();
            found.push(names);
        }
    }
    found
}

/// The faces that continue the cylinders of the faces `seed` holds: reached across edges
/// from cylindrical seed faces, cylindrical themselves, about the same axis line (their
/// shared edges put them on the same radius). Seed faces are not listed.
fn same_cylinder(t: &gusset_kernel::Topology, seed: impl Fn(u32) -> bool) -> Vec<u32> {
    let cyl = |f: u32| t.faces.get(f as usize).filter(|fi| fi.kind == SurfaceKind::Cylinder).and_then(|fi| fi.axis);
    let coaxial = |a: gusset_kernel::Axis3, b: gusset_kernel::Axis3| {
        norm(cross(a.dir, b.dir)) < 1e-6 && norm(cross(sub(b.origin, a.origin), a.dir)) < 1e-6
    };
    let mut set: Vec<u32> = (0..t.faces.len() as u32).filter(|&f| seed(f) && cyl(f).is_some()).collect();
    let seeds = set.len();
    if seeds == 0 {
        return Vec::new();
    }
    let mut grew = true;
    while grew && set.len() < MAX_RUN_EDGES {
        grew = false;
        for e in &t.edges {
            let [f0, f1] = e.faces.as_slice() else { continue };
            for (a, b) in [(*f0, *f1), (*f1, *f0)] {
                if set.contains(&a)
                    && !set.contains(&b)
                    && let (Some(x), Some(y)) = (cyl(a), cyl(b))
                    && coaxial(x, y)
                {
                    set.push(b);
                    grew = true;
                }
            }
        }
    }
    set.split_off(seeds)
}

/// Most edges between one pair of faces looked at.
const MAX_RUN_EDGES: usize = 256;
