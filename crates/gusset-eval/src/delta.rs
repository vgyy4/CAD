//! What one feature changed in the evaluation state, cached under the feature's
//! dependency key (ADR 0024 §2). When an earlier feature changes that this one does not
//! depend on, the prefix cache misses but the delta can be replayed onto the new running
//! state instead of building the feature again.

use crate::budget::Work;
use crate::{Body, ErrorCode, FeatureStatus, MeshBody, Solved, State};
use gusset_kernel::Frame;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct Delta {
    /// Bodies replaced in place, by the feature that created them (`Body::feature`).
    replaced: Vec<(String, Arc<Body>)>,
    /// Bodies removed (combine tools), by creating feature.
    removed: Vec<String>,
    appended: Vec<Arc<Body>>,
    meshes: Vec<Arc<MeshBody>>,
    solved: Option<Arc<Solved>>,
    plane: Option<Frame>,
    status: FeatureStatus,
    failed: bool,
    work_before: Work,
    work_after: Work,
}

fn ptr<T>(a: &Arc<T>) -> usize {
    Arc::as_ptr(a) as *const () as usize
}

impl Delta {
    /// The change from `before` to `after` made by feature `name`, if it can be expressed
    /// as a delta (checked by replaying it onto `before`).
    pub(crate) fn diff(before: &State, after: &State, name: &str) -> Option<Delta> {
        let (b, a) = (&before.bodies, &after.bodies);
        let in_before: HashSet<usize> = b.iter().map(ptr).collect();
        let in_after: HashSet<usize> = a.iter().map(ptr).collect();
        let (mut replaced, mut removed) = (Vec::new(), Vec::new());
        let (mut i, mut j) = (0, 0);
        while let Some(old) = b.get(i) {
            let new = a.get(j);
            if new.is_some_and(|n| Arc::ptr_eq(n, old)) {
                i += 1;
                j += 1;
                continue;
            }
            if in_after.contains(&ptr(old)) {
                return None; // reordered: not a delta
            }
            match new {
                Some(n) if !in_before.contains(&ptr(n)) && n.feature == old.feature => {
                    replaced.push((n.feature.clone(), n.clone()));
                    j += 1;
                }
                _ => removed.push(old.feature.clone()),
            }
            i += 1;
        }
        let appended: Vec<Arc<Body>> = a.get(j..).unwrap_or_default().to_vec();
        if appended.iter().any(|n| in_before.contains(&ptr(n))) {
            return None;
        }
        // Joint records hold the placements a later loop may change: a joint's change is
        // kept as a full state, not a delta.
        if !Arc::ptr_eq(&before.joints, &after.joints) {
            return None;
        }
        let (bm, am) = (&before.meshes, &after.meshes);
        if am.len() < bm.len() || !bm.iter().zip(am).all(|(x, y)| Arc::ptr_eq(x, y)) {
            return None;
        }
        let solved = match (before.solved.get(name), after.solved.get(name)) {
            (None, Some(s)) => Some(s.clone()),
            (Some(x), Some(y)) if Arc::ptr_eq(x, y) => None,
            (None, None) => None,
            _ => return None,
        };
        let plane = match (before.planes.get(name), after.planes.get(name)) {
            (None, Some(p)) => Some(*p),
            (None, None) => None,
            _ => return None,
        };
        let status = match after.statuses.newest() {
            Some((n, s)) if n == name => s.clone(),
            _ => return None,
        };
        let failed = after.failed.contains(name) && !before.failed.contains(name);
        let d = Delta {
            replaced,
            removed,
            appended,
            meshes: am.get(bm.len()..).unwrap_or_default().to_vec(),
            solved,
            plane,
            status,
            failed,
            work_before: before.work,
            work_after: after.work,
        };
        // The check that makes the diff safe: replaying it onto `before` gives `after`.
        let mut again = before.clone();
        d.replay(&mut again, name);
        let same = again.bodies.len() == a.len() && again.bodies.iter().zip(a).all(|(x, y)| Arc::ptr_eq(x, y));
        same.then_some(d)
    }

    /// Whether replaying is equivalent to building the feature on a state whose work so
    /// far is `work`: the work limits (ADR 0020) see the same totals, or the feature
    /// charged nothing and did not fail on a limit.
    pub(crate) fn replayable(&self, work: &Work, limits: &crate::EvalLimits) -> bool {
        if *work == self.work_before {
            return true;
        }
        // The feature's own charge does not depend on the work before it (ADR 0020), only
        // whether the totals stay within the limits: the same totals as building it here.
        let limit_failure = matches!(&self.status, FeatureStatus::Failed(e) if e.code == ErrorCode::TooComplex);
        let faces = work.faces.saturating_add(self.work_after.faces.saturating_sub(self.work_before.faces));
        let tools = work.tools.saturating_add(self.work_after.tools.saturating_sub(self.work_before.tools));
        !limit_failure && faces <= limits.max_boolean_work && tools <= limits.max_tools
    }

    /// Applies the change to `s` (the state before feature `name`).
    pub(crate) fn replay(&self, s: &mut State, name: &str) {
        for (feature, body) in &self.replaced {
            if let Some(slot) = s.bodies.iter_mut().find(|b| b.feature == *feature) {
                *slot = body.clone();
            }
        }
        for feature in &self.removed {
            if let Some(i) = s.bodies.iter().position(|b| b.feature == *feature) {
                s.bodies.remove(i);
            }
        }
        s.bodies.extend(self.appended.iter().cloned());
        s.meshes.extend(self.meshes.iter().cloned());
        if let Some(sol) = &self.solved {
            s.solved.insert(name.to_string(), sol.clone());
        }
        if let Some(p) = self.plane {
            s.planes.push(name, p);
        }
        if self.failed {
            s.failed.insert(name.to_string());
        }
        s.statuses.push(name.to_string(), self.status.clone());
        s.work = Work {
            faces: s.work.faces.saturating_add(self.work_after.faces.saturating_sub(self.work_before.faces)),
            tools: s.work.tools.saturating_add(self.work_after.tools.saturating_sub(self.work_before.tools)),
        };
    }

    /// Body and mesh allocations this delta holds, for the cache's memory accounting.
    pub(crate) fn refs(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        let bodies = self.replaced.iter().map(|(_, b)| b).chain(&self.appended);
        bodies.map(|b| (ptr(b), crate::body_bytes(b))).chain(self.meshes.iter().map(|m| (ptr(m), crate::mesh_bytes(m))))
    }
}
