//! Persistent topological naming (ADR 0003).
//!
//! Turns kernel operation history ([`FaceOrigin`]) into stable [`FaceRef`] names, names
//! edges by their adjacent faces, and resolves stored [`EdgeRef`]s back to edge indices
//! with explicit `Resolved` / `Ambiguous` / `Missing` outcomes. Nothing here depends on
//! a particular kernel; it is tested with the mock kernel.

#![cfg_attr(test, allow(clippy::float_cmp))]

use gusset_doc::{CapSide, EdgeRef, FaceKey, FaceRef, HolePart};
use gusset_kernel::{CapEnd, FaceOrigin, Topology, V3};
use std::collections::BTreeMap;

// ------------------------------------------------------------------ entity tags

/// Encodes a sketch entity id (`l3`, or copy 2 of it made by a sketch pattern, `l3_2`) as a
/// kernel profile tag: number in bits 0..32, prefix letter in 32..40, copy number in 40..52
/// (0 = the entity itself), spline segment in 52..64 (see [`with_segment`]).
pub fn entity_tag(id: &str) -> Option<u64> {
    let mut c = id.chars();
    let p = c.next()?;
    if !p.is_ascii_lowercase() {
        return None;
    }
    let (num, copy) = match c.as_str().split_once('_') {
        Some((n, k)) => (n, k.parse::<u64>().ok().filter(|k| (1..0x1000).contains(k))?),
        None => (c.as_str(), 0),
    };
    let n: u32 = num.parse().ok()?;
    Some((copy << 40) | (u64::from(p as u8) << 32) | u64::from(n))
}

/// Tag of Bézier segment `seg` of a spline whose entity tag is `tag` (`s9` → `s9_g3`):
/// every segment of a spline sweeps its own lateral face. Stored in the top 12 bits
/// (`seg + 1`; splines have at most 1000 segments).
pub fn with_segment(tag: u64, seg: u32) -> u64 {
    (tag & !(0xfff_u64 << 52)) | (u64::from(seg.min(0xffe) + 1) << 52)
}

/// Decodes a profile tag back into a sketch entity id (`l5`, a spline segment `s9_g3`).
pub fn tag_entity(tag: u64) -> Option<String> {
    let low = tag & 0xff_ffff_ffff;
    let p = u8::try_from(low >> 32).ok()?;
    if !p.is_ascii_lowercase() {
        return None;
    }
    let seg = tag >> 52;
    let copy = (tag >> 40) & 0xfff;
    let mut id = format!("{}{}", p as char, low & 0xffff_ffff);
    if copy > 0 {
        id.push_str(&format!("_{copy}"));
    }
    if seg > 0 {
        id.push_str(&format!("_g{}", seg - 1));
    }
    Some(id)
}

/// The profile tag of a hole tool's edge that makes face `part` (ADR 0019).
pub fn hole_tag(part: HolePart) -> u64 {
    HolePart::ALL.iter().position(|p| *p == part).map_or(u64::MAX, |i| i as u64)
}

/// The face part a hole tool's profile tag stands for.
pub fn tag_hole_part(tag: u64) -> Option<HolePart> {
    HolePart::ALL.get(usize::try_from(tag).ok()?).copied()
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
    /// A loft through the profiles of `sketches` (in order) by `feature`: lateral faces are
    /// named after the section edge that generated them, `side(<sketch>/<entity>)`.
    Loft { feature: &'a str, sketches: &'a [String] },
    /// A combined operation: sweep then boolean. Input 0 is the target body, input 1
    /// the freshly swept tool (whose own origins are named as a sweep).
    SweepInto { feature: &'a str, sketch: &'a str, regions: &'a [String], target: &'a BodyNames },
    /// The tool of one hole of a `hole` feature, a revolved profile whose edge tags are
    /// [`hole_tag`]s: faces are named `feature/<part>(at)`, `at` being the id of the hole.
    Hole { feature: &'a str, at: &'a str },
    /// An imported body; faces are named by index.
    Import { feature: &'a str },
    /// A shell of the body whose names are `input`: surviving faces keep their names,
    /// the inner wall faces become `feature/inner(<face>)`.
    Shell { feature: &'a str, input: &'a BodyNames },
    /// A mirrored copy of a body: face *i* is `feature/mirror(<face i>)`.
    Mirror { feature: &'a str, input: &'a BodyNames },
    /// A moved copy of a body (`move` with `copy: true`): face *i* is
    /// `feature/copy(<face i>)`. A plain move keeps names and uses [`OpContext::Modify`].
    Copy { feature: &'a str, input: &'a BodyNames },
    /// Instance `k` (1-based) of a pattern: face *i* is `feature/instance(k, <face i>)`.
    Instance { feature: &'a str, k: u32, input: &'a BodyNames },
}

/// Names of every face and edge of one body.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BodyNames {
    pub faces: Vec<FaceRef>,
    pub edges: Vec<EdgeRef>,
    /// Per vertex: the smallest name among its edges. A corner blend made at that
    /// vertex is named after it (review L3).
    pub corners: Vec<Option<EdgeRef>>,
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
        FaceOrigin::Lateral { .. } | FaceOrigin::Swept { .. } | FaceOrigin::Section { .. } | FaceOrigin::Cap { .. } => {
            0
        }
        FaceOrigin::FromEdge { .. } | FaceOrigin::FromVertex { .. } | FaceOrigin::Offset { .. } => 1,
        FaceOrigin::Input { .. } => 2,
    }
}

