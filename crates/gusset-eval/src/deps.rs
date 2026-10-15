//! Which earlier features each feature depends on (ADR 0024 §1): the basis for
//! dependency-keyed caching and for building independent features in parallel.
//!
//! The analysis is conservative, so a feature never misses a dependency the sequential
//! evaluation would have given it:
//! - Sketches and datum planes never change after their own feature, so a reference to
//!   one depends on that feature only.
//! - A body named explicitly (`body:`, `tools:`) depends on the last feature that created
//!   or changed that body.
//! - Anything resolved against the current bodies at evaluation time — a modifying feature
//!   without `body:` (it works on the most recent body), and edge, face or direction
//!   references (they find the body holding the named faces) — reads *every* body, so
//!   the feature depends on everything before it; and as it may change any body, every
//!   feature after it depends on it.

use gusset_doc::{BodyOp, Document, Feature, FeatureKind};
use std::collections::{BTreeSet, HashMap};

/// The direct dependencies of every feature, as indices of earlier features, ascending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Graph {
    pub(crate) deps: Vec<Vec<usize>>,
}

/// The bodies a feature names explicitly, and whether it also works on bodies found at
/// evaluation time.
struct Access {
    bodies: Vec<String>,
    dynamic: bool,
}

fn modifies(op: BodyOp, body: Option<&String>) -> Access {
    match (op, body) {
        (BodyOp::New, _) => Access { bodies: Vec::new(), dynamic: false },
        (_, Some(b)) => Access { bodies: vec![b.clone()], dynamic: false },
        (_, None) => Access { bodies: Vec::new(), dynamic: true },
    }
}

fn target(body: Option<&String>) -> Access {
    match body {
        Some(b) => Access { bodies: vec![b.clone()], dynamic: false },
        None => Access { bodies: Vec::new(), dynamic: true },
    }
}

fn access(kind: &FeatureKind) -> Access {
    match kind {
        FeatureKind::Sketch(_) | FeatureKind::Plane(_) | FeatureKind::Import(_) | FeatureKind::Component(_) => {
            Access { bodies: Vec::new(), dynamic: false }
        }
        FeatureKind::Extrude(d) => modifies(d.op, d.body.as_ref()),
        FeatureKind::Revolve(d) => modifies(d.op, d.body.as_ref()),
        FeatureKind::Sweep(d) => modifies(d.op, d.body.as_ref()),
        FeatureKind::Loft(d) => modifies(d.op, d.body.as_ref()),
        FeatureKind::Text(d) => modifies(d.op, d.body.as_ref()),
        // The edges decide the body.
        FeatureKind::Fillet(_) | FeatureKind::Chamfer(_) | FeatureKind::Hole(_) => {
            Access { bodies: Vec::new(), dynamic: true }
        }
        FeatureKind::Shell(d) => target(d.body.as_ref()),
        FeatureKind::Draft(d) => target(d.body.as_ref()),
        FeatureKind::OffsetFace(d) => target(d.body.as_ref()),
        FeatureKind::Mirror(d) => target(d.body.as_ref()),
        FeatureKind::PatternLinear(d) => target(d.body.as_ref()),
        FeatureKind::PatternCircular(d) => target(d.body.as_ref()),
        FeatureKind::Move(d) => target(d.body.as_ref()),
        FeatureKind::Joint(d) => {
            Access { bodies: std::iter::once(&d.body).chain(&d.with).cloned().collect(), dynamic: false }
        }
        FeatureKind::Combine(d) => {
            let mut a = target(d.body.as_ref());
            a.bodies.extend(d.tools.iter().cloned());
            a
        }
    }
}

fn defines_geometry_only(f: &Feature) -> bool {
    matches!(f.kind, FeatureKind::Sketch(_) | FeatureKind::Plane(_))
}

