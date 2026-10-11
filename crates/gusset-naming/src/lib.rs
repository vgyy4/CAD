//! Persistent topological naming (ADR 0003).
//!
//! Turns kernel operation history ([`FaceOrigin`]) into stable [`FaceRef`] names, names
//! edges by their adjacent faces, and resolves stored [`EdgeRef`]s back to edge indices
//! with explicit `Resolved` / `Ambiguous` / `Missing` outcomes. Nothing here depends on
//! a particular kernel; it is tested with the mock kernel.

#![cfg_attr(test, allow(clippy::float_cmp))]

use gusset_doc::{CapSide, EdgeRef, FaceKey, FaceRef};
use gusset_kernel::{CapEnd, FaceOrigin, Topology, V3};
use std::collections::BTreeMap;

// ------------------------------------------------------------------ entity tags

/// Encodes a sketch entity id (`l3`) as a kernel profile tag.
pub fn entity_tag(id: &str) -> Option<u64> {
    let mut c = id.chars();
    let p = c.next()?;
    if !p.is_ascii_lowercase() {
        return None;
    }
    let n: u32 = c.as_str().parse().ok()?;
    Some((u64::from(p as u8) << 32) | u64::from(n))
}

/// Decodes a profile tag back into a sketch entity id.
pub fn tag_entity(tag: u64) -> Option<String> {
    let p = u8::try_from(tag >> 32).ok()?;
    if !p.is_ascii_lowercase() {
        return None;
    }
    Some(format!("{}{}", p as char, tag & 0xffff_ffff))
}

// ------------------------------------------------------------------ naming context