fn sweep_face(feature: &str, sketch: &str, regions: &[String], o: &FaceOrigin) -> Option<FaceRef> {
    match o {
        FaceOrigin::Lateral { tag } => Some(FaceRef::new(
            feature,
            FaceKey::Side { sketch: sketch.to_string(), entity: tag_entity(*tag).unwrap_or_else(|| format!("t{tag}")) },
        )),
        // A sweep along a path of several edges makes one face per path edge: the first keeps
        // the plain name, later ones get `_p<k>` (k = path edge, counted from the path's start).
        FaceOrigin::Swept { tag, path } => {
            let mut entity = tag_entity(*tag).unwrap_or_else(|| format!("t{tag}"));
            if *path > 0 {
                entity.push_str(&format!("_p{path}"));
            }
            Some(FaceRef::new(feature, FaceKey::Side { sketch: sketch.to_string(), entity }))
        }
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
    // A loft face can come from the edges of several sections (and a section edge can feed
    // several faces when edge counts differ): a face takes the origin shared with the fewest
    // other faces, so names stay as specific as possible.
    let mut shared: BTreeMap<(u32, u64), usize> = BTreeMap::new();
    if matches!(ctx, OpContext::Loft { .. }) {
        for os in origins {
            for o in os {
                if let FaceOrigin::Section { section, tag } = o {
                    *shared.entry((*section, *tag)).or_insert(0) += 1;
                }
            }
        }
    }
    for (fi, os) in origins.iter().enumerate() {
        let mut sorted = os.clone();
        sorted.sort_by_key(|o| {
            let n = match o {
                FaceOrigin::Section { section, tag } => shared.get(&(*section, *tag)).copied().unwrap_or(0),
                _ => 0,
            };
            (origin_rank(o), n)
        });
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
                (OpContext::Loft { feature, sketches }, FaceOrigin::Section { section, tag }) => {
                    sketches.get(*section as usize).map(|sketch| {
                        FaceRef::new(
                            *feature,
                            FaceKey::Side {
                                sketch: sketch.clone(),
                                entity: tag_entity(*tag).unwrap_or_else(|| format!("t{tag}")),
                            },
                        )
                    })
                }
                (OpContext::Hole { feature, at }, FaceOrigin::Lateral { tag }) => tag_hole_part(*tag)
                    .map(|part| FaceRef::new(*feature, FaceKey::Hole { part, at: (*at).to_string() })),
                (OpContext::Loft { feature, .. }, FaceOrigin::Cap { end, .. }) => {
                    Some(FaceRef::new(*feature, FaceKey::Cap { side: cap(*end), region: None }))
                }
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
                (OpContext::Shell { input, .. }, FaceOrigin::Input { input: 0, face }) => {
                    input.faces.get(*face as usize).cloned()
                }
                (OpContext::Shell { feature, input }, FaceOrigin::Offset { input: 0, face }) => {
                    input.faces.get(*face as usize).map(|f| FaceRef::new(*feature, FaceKey::Inner(Box::new(f.clone()))))
                }
                (OpContext::Shell { feature, input }, FaceOrigin::FromEdge { input: 0, edge }) => {
                    input.edges.get(*edge as usize).map(|e| FaceRef::new(*feature, FaceKey::Blend(Box::new(e.clone()))))
                }
                (OpContext::Shell { feature, input }, FaceOrigin::FromVertex { input: 0, vertex }) => {
                    corner_edge(input, *vertex).map(|e| FaceRef::new(*feature, FaceKey::Blend(Box::new(e))))
                }
                (OpContext::Mirror { feature, input }, FaceOrigin::Input { input: 0, face }) => input
                    .faces
                    .get(*face as usize)
                    .map(|f| FaceRef::new(*feature, FaceKey::Mirror(Box::new(f.clone())))),
                (OpContext::Copy { feature, input }, FaceOrigin::Input { input: 0, face }) => {
                    input.faces.get(*face as usize).map(|f| FaceRef::new(*feature, FaceKey::Copy(Box::new(f.clone()))))
                }
                (OpContext::Instance { feature, k, input }, FaceOrigin::Input { input: 0, face }) => input
                    .faces
                    .get(*face as usize)
                    .map(|f| FaceRef::new(*feature, FaceKey::Instance { k: *k, face: Box::new(f.clone()) })),
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
    // The smallest-named edge at each vertex (each name formatted once: names nest, so
    // formatting them per comparison was quadratic in large patterned bodies).
    let texts: Vec<String> = edges.iter().map(ToString::to_string).collect();
    let mut best: Vec<Option<usize>> = vec![None; topo.vertices.len()];
    for (i, info) in topo.edges.iter().enumerate().take(edges.len()) {
        for &v in &info.vertices {
            if let Some(slot) = best.get_mut(v as usize)
                && slot.and_then(|c| texts.get(c)).is_none_or(|c| texts.get(i).is_some_and(|t| t < c))
            {
                *slot = Some(i);
            }
        }
    }
    let corners = best.into_iter().map(|b| b.and_then(|i| edges.get(i).cloned())).collect();
    Ok(BodyNames { faces, edges, corners })
}

fn corner_edge(b: &BodyNames, vertex: u32) -> Option<EdgeRef> {
    // The smallest-named edge at that vertex; the two corners at the ends of one edge
    // are then told apart by the geometric `#k` index. Bodies named without vertex
    // information fall back to the smallest edge name overall.
    b.corners.get(vertex as usize).cloned().flatten().or_else(|| b.edges.iter().min_by_key(|e| e.to_string()).cloned())
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
        for id in ["l3", "p1", "a42", "c4294967295", "l3_2", "c7_499"] {
            assert_eq!(tag_entity(entity_tag(id).unwrap()).unwrap(), id);
        }
        assert!(entity_tag("L3").is_none());
        assert!(entity_tag("l").is_none());
        assert!(entity_tag("l3_0").is_none() && entity_tag("l3_x").is_none());
        let seg = with_segment(entity_tag("s9").unwrap(), 3);
        assert_eq!(tag_entity(seg).unwrap(), "s9_g3");
        assert_eq!(tag_entity(with_segment(entity_tag("s9_2").unwrap(), 0)).unwrap(), "s9_2_g0");
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
    fn corner_blends_are_named_after_an_edge_at_their_vertex() {
        // Review L3: two corner blends at different vertices get different names, each
        // from an edge that meets its own vertex.
        let mut base = named_box(40.0);
        let pick = |i: usize| base.edges.get(i).cloned();
        base.corners = vec![pick(0), pick(5)];
        let mut origins: Vec<Vec<FaceOrigin>> = (0..6).map(|f| vec![FaceOrigin::Input { input: 0, face: f }]).collect();
        origins.push(vec![FaceOrigin::FromVertex { input: 0, vertex: 0 }]);
        origins.push(vec![FaceOrigin::FromVertex { input: 0, vertex: 1 }]);
        let mut topo = Topology::default();
        for i in 0..8 {
            topo.faces.push(FaceInfo {
                kind: SurfaceKind::Plane,
                area: 1.0,
                centroid: [i as f64, 0.0, 0.0],
                normal: [0.0; 3],
            });
        }
        let n = name_body(&OpContext::Modify { feature: "Fillet1", inputs: &[&base] }, &origins, &topo).unwrap();
        assert_eq!(n.faces[6].to_string(), format!("Fillet1/blend({})", base.edges[0]));
        assert_eq!(n.faces[7].to_string(), format!("Fillet1/blend({})", base.edges[5]));
    }

    #[test]
    fn corners_come_from_incident_edges() {
        let k = MockKernel;
        let res = k
            .extrude(
                &rect_profile(40.0, 20.0, ["l5", "l6", "l7", "l8"]),
                &ExtrudeSpec { distance: 10.0, symmetric: false },
            )
            .unwrap();
        let t = k.topology(&res.shape).unwrap();
        let n =
            name_body(&OpContext::Sweep { feature: "E", sketch: "S", regions: &[] }, &res.face_origins, &t).unwrap();
        assert!(t.vertices.len() >= 8, "the mock box has its vertices");
        assert_eq!(n.corners.len(), t.vertices.len());
        for (v, c) in n.corners.iter().enumerate() {
            let c = c.as_ref().unwrap();
            let ei = n.edges.iter().position(|e| e == c).unwrap();
            assert!(t.edges[ei].vertices.contains(&(v as u32)), "corner {v} named after a far edge");
        }
    }

    #[test]
    fn transform_and_shell_names() {
        let k = MockKernel;
        let base = named_box(40.0);
        let b = k
            .extrude(
                &rect_profile(40.0, 20.0, ["l5", "l6", "l7", "l8"]),
                &ExtrudeSpec { distance: 10.0, symmetric: false },
            )
            .unwrap();
        let mir = k.transform(&b.shape, &Transform::Mirror { origin: [0.0; 3], normal: [1.0, 0.0, 0.0] }).unwrap();
        let t = k.topology(&mir.shape).unwrap();
        let n = name_body(&OpContext::Mirror { feature: "Mirror1", input: &base }, &mir.face_origins, &t).unwrap();
        assert_eq!(n.faces[0].to_string(), "Mirror1/mirror(Extrude1/cap(start))");
        assert_eq!(n.faces[2].to_string(), "Mirror1/mirror(Extrude1/side(Sketch1/l5))");
        assert_eq!(n.edges.len(), 12);
        let moved = k.transform(&b.shape, &Transform::Translate([5.0, 0.0, 0.0])).unwrap();
        let t2 = k.topology(&moved.shape).unwrap();
        let c = name_body(&OpContext::Copy { feature: "Move1", input: &base }, &moved.face_origins, &t2).unwrap();
        assert_eq!(c.faces[1].to_string(), "Move1/copy(Extrude1/cap(end))");
        let same =
            name_body(&OpContext::Modify { feature: "Move1", inputs: &[&base] }, &moved.face_origins, &t2).unwrap();
        assert_eq!(same.faces, base.faces, "a plain move keeps every name");
        let inst = name_body(&OpContext::Instance { feature: "P", k: 2, input: &base }, &mir.face_origins, &t).unwrap();
        assert_eq!(inst.faces[1].to_string(), "P/instance(2, Extrude1/cap(end))");
        // Shell: 6 survivors + 5 inner faces; the inner face of cap(end) is named after it.
        let mut origins: Vec<Vec<FaceOrigin>> = (0..6).map(|f| vec![FaceOrigin::Input { input: 0, face: f }]).collect();
        origins.extend((0..5).map(|f| vec![FaceOrigin::Offset { input: 0, face: f }]));
        let mut topo = Topology::default();
        for i in 0..11 {
            topo.faces.push(FaceInfo {
                kind: SurfaceKind::Plane,
                area: 1.0,
                centroid: [i as f64, 0.0, 0.0],
                normal: [0.0; 3],
            });
        }
        let n = name_body(&OpContext::Shell { feature: "Shell1", input: &base }, &origins, &topo).unwrap();
        assert_eq!(n.faces[1], base.faces[1]);
        assert_eq!(n.faces[7].to_string(), "Shell1/inner(Extrude1/cap(end))");
        // Names survive a print/parse round trip.
        for f in &n.faces {
            assert_eq!(&FaceRef::parse(&f.to_string()).unwrap(), f);
        }
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

    fn flat_topo(n: usize) -> Topology {
        let faces = (0..n)
            .map(|i| FaceInfo {
                kind: SurfaceKind::Plane,
                area: 1.0,
                centroid: [i as f64, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            })
            .collect();
        Topology { faces, ..Default::default() }
    }

    #[test]
    fn sweep_faces_get_path_edge_suffixes() {
        let l5 = entity_tag("l5").unwrap();
        let s9 = with_segment(entity_tag("s9").unwrap(), 2);
        let origins = vec![
            vec![FaceOrigin::Cap { end: CapEnd::Start, region: 0 }],
            vec![FaceOrigin::Cap { end: CapEnd::End, region: 0 }],
            vec![FaceOrigin::Swept { tag: l5, path: 0 }],
            vec![FaceOrigin::Swept { tag: l5, path: 1 }],
            vec![FaceOrigin::Swept { tag: l5, path: 2 }],
            vec![FaceOrigin::Swept { tag: s9, path: 1 }],
        ];
        let ctx = OpContext::Sweep { feature: "Sweep1", sketch: "Prof", regions: &["l5".into()] };
        let n = name_body(&ctx, &origins, &flat_topo(6)).unwrap();
        let names: Vec<String> = n.faces.iter().map(ToString::to_string).collect();
        assert_eq!(
            names,
            [
                "Sweep1/cap(start)",
                "Sweep1/cap(end)",
                "Sweep1/side(Prof/l5)",
                "Sweep1/side(Prof/l5_p1)",
                "Sweep1/side(Prof/l5_p2)",
                "Sweep1/side(Prof/s9_g2_p1)"
            ]
        );
        for f in &n.faces {
            assert_eq!(&FaceRef::parse(&f.to_string()).unwrap(), f);
        }
    }

    #[test]
    fn loft_faces_take_the_most_specific_origin() {
        let (c3, l5, l6) = (entity_tag("c3").unwrap(), entity_tag("l5").unwrap(), entity_tag("l6").unwrap());
        let sketches = vec!["Round".to_string(), "Square".to_string()];
        // A circle lofted to a square: the circle's one edge feeds both side faces, each
        // square edge only one. Faces are named after the square's edges.
        let origins = vec![
            vec![FaceOrigin::Cap { end: CapEnd::Start, region: 0 }],
            vec![FaceOrigin::Cap { end: CapEnd::End, region: 0 }],
            vec![FaceOrigin::Section { section: 0, tag: c3 }, FaceOrigin::Section { section: 1, tag: l5 }],
            vec![FaceOrigin::Section { section: 0, tag: c3 }, FaceOrigin::Section { section: 1, tag: l6 }],
        ];
        let ctx = OpContext::Loft { feature: "Loft1", sketches: &sketches };
        let n = name_body(&ctx, &origins, &flat_topo(4)).unwrap();
        let names: Vec<String> = n.faces.iter().map(ToString::to_string).collect();
        assert_eq!(names, ["Loft1/cap(start)", "Loft1/cap(end)", "Loft1/side(Square/l5)", "Loft1/side(Square/l6)"]);
        // Equal specificity: the first section wins; a shared origin gets #k indices.
        let origins = vec![
            vec![FaceOrigin::Section { section: 0, tag: l5 }, FaceOrigin::Section { section: 1, tag: l5 }],
            vec![FaceOrigin::Section { section: 0, tag: l5 }, FaceOrigin::Section { section: 1, tag: l5 }],
        ];
        let n = name_body(&ctx, &origins, &flat_topo(2)).unwrap();
        let names: Vec<String> = n.faces.iter().map(ToString::to_string).collect();
        assert_eq!(names, ["Loft1/side(Round/l5)#1", "Loft1/side(Round/l5)#2"]);
        // An origin outside the section list cannot be named: a clean error.
        let bad = vec![vec![FaceOrigin::Section { section: 5, tag: l5 }]];
        assert!(name_body(&ctx, &bad, &flat_topo(1)).is_err());
    }

    #[test]
    fn hole_tool_faces_are_named_after_their_part_and_site() {
        let origins: Vec<Vec<FaceOrigin>> = [HolePart::Top, HolePart::Wall, HolePart::Floor, HolePart::CboreFloor]
            .iter()
            .map(|p| vec![FaceOrigin::Lateral { tag: hole_tag(*p) }])
            .collect();
        let ctx = OpContext::Hole { feature: "Hole1", at: "p5_2" };
        let n = name_body(&ctx, &origins, &flat_topo(4)).unwrap();
        let names: Vec<String> = n.faces.iter().map(ToString::to_string).collect();
        assert_eq!(names, ["Hole1/top(p5_2)", "Hole1/wall(p5_2)", "Hole1/floor(p5_2)", "Hole1/cbore_floor(p5_2)"]);
        for p in HolePart::ALL {
            assert_eq!(tag_hole_part(hole_tag(p)), Some(p));
        }
        assert_eq!(tag_hole_part(99), None);
        // The names survive a round trip through the text form.
        for f in &n.faces {
            assert_eq!(&FaceRef::parse(&f.to_string()).unwrap(), f);
        }
        // A cut keeps the target's names and takes the tool's for the new faces.
        let target = BodyNames { faces: vec![FaceRef::parse("E/cap(end)").unwrap()], edges: vec![], corners: vec![] };
        let tool = BodyNames { faces: n.faces.clone(), edges: vec![], corners: vec![] };
        let origins = vec![
            vec![FaceOrigin::Input { input: 0, face: 0 }, FaceOrigin::Input { input: 1, face: 0 }],
            vec![FaceOrigin::Input { input: 1, face: 1 }],
        ];
        let ctx = OpContext::Modify { feature: "Hole1", inputs: &[&target, &tool] };
        let n = name_body(&ctx, &origins, &flat_topo(2)).unwrap();
        let names: Vec<String> = n.faces.iter().map(ToString::to_string).collect();
        assert_eq!(names, ["E/cap(end)", "Hole1/wall(p5_2)"]);
    }

    #[test]
    fn count_mismatch_is_an_error() {
        let topo = Topology::default();
        assert!(name_body(&OpContext::Import { feature: "I" }, &[vec![]], &topo).is_err());
    }
}