/// The dependency graph of `doc`'s features.
pub(crate) fn graph(doc: &Document) -> Graph {
    let mut deps = Vec::with_capacity(doc.features.len());
    // Name -> the feature that defined it (sketches, planes) or last changed it (bodies).
    let mut last_writer: HashMap<String, usize> = HashMap::new();
    let mut sketch_or_plane: HashMap<&str, bool> = HashMap::new();
    // The last feature that may have changed any body.
    let mut barrier: Option<usize> = None;
    // Features no later feature depends on yet: every earlier feature is an ancestor of
    // one of them, so depending on all of them means depending on everything before.
    let mut sinks: BTreeSet<usize> = BTreeSet::new();
    // Bodies a joint has placed: a later joint moving one closes a loop.
    let mut joint_placed: BTreeSet<&str> = BTreeSet::new();
    for (i, f) in doc.features.iter().enumerate() {
        let a = access(&f.kind);
        // A joint the loop solver may change, or one closing a loop, moves bodies other
        // than its own: like a reference resolved at evaluation time, it reads and may
        // change every body (ADR 0026 §1).
        let in_loop = match &f.kind {
            FeatureKind::Joint(d) => {
                let closes = joint_placed.contains(d.body.as_str());
                joint_placed.extend(std::iter::once(d.body.as_str()).chain(d.with.iter().map(String::as_str)));
                closes || !d.free.is_empty()
            }
            _ => false,
        };
        let referenced = f.kind.dependencies();
        // References other than sketches, planes and explicitly named bodies are naming
        // references, resolved against whatever body holds those faces now.
        let naming = referenced
            .iter()
            .any(|n| !sketch_or_plane.get(n.as_str()).copied().unwrap_or(false) && !a.bodies.iter().any(|b| b == n));
        let dynamic = a.dynamic || naming || in_loop;
        let mut d: BTreeSet<usize> = BTreeSet::new();
        if dynamic {
            d.extend(sinks.iter().copied());
        } else {
            d.extend(barrier);
            for n in referenced.iter().chain(&a.bodies).chain(std::iter::once(&f.name)) {
                if let Some(&w) = last_writer.get(n.as_str()) {
                    d.insert(w);
                }
            }
        }
        for x in &d {
            sinks.remove(x);
        }
        sinks.insert(i);
        if dynamic {
            barrier = Some(i);
        }
        last_writer.insert(f.name.clone(), i);
        for b in a.bodies {
            last_writer.insert(b, i);
        }
        sketch_or_plane.insert(f.name.as_str(), defines_geometry_only(f));
        deps.push(d.into_iter().collect());
    }
    Graph { deps }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = "{
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}";

    fn deps(features: &str) -> Vec<Vec<usize>> {
        let text = format!("gusset 1\nunits mm\n{features}");
        let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        graph(&doc).deps
    }

    fn sketch(name: &str) -> String {
        format!("sketch {name} (plane: XY, next_id: 9) {SQUARE}\n")
    }

    #[test]
    fn two_new_bodies_are_independent() {
        let text = [
            sketch("S1"),
            "extrude E1 (profile: S1, distance: 5 mm, op: new)\n".into(),
            sketch("S2"),
            "extrude E2 (profile: S2, distance: 5 mm, op: new)\n".into(),
        ]
        .concat();
        assert_eq!(deps(&text), [vec![], vec![0], vec![], vec![2]]);
    }

    #[test]
    fn explicit_bodies_chain_and_combine_joins() {
        let text = [
            sketch("S1"),
            "extrude E1 (profile: S1, distance: 5 mm, op: new)\n".into(),
            "extrude E2 (profile: S1, distance: 2 mm, op: new)\n".into(),
            "extrude E3 (profile: S1, distance: 9 mm, op: add, body: E1)\n".into(),
            "combine C1 (body: E1, tools: [E2], op: add)\n".into(),
        ]
        .concat();
        assert_eq!(deps(&text), [vec![], vec![0], vec![0], vec![0, 1], vec![2, 3]]);
    }

    #[test]
    fn implicit_targets_and_naming_references_are_barriers() {
        let text = [
            sketch("S1"),
            "extrude E1 (profile: S1, distance: 5 mm, op: new)\n".into(),
            sketch("S2"),
            "extrude E2 (profile: S2, distance: 5 mm, op: new)\n".into(),
            // No `body:`: the most recent body, decided at evaluation time.
            "extrude E3 (profile: S2, distance: 1 mm, op: cut)\n".into(),
            "extrude E4 (profile: S1, distance: 7 mm, op: new)\n".into(),
        ]
        .concat();
        let d = deps(&text);
        // E3 depends on everything before it (through the sinks E1 and E2) ...
        assert_eq!(d.get(4), Some(&vec![1, 3]));
        // ... and everything after it depends on E3.
        assert_eq!(d.get(5), Some(&vec![0, 4]));
    }

    #[test]
    fn a_joint_closing_a_loop_reads_and_changes_every_body() {
        let text = [
            sketch("S1"),
            "extrude E1 (profile: S1, distance: 5 mm, op: new)\n".into(),
            sketch("S2"),
            "extrude E2 (profile: S2, distance: 5 mm, op: new)\n".into(),
            "joint J1 (type: rigid, body: E1, at: origin, to: XY)\n".into(),
            // E1 is placed already: J2 closes a loop and may move any body placed before.
            "joint J2 (type: rigid, body: E1, at: origin, to: XZ)\n".into(),
            "extrude E3 (profile: S1, distance: 7 mm, op: new)\n".into(),
        ]
        .concat();
        let d = deps(&text);
        assert_eq!(d.get(4), Some(&vec![1]), "an ordinary joint depends on its body");
        assert_eq!(d.get(5), Some(&vec![3, 4]), "the closing joint depends on everything");
        assert_eq!(d.get(6), Some(&vec![0, 5]), "and everything after depends on it");
    }
}