/// What the naming layer needs to know about the operation that produced a body.
#[derive(Debug, Clone)]
pub enum OpContext<'a> {
    /// A sweep (extrude/revolve) of `sketch` by `feature`. `regions[i]` is the identifying
    /// entity of profile region `i`; caps carry it only when there are several regions.
    Sweep { feature: &'a str, sketch: &'a str, regions: &'a [String] },
    /// An operation on existing bodies (booleans, fillet, chamfer). `inputs[i]` holds the
    /// names of input shape `i` (faces and edges).
    Modify { feature: &'a str, inputs: &'a [&'a BodyNames] },
    /// A combined operation: sweep then boolean. Input 0 is the target body, input 1
    /// the freshly swept tool (whose own origins are named as a sweep).
    SweepInto { feature: &'a str, sketch: &'a str, regions: &'a [String], target: &'a BodyNames },
    /// An imported body; faces are named by index.
    Import { feature: &'a str },
}

/// Names of every face and edge of one body.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BodyNames {
    pub faces: Vec<FaceRef>,
    pub edges: Vec<EdgeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NamingError(pub String);

impl std::fmt::Display for NamingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NamingError {}

fn cap(end: CapEnd) -> CapSide {
    match end {
        CapEnd::Start => CapSide::Start,
        CapEnd::End => CapSide::End,
    }
}

/// Precedence when a face has several origins: generated-by-this-op names first
/// (they are the most specific), then carried-over input names.
fn origin_rank(o: &FaceOrigin) -> u8 {
    match o {
        FaceOrigin::Lateral { .. } | FaceOrigin::Cap { .. } => 0,
        FaceOrigin::FromEdge { .. } | FaceOrigin::FromVertex { .. } => 1,
        FaceOrigin::Input { .. } => 2,
    }
}

fn sweep_face(feature: &str, sketch: &str, regions: &[String], o: &FaceOrigin) -> Option<FaceRef> {
    match o {
        FaceOrigin::Lateral { tag } => Some(FaceRef::new(
            feature,
            FaceKey::Side { sketch: sketch.to_string(), entity: tag_entity(*tag).unwrap_or_else(|| format!("t{tag}")) },
        )),
        FaceOrigin::Cap { end, region } => {
            // The first region (regions are ordered by identifying entity, so the oldest
            // loop) keeps the plain `cap(end)` name: adding another closed loop to the
            // sketch later must not rename the caps existing references use (review L4).
            let region = if *region == 0 { None } else { regions.get(*region as usize).cloned() };
            Some(FaceRef::new(feature, FaceKey::Cap { side: cap(*end), region }))
        }
        _ => None,
    }
}

/// Computes the names of every face and edge of a body produced by an operation.
pub fn name_body(ctx: &OpContext<'_>, origins: &[Vec<FaceOrigin>], topo: &Topology) -> Result<BodyNames, NamingError> {
    if origins.len() != topo.faces.len() {
        return Err(NamingError(format!(
            "kernel reported history for {} faces but the body has {}",
            origins.len(),
            topo.faces.len()
        )));
    }
    let mut faces = Vec::with_capacity(origins.len());
    for (fi, os) in origins.iter().enumerate() {
        let mut sorted = os.clone();
        sorted.sort_by_key(origin_rank);
        // Imported faces are named by index; they have no operation history (the
        // kernel reports none), so this must not depend on `os`.
        let mut name = match ctx {
            OpContext::Import { feature } => {
                Some(FaceRef::new(*feature, FaceKey::Imported(u32::try_from(fi).unwrap_or(u32::MAX))))
            }
            _ => None,
        };
        for o in if name.is_none() { sorted.as_slice() } else { &[] } {
            name = match (ctx, o) {
                (OpContext::Sweep { feature, sketch, regions }, o) => sweep_face(feature, sketch, regions, o),
                (
                    OpContext::SweepInto { feature, sketch, regions, .. },
                    FaceOrigin::Lateral { .. } | FaceOrigin::Cap { .. },
                ) => sweep_face(feature, sketch, regions, o),
                (OpContext::SweepInto { target, .. }, FaceOrigin::Input { input: 0, face }) => {
                    target.faces.get(*face as usize).cloned()
                }
                (OpContext::Modify { inputs, .. }, FaceOrigin::Input { input, face }) => {
                    inputs.get(*input as usize).and_then(|b| b.faces.get(*face as usize)).cloned()
                }
                (OpContext::Modify { feature, inputs }, FaceOrigin::FromEdge { input, edge }) => inputs
                    .get(*input as usize)
                    .and_then(|b| b.edges.get(*edge as usize))
                    .map(|e| FaceRef::new(*feature, FaceKey::Blend(Box::new(e.clone())))),
                (OpContext::Modify { feature, inputs }, FaceOrigin::FromVertex { input, vertex }) => {
                    // Corner blend: named after the smallest-named input edge at that vertex.
                    let b = inputs.get(*input as usize);
                    b.and_then(|b| corner_edge(b, *vertex)).map(|e| FaceRef::new(*feature, FaceKey::Blend(Box::new(e))))
                }
                (OpContext::Import { feature }, _) => {
                    Some(FaceRef::new(*feature, FaceKey::Imported(u32::try_from(fi).unwrap_or(u32::MAX))))
                }
                _ => None,
            };
            if name.is_some() {
                break;
            }
        }
        let Some(name) = name else {
            return Err(NamingError(format!("face {fi} has no usable origin ({os:?})")));
        };
        faces.push(name);
    }
    // Disambiguate duplicate face names by geometric key.
    disambiguate(&mut faces, |i| topo.faces.get(i).map(|f| geo_key(f.centroid, f.area)), |f, k| f.index = k);
    // Edge names from adjacent face names.
    let mut edges: Vec<EdgeRef> = topo
        .edges
        .iter()
        .map(|e| EdgeRef::new(e.faces.iter().filter_map(|&f| faces.get(f as usize).cloned()).collect(), None))
        .collect();
    disambiguate(&mut edges, |i| topo.edges.get(i).map(|e| geo_key(e.midpoint, e.length)), |e, k| e.index = k);
    Ok(BodyNames { faces, edges })
}

fn corner_edge(b: &BodyNames, _vertex: u32) -> Option<EdgeRef> {
    // Vertex adjacency is not tracked in BodyNames; use the smallest edge name overall as a
    // stable fallback. Corner blends are rarely referenced directly.
    b.edges.iter().min_by_key(|e| e.to_string()).cloned()
}

/// A deterministic ordering key from rounded geometry.
fn geo_key(p: V3, size: f64) -> [i64; 4] {
    // 1e-6 mm resolution keeps keys stable against floating-point noise.
    let q = |v: f64| (v * 1e6).round() as i64;
    [q(p[0]), q(p[1]), q(p[2]), q(size)]
}

/// Assigns 1-based `#k` indices to items whose names collide, ordered by geometric key.
fn disambiguate<T: Clone + ToString>(
    items: &mut [T],
    key: impl Fn(usize) -> Option<[i64; 4]>,
    set: impl Fn(&mut T, Option<u32>),
) {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        groups.entry(it.to_string()).or_default().push(i);
    }
    for idxs in groups.values() {
        if idxs.len() < 2 {
            continue;
        }
        let mut sorted = idxs.clone();
        sorted.sort_by_key(|&i| (key(i), i));
        for (k, &i) in sorted.iter().enumerate() {
            if let Some(it) = items.get_mut(i) {
                set(it, Some(u32::try_from(k + 1).unwrap_or(u32::MAX)));
            }
        }
    }
}

// ------------------------------------------------------------------ resolution

/// Outcome of resolving a stored reference against the current geometry.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    Resolved(u32),
    /// Several entities match; the reference needs a `#k` index. Candidates are listed by
    /// their full names.
    Ambiguous(Vec<EdgeRef>),
    /// Nothing matches. `reason` is human-readable; `suggestion` is the closest candidate
    /// (offered to the user as a fix, never applied automatically).
    Missing {
        reason: String,
        suggestion: Option<EdgeRef>,
    },
}

impl BodyNames {
    pub fn face_index(&self, r: &FaceRef) -> Option<u32> {
        self.faces.iter().position(|f| f == r).and_then(|i| u32::try_from(i).ok())
    }

    /// Resolves an edge reference (ADR 0003 §5).
    pub fn resolve_edge(&self, r: &EdgeRef) -> Resolution {
        if let Some(i) = self.edges.iter().position(|e| e == r) {
            return Resolution::Resolved(u32::try_from(i).unwrap_or(u32::MAX));
        }
        // Same faces, different/absent index?
        let same_faces: Vec<&EdgeRef> = self.edges.iter().filter(|e| e.faces == r.faces).collect();
        match (same_faces.as_slice(), r.index) {
            ([only], _) => {
                // The edge stopped (or started) needing an index; it is unambiguous.
                return self
                    .edges
                    .iter()
                    .position(|e| e == *only)
                    .map_or(Resolution::Missing { reason: "internal".into(), suggestion: None }, |i| {
                        Resolution::Resolved(u32::try_from(i).unwrap_or(u32::MAX))
                    });
            }
            ([], _) => {}
            (many, None) => return Resolution::Ambiguous(many.iter().map(|e| (*e).clone()).collect()),
            (many, Some(_)) => {
                return Resolution::Missing {
                    reason: format!("{} edges now lie between these faces and the index no longer matches", many.len()),
                    suggestion: many.first().map(|e| (*e).clone()),
                };
            }
        }
        // Explain which face is gone, and suggest an edge sharing the surviving faces.
        let missing: Vec<String> =
            r.faces.iter().filter(|f| !self.faces.iter().any(|g| g == *f)).map(ToString::to_string).collect();
        let reason = if missing.is_empty() {
            "these faces no longer share an edge".to_string()
        } else {
            format!("face {} no longer exists", missing.join(" and "))
        };
        let suggestion = self
            .edges
            .iter()
            .max_by_key(|e| (e.faces.iter().filter(|f| r.faces.contains(f)).count(), std::cmp::Reverse(e.to_string())))
            .filter(|e| e.faces.iter().any(|f| r.faces.contains(f)))
            .cloned();
        Resolution::Missing { reason, suggestion }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gusset_kernel::mock::MockKernel;
    use gusset_kernel::*;

    fn rect_profile(w: f64, h: f64, ids: [&str; 4]) -> Profile {
        let pts = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
        let edges = (0..4)
            .map(|i| ProfileEdge {
                seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] },
                tag: entity_tag(ids[i]).unwrap(),
            })
            .collect();
        Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] }
    }

    fn named_box(w: f64) -> BodyNames {
        let k = MockKernel;
        let r = k
            .extrude(
                &rect_profile(w, 20.0, ["l5", "l6", "l7", "l8"]),
                &ExtrudeSpec { distance: 10.0, symmetric: false },
            )
            .unwrap();
        let t = k.topology(&r.shape).unwrap();
        name_body(
            &OpContext::Sweep { feature: "Extrude1", sketch: "Sketch1", regions: &["l5".into()] },
            &r.face_origins,
            &t,
        )
        .unwrap()
    }

    #[test]
    fn tags_roundtrip() {
        for id in ["l3", "p1", "a42", "c4294967295"] {
            assert_eq!(tag_entity(entity_tag(id).unwrap()).unwrap(), id);
        }
        assert!(entity_tag("L3").is_none());
        assert!(entity_tag("l").is_none());
    }

    #[test]
    fn box_names() {
        let n = named_box(40.0);
        let names: Vec<String> = n.faces.iter().map(ToString::to_string).collect();
        assert_eq!(
            names,
            [
                "Extrude1/cap(start)",
                "Extrude1/cap(end)",
                "Extrude1/side(Sketch1/l5)",
                "Extrude1/side(Sketch1/l6)",
                "Extrude1/side(Sketch1/l7)",
                "Extrude1/side(Sketch1/l8)"
            ]
        );
        // All 12 edges have distinct names without indices.
        assert_eq!(n.edges.len(), 12);
        assert!(n.edges.iter().all(|e| e.index.is_none()));
        let mut uniq: Vec<String> = n.edges.iter().map(ToString::to_string).collect();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 12);
    }

    #[test]
    fn references_survive_parameter_changes() {
        // Golden test: the same reference resolves to the geometrically same edge after the
        // width changes, even though nothing about indices is assumed.
        let r = EdgeRef::parse("edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))").unwrap();
        for w in [40.0, 10.0, 123.4] {
            let k = MockKernel;
            let res = k
                .extrude(
                    &rect_profile(w, 20.0, ["l5", "l6", "l7", "l8"]),
                    &ExtrudeSpec { distance: 10.0, symmetric: false },
                )
                .unwrap();
            let t = k.topology(&res.shape).unwrap();
            let n = name_body(
                &OpContext::Sweep { feature: "Extrude1", sketch: "Sketch1", regions: &[] },
                &res.face_origins,
                &t,
            )
            .unwrap();
            let Resolution::Resolved(i) = n.resolve_edge(&r) else { panic!("unresolved") };
            let e = &t.edges[i as usize];
            // l6 is the right side (x = w); top cap is z = 10.
            assert!((e.midpoint[0] - w).abs() < 1e-9 && (e.midpoint[2] - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn references_survive_sketch_reordering() {
        // Entity ids, not profile order, drive names: rotating the loop start must not
        // change what a reference resolves to.
        let k = MockKernel;
        let pts = [[0.0, 20.0], [0.0, 0.0], [40.0, 0.0], [40.0, 20.0]];
        let ids = ["l8", "l5", "l6", "l7"];
        let edges = (0..4)
            .map(|i| ProfileEdge {
                seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] },
                tag: entity_tag(ids[i]).unwrap(),
            })
            .collect();
        let prof = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
        let res = k.extrude(&prof, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
        let t = k.topology(&res.shape).unwrap();
        let n = name_body(
            &OpContext::Sweep { feature: "Extrude1", sketch: "Sketch1", regions: &[] },
            &res.face_origins,
            &t,
        )
        .unwrap();
        let r = EdgeRef::parse("edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))").unwrap();
        let Resolution::Resolved(i) = n.resolve_edge(&r) else { panic!() };
        assert!((t.edges[i as usize].midpoint[0] - 40.0).abs() < 1e-9);
    }

    #[test]
    fn missing_reference_explains_and_suggests() {
        let n = named_box(40.0);
        let r = EdgeRef::parse("edge(Extrude1/cap(end), Extrude1/side(Sketch1/l99))").unwrap();
        match n.resolve_edge(&r) {
            Resolution::Missing { reason, suggestion } => {
                assert!(reason.contains("side(Sketch1/l99)"), "{reason}");
                assert!(suggestion.unwrap().faces.iter().any(|f| f.to_string() == "Extrude1/cap(end)"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn duplicate_names_get_geometric_indices() {
        // Two faces with the same origin (as after a split) get #1/#2 ordered by position.
        let topo = Topology {
            faces: vec![
                FaceInfo { kind: SurfaceKind::Plane, area: 1.0, centroid: [5.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] },
                FaceInfo { kind: SurfaceKind::Plane, area: 1.0, centroid: [1.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] },
            ],
            ..Topology::default()
        };
        let o = vec![vec![FaceOrigin::Lateral { tag: entity_tag("l1").unwrap() }]; 2];
        let n = name_body(&OpContext::Sweep { feature: "E", sketch: "S", regions: &[] }, &o, &topo).unwrap();
        assert_eq!(n.faces[1].index, Some(1));
        assert_eq!(n.faces[0].index, Some(2));
    }

    #[test]
    fn modify_carries_names_and_blends() {
        let base = named_box(40.0);
        // Simulated fillet of edge 4 (top edge of l5): output faces = 6 inputs + 1 blend.
        let mut origins: Vec<Vec<FaceOrigin>> = (0..6).map(|f| vec![FaceOrigin::Input { input: 0, face: f }]).collect();
        origins.push(vec![FaceOrigin::FromEdge { input: 0, edge: 4 }]);
        let mut topo = Topology::default();
        for i in 0..7 {
            topo.faces.push(FaceInfo {
                kind: SurfaceKind::Plane,
                area: 1.0,
                centroid: [i as f64, 0.0, 0.0],
                normal: [0.0; 3],
            });
        }
        let n = name_body(&OpContext::Modify { feature: "Fillet1", inputs: &[&base] }, &origins, &topo).unwrap();
        assert_eq!(n.faces[2], base.faces[2]);
        assert_eq!(n.faces[6].to_string(), format!("Fillet1/blend({})", base.edges[4]));
    }

    #[test]
    fn imported_faces_need_no_history() {
        let f =
            |x: f64| FaceInfo { kind: SurfaceKind::Plane, area: 1.0, centroid: [x, 0.0, 0.0], normal: [0.0, 0.0, 1.0] };
        let topo = Topology { faces: vec![f(0.0), f(1.0)], ..Default::default() };
        let n = name_body(&OpContext::Import { feature: "Import1" }, &[vec![], vec![]], &topo).unwrap();
        assert_eq!(n.faces.len(), 2);
        assert_ne!(n.faces[0], n.faces[1]);
    }

    #[test]
    fn count_mismatch_is_an_error() {
        let topo = Topology::default();
        assert!(name_body(&OpContext::Import { feature: "I" }, &[vec![]], &topo).is_err());
    }
}
