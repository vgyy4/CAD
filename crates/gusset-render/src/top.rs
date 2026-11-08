//! A bounding-volume hierarchy over the scene's meshes (their world boxes), so a ray meets
//! the few bodies near it instead of testing every body's box: the path tracer casts
//! millions of rays through scenes of hundreds of bodies (PERFORMANCE.md).

use crate::math::{Aabb, Vec3};
use crate::scene::MeshId;

/// Leaves hold at most this many meshes.
const LEAF: usize = 4;

#[derive(Debug, Clone)]
enum Node {
    /// `items[start..end]`.
    Leaf { bounds: Aabb, start: usize, end: usize },
    /// Children at these node indices.
    Inner { bounds: Aabb, left: usize, right: usize },
}

impl Node {
    fn bounds(&self) -> &Aabb {
        match self {
            Node::Leaf { bounds, .. } | Node::Inner { bounds, .. } => bounds,
        }
    }
}

/// The hierarchy: built once per scene state, read by every ray.
#[derive(Debug, Clone, Default)]
pub(crate) struct TopBvh {
    nodes: Vec<Node>,
    items: Vec<(MeshId, Aabb)>,
}

impl TopBvh {
    /// Builds over `(id, world box)` pairs; empty boxes are left out.
    pub(crate) fn build(mut items: Vec<(MeshId, Aabb)>) -> TopBvh {
        items.retain(|(_, b)| b.min[0] <= b.max[0]);
        let mut t = TopBvh { nodes: Vec::with_capacity(items.len() / 2 + 1), items };
        if !t.items.is_empty() {
            let n = t.items.len();
            t.split(0, n);
        }
        t
    }

    /// Builds the node for `items[start..end]` and returns its index.
    fn split(&mut self, start: usize, end: usize) -> usize {
        let slice = self.items.get(start..end).unwrap_or_default();
        let bounds = slice.iter().fold(Aabb::EMPTY, |b, (_, x)| b.union(x));
        let at = self.nodes.len();
        if end - start <= LEAF {
            self.nodes.push(Node::Leaf { bounds, start, end });
            return at;
        }
        // Median split of the box centres along the longest axis of their spread.
        let mut spread = Aabb::EMPTY;
        for (_, b) in slice {
            spread.include(b.center());
        }
        let [sx, sy, sz] =
            [0, 1, 2].map(|k| spread.max.get(k).copied().unwrap_or(0.0) - spread.min.get(k).copied().unwrap_or(0.0));
        let axis = if sx >= sy && sx >= sz {
            0
        } else if sy >= sz {
            1
        } else {
            2
        };
        let mid = start + (end - start) / 2;
        if let Some(s) = self.items.get_mut(start..end) {
            s.select_nth_unstable_by(mid - start, |(_, a), (_, b)| {
                let c = |x: &Aabb| x.center().get(axis).copied().unwrap_or(0.0);
                c(a).total_cmp(&c(b))
            });
        }
        self.nodes.push(Node::Leaf { bounds, start, end }); // placeholder, replaced below
        let left = self.split(start, mid);
        let right = self.split(mid, end);
        if let Some(n) = self.nodes.get_mut(at) {
            *n = Node::Inner { bounds, left, right };
        }
        at
    }

    /// Calls `visit` for every mesh whose box the ray from `o` along `d` meets within the
    /// limit `visit` returns (the closest hit so far: farther boxes are skipped).
    pub(crate) fn visit(&self, o: Vec3, d: Vec3, mut limit: f64, mut visit: impl FnMut(MeshId) -> f64) {
        let mut stack = Vec::with_capacity(64);
        if !self.nodes.is_empty() {
            stack.push(0usize);
        }
        while let Some(i) = stack.pop() {
            let Some(node) = self.nodes.get(i) else { continue };
            if !crate::pick::ray_meets_box(o, d, node.bounds(), limit) {
                continue;
            }
            match node {
                Node::Leaf { start, end, .. } => {
                    for (id, b) in self.items.get(*start..*end).unwrap_or_default() {
                        if crate::pick::ray_meets_box(o, d, b, limit) {
                            limit = limit.min(visit(*id));
                        }
                    }
                }
                Node::Inner { left, right, .. } => {
                    stack.push(*right);
                    stack.push(*left);
                }
            }
        }
    }
}
