//! History bundles (ADR 0044 phase 1): a branch's node chain and every object it reaches,
//! as one self-checking file that another copy of the project can import.
//!
//! Layout: `GUSB`, version byte, BLAKE3 of the payload (32 bytes), then the zstd-compressed
//! payload. The payload is: source branch name, node count, object count, the nodes root
//! first (source id, source parent or 0, label, actor, time, state, snapshot hash), then
//! the objects (kind, hash, length, bytes). Integers are little-endian. A bundle is
//! untrusted input: every size is bounded and every object re-hashed on import.

use crate::history::{
    Actor, Branch, BranchId, Ctx, Event, HResult, History, HistoryError, MAX_NAME_LEN, Need, Node, NodeId, NodeState,
    check_branch_name,
};
use crate::objects::{Hash, ObjectKind, ObjectStore};
use crate::persist::{branch_line, node_line};
use std::collections::{BTreeSet, HashMap};
use std::io::Read;

const MAGIC: &[u8; 4] = b"GUSB";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 4 + 1 + 32;
const MAX_STR: usize = 64 << 10;
const WINDOW_LOG_MAX: u32 = 25;

/// Bounds applied to a bundle on import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleLimits {
    /// Bytes of the file as given.
    pub max_bundle_bytes: usize,
    /// Bytes after decompression (also bounds the sum of the objects).
    pub max_decompressed_bytes: usize,
    pub max_nodes: usize,
    pub max_objects: usize,
    /// Bytes of one object.
    pub max_object_bytes: usize,
}

impl Default for BundleLimits {
    fn default() -> Self {
        BundleLimits {
            max_bundle_bytes: 512 << 20,
            max_decompressed_bytes: 1 << 30,
            max_nodes: 100_000,
            max_objects: 1_000_000,
            max_object_bytes: 256 << 20,
        }
    }
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleReport {
    pub nodes_added: usize,
    pub nodes_reused: usize,
    pub objects_added: usize,
    pub branch: BranchId,
    pub branch_name: String,
    /// False when an existing branch already pointed at the imported head.
    pub branch_created: bool,
    pub head: NodeId,
}

fn refuse<T>(m: impl Into<String>) -> HResult<T> {
    Err(HistoryError::Bundle(m.into()))
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&u32::try_from(s.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn kind_byte(k: ObjectKind) -> u8 {
    match k {
        ObjectKind::Feature => 0,
        ObjectKind::Params => 1,
        ObjectKind::Snapshot => 2,
        ObjectKind::Blob => 3,
    }
}

fn kind_from(b: u8) -> Option<ObjectKind> {
    Some(match b {
        0 => ObjectKind::Feature,
        1 => ObjectKind::Params,
        2 => ObjectKind::Snapshot,
        3 => ObjectKind::Blob,
        _ => return None,
    })
}

/// A bounds-checked reader over the payload.
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> HResult<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.b.len());
        let Some(end) = end else { return refuse("the bundle is truncated") };
        let s = self.b.get(self.pos..end).unwrap_or(&[]);
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> HResult<u8> {
        Ok(self.take(1)?.first().copied().unwrap_or(0))
    }
    fn u32(&mut self) -> HResult<u32> {
        let mut a = [0u8; 4];
        a.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(a))
    }
    fn u64(&mut self) -> HResult<u64> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn hash(&mut self) -> HResult<Hash> {
        let mut a = [0u8; 32];
        a.copy_from_slice(self.take(32)?);
        Ok(Hash(a))
    }
    fn str(&mut self) -> HResult<String> {
        let n = self.u32()? as usize;
        if n > MAX_STR {
            return refuse("a text field in the bundle is too long");
        }
        match std::str::from_utf8(self.take(n)?) {
            Ok(s) => Ok(s.to_string()),
            Err(_) => refuse("a text field in the bundle is not UTF-8"),
        }
    }
}

struct BNode {
    id: u64,
    parent: u64,
    label: String,
    actor: Actor,
    time: u64,
    state: NodeState,
    snapshot: Hash,
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

impl History {
    /// Writes `branch`'s chain (root to head) and everything it reaches as a `.gusb`
    /// bundle. The history is not changed.
    pub fn export_bundle(&self, branch: BranchId) -> HResult<Vec<u8>> {
        let name = self.branch(branch)?.name.clone();
        self.export_bundle_as(branch, &name)
    }

    /// As [`History::export_bundle`], with `name` written as the source branch name (a
    /// folder sync writes `<who>/<branch>`, so the receiver gets `from/<who>/<branch>`).
    pub fn export_bundle_as(&self, branch: BranchId, name: &str) -> HResult<Vec<u8>> {
        let b = self.branch(branch)?;
        let mut chain = self.lineage(b.head);
        chain.reverse();
        let mut want = BTreeSet::new();
        for id in &chain {
            if !self.objects.snapshot_closure(self.node(*id)?.snapshot, &mut want) {
                return refuse("some history objects of this branch are missing or unreadable");
            }
        }
        let mut p = Vec::new();
        put_str(&mut p, name);
        p.extend_from_slice(&(chain.len() as u32).to_le_bytes());
        p.extend_from_slice(&(want.len() as u32).to_le_bytes());
        for id in &chain {
            let n = self.node(*id)?;
            p.extend_from_slice(&n.id.0.to_le_bytes());
            p.extend_from_slice(&n.parent.map_or(0, |x| x.0).to_le_bytes());
            put_str(&mut p, &n.label);
            match &n.actor {
                Actor::User => p.push(0),
                Actor::Agent(a) => {
                    p.push(1);
                    put_str(&mut p, a);
                }
            }
            p.extend_from_slice(&n.time.to_le_bytes());
            match &n.state {
                NodeState::Ok => p.push(0),
                NodeState::Broken { message } => {
                    p.push(1);
                    put_str(&mut p, message);
                }
            }
            p.extend_from_slice(&n.snapshot.0);
        }
        for h in &want {
            let (Some(kind), Ok(bytes)) = (self.objects.kind(*h), self.objects.get(*h)) else {
                return refuse("a history object is missing");
            };
            p.push(kind_byte(kind));
            p.extend_from_slice(&h.0);
            p.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            p.extend_from_slice(bytes);
        }
        let sum = blake3::hash(&p);
        let z = zstd::stream::encode_all(&p[..], 3)
            .map_err(|e| HistoryError::Bundle(format!("compression failed: {e}")))?;
        let mut out = Vec::with_capacity(HEADER_LEN + z.len());
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(sum.as_bytes());
        out.extend_from_slice(&z);
        Ok(out)
    }

    /// Reads a bundle: objects are stored by hash, nodes are re-created with fresh ids
    /// (reusing a node whose snapshot and remapped parent already exist), and the head
    /// becomes the branch `from/<source name>` (never `main`). Nothing changes when the
    /// bundle is refused.
    pub fn import_bundle(&mut self, ctx: &Ctx, bytes: &[u8], limits: &BundleLimits) -> HResult<BundleReport> {
        if bytes.len() > limits.max_bundle_bytes {
            return refuse(format!("the file is larger than the {} byte limit", limits.max_bundle_bytes));
        }
        if bytes.get(..4) != Some(&MAGIC[..]) {
            return refuse("not a Gusset history bundle");
        }
        if bytes.get(4) != Some(&VERSION) {
            return refuse("unsupported bundle version");
        }
        let (Some(sum), Some(z)) = (bytes.get(5..HEADER_LEN), bytes.get(HEADER_LEN..)) else {
            return refuse("the bundle is truncated");
        };
        let payload = decompress(z, limits)?;
        if blake3::hash(&payload).as_bytes()[..] != *sum {
            return refuse("the checksum does not match: the file is damaged or was changed");
        }
        let mut r = Reader { b: &payload, pos: 0 };
        let src_name = r.str()?;
        let (n_nodes, n_objs) = (r.u32()? as usize, r.u32()? as usize);
        if n_nodes == 0 {
            return refuse("the bundle has no iterations");
        }
        if n_nodes > limits.max_nodes {
            return refuse(format!("{n_nodes} iterations; the limit is {}", limits.max_nodes));
        }
        if n_objs > limits.max_objects {
            return refuse(format!("{n_objs} objects; the limit is {}", limits.max_objects));
        }
        let mut bnodes = Vec::with_capacity(n_nodes.min(4096));
        for _ in 0..n_nodes {
            let id = r.u64()?;
            let parent = r.u64()?;
            let label = clean(&r.str()?, 200);
            let actor = match r.u8()? {
                0 => Actor::User,
                1 => Actor::Agent(clean(&r.str()?, 64)),
                _ => return refuse("unknown actor in the bundle"),
            };
            let time = r.u64()?;
            let state = match r.u8()? {
                0 => NodeState::Ok,
                1 => NodeState::Broken { message: r.str()?.chars().take(4000).collect() },
                _ => return refuse("unknown iteration state in the bundle"),
            };
            bnodes.push(BNode { id, parent, label, actor, time, state, snapshot: r.hash()? });
        }
        // Objects: re-hashed, bounded, and self-contained.
        let mut staged = ObjectStore::default();
        for _ in 0..n_objs {
            let kind = kind_from(r.u8()?).ok_or_else(|| HistoryError::Bundle("unknown object kind".into()))?;
            let hash = r.hash()?;
            let len = r.u32()? as usize;
            if len > limits.max_object_bytes {
                return refuse(format!("an object is {len} bytes; the limit is {}", limits.max_object_bytes));
            }
            if kind != ObjectKind::Blob && len > self.limits().max_object_bytes {
                return refuse("an object is larger than a project may hold");
            }
            let data: std::sync::Arc<[u8]> = std::sync::Arc::from(r.take(len)?);
            if staged.contains(hash) {
                return refuse("an object appears twice");
            }
            if staged.insert_verified(kind, hash, data).is_err() {
                return refuse("an object does not match its hash");
            }
        }
        if r.pos != payload.len() {
            return refuse("unexpected data after the last object");
        }
        let mut reach = BTreeSet::new();
        for n in &bnodes {
            if !staged.snapshot_closure(n.snapshot, &mut reach) || staged.document(n.snapshot).is_err() {
                return refuse("an iteration's document is missing or unreadable");
            }
        }
        if reach.len() != staged.len() {
            return refuse("the bundle holds objects no iteration uses");
        }
        // Nodes: parents must come earlier in the bundle; only the first may be a root.
        let mut index: HashMap<u64, usize> = HashMap::new();
        for (i, n) in bnodes.iter().enumerate() {
            if index.insert(n.id, i).is_some() {
                return refuse("an iteration id appears twice");
            }
            if n.id == 0 {
                return refuse("iteration id 0 is reserved");
            }
            if n.parent == 0 {
                if i != 0 {
                    return refuse("only the first iteration may have no parent");
                }
            } else if !index.contains_key(&n.parent) || n.parent == n.id {
                return refuse("an iteration's parent is not in the bundle");
            }
        }
        // Room for the worst case (every node new, a new branch) first: making room may
        // compact history, so the plan is made after it.
        let new_objects = staged.iter().filter(|(h, _, _)| !self.objects.contains(*h)).count();
        let name = branch_name_for(&src_name);
        let upper: usize = bnodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let id = NodeId(self.next_node.saturating_add(i as u64));
                let node = Node {
                    id,
                    parent: (n.parent != 0).then_some(id),
                    snapshot: n.snapshot,
                    label: n.label.clone(),
                    actor: n.actor.clone(),
                    time: n.time,
                    state: n.state.clone(),
                    name: None,
                    starred: false,
                    merged: None,
                };
                node_line(&node).len()
            })
            .sum();
        let need = Need {
            nodes: upper,
            new_nodes: bnodes.len(),
            new_objects,
            branches: 3 * name.len() + 256,
            new_branches: 1,
            events: 256,
        };
        self.room(ctx.time, need, false)?;
        let mut known: HashMap<(Hash, Option<NodeId>), NodeId> = HashMap::new();
        for n in self.nodes.values() {
            known.entry((n.snapshot, n.parent)).or_insert(n.id);
        }
        let mut map: Vec<NodeId> = Vec::with_capacity(bnodes.len());
        let mut fresh = Vec::new();
        let mut next = self.next_node;
        for n in &bnodes {
            let parent = if n.parent == 0 { None } else { index.get(&n.parent).and_then(|i| map.get(*i)).copied() };
            let id = match known.get(&(n.snapshot, parent)) {
                Some(id) => *id,
                None => {
                    let id = NodeId(next);
                    next += 1;
                    known.insert((n.snapshot, parent), id);
                    fresh.push(Node {
                        id,
                        parent,
                        snapshot: n.snapshot,
                        label: n.label.clone(),
                        actor: n.actor.clone(),
                        time: n.time,
                        state: n.state.clone(),
                        name: None,
                        starred: false,
                        merged: None,
                    });
                    id
                }
            };
            map.push(id);
        }
        let Some(head) = map.last().copied() else { return refuse("the bundle has no iterations") };
        let existing = self
            .branches
            .values()
            .find(|b| b.deleted_at.is_none() && b.head == head && b.name == name)
            .map(|b| (b.id, b.name.clone()));
        let nodes_reused = bnodes.len() - fresh.len();
        let nodes_added = fresh.len();
        for (h, kind, data) in staged.iter() {
            // Already hashed above; the store re-checks, which keeps this path honest.
            self.objects.insert_verified(kind, h, data.clone())?;
        }
        for n in fresh {
            self.sizes.nodes += node_line(&n).len();
            self.nodes.insert(n.id, n);
        }
        self.next_node = next;
        let (branch, branch_name, branch_created) = match existing {
            Some((id, name)) => (id, name, false),
            None => {
                let name = self.free_branch_name(&name);
                let id = BranchId(self.next_branch);
                let b = Branch {
                    id,
                    name: name.clone(),
                    head,
                    color: None,
                    notes: String::new(),
                    pinned: false,
                    created_by: ctx.actor.clone(),
                    created_at: ctx.time,
                    deleted_at: None,
                    rollback: None,
                };
                self.sizes.branches += branch_line(&b).len();
                self.next_branch += 1;
                self.branches.insert(id, b);
                self.record(ctx, Event::CreateBranch { branch: id, at: head });
                (id, name, true)
            }
        };
        Ok(BundleReport {
            nodes_added,
            nodes_reused,
            objects_added: new_objects,
            branch,
            branch_name,
            branch_created,
            head,
        })
    }

    /// `base`, or `base 2`, `base 3`, ... when a live branch already has that name.
    fn free_branch_name(&self, base: &str) -> String {
        if self.branch_by_name_folded(base).is_none() {
            return base.to_string();
        }
        (2u32..)
            .map(|i| {
                let suffix = format!(" {i}");
                let keep = MAX_NAME_LEN - suffix.chars().count();
                format!("{}{suffix}", base.chars().take(keep).collect::<String>())
            })
            .find(|n| self.branch_by_name_folded(n).is_none())
            .unwrap_or_else(|| base.to_string())
    }
}

/// `from/<source name>` as a valid branch name.
fn branch_name_for(src: &str) -> String {
    let tail = clean(src, MAX_NAME_LEN - 5);
    let tail = tail.trim();
    let name = format!("from/{}", if tail.is_empty() { "bundle" } else { tail });
    if check_branch_name(&name).is_ok() { name } else { "from/bundle".to_string() }
}

fn decompress(z: &[u8], limits: &BundleLimits) -> HResult<Vec<u8>> {
    let bad = |e: std::io::Error| HistoryError::Bundle(format!("the bundle cannot be decompressed: {e}"));
    let mut dec = zstd::stream::read::Decoder::with_buffer(z).map_err(bad)?;
    dec.window_log_max(WINDOW_LOG_MAX).map_err(bad)?;
    let mut out = Vec::new();
    let cap = limits.max_decompressed_bytes as u64 + 1;
    dec.take(cap).read_to_end(&mut out).map_err(bad)?;
    if out.len() > limits.max_decompressed_bytes {
        return refuse(format!("the content is larger than the {} byte limit", limits.max_decompressed_bytes));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gusset_doc::Document;

    fn doc(params: &[(&str, &str)]) -> Document {
        let mut t = String::from("gusset 1\nunits mm\n\n");
        for (n, v) in params {
            t.push_str(&format!("param {n} = {v}\n"));
        }
        Document::from_text(&t).unwrap()
    }

    fn chain(h: &mut History, vals: &[&str]) -> NodeId {
        let u = Ctx::user(10);
        let main = h.main();
        let mut last = h.head(main).unwrap();
        for v in vals {
            last = h.commit(&u, main, &doc(&[("a", v)]), &format!("a={v}"), NodeState::Ok).unwrap();
        }
        last
    }

    fn sample() -> (History, Vec<u8>) {
        let mut h = History::new(&doc(&[("a", "1 mm")]), &Ctx::user(1));
        chain(&mut h, &["2 mm", "3 mm"]);
        let main = h.main();
        let blob = h.put_blob(b"blob bytes");
        assert!(blob != Hash::of(b""));
        let bytes = h.export_bundle(main).unwrap();
        (h, bytes)
    }

    #[test]
    fn round_trip_into_a_fresh_history() {
        let (src, bytes) = sample();
        let mut dst = History::new(&doc(&[("z", "9 mm")]), &Ctx::user(5));
        let ctx = Ctx::agent("peer", 50);
        let r = dst.import_bundle(&ctx, &bytes, &BundleLimits::default()).unwrap();
        assert_eq!((r.nodes_added, r.nodes_reused), (3, 0));
        assert!(r.branch_created && r.objects_added > 0);
        assert_eq!(r.branch_name, "from/Main");
        assert_ne!(r.branch, dst.main());
        assert_eq!(dst.head_document(r.branch).unwrap(), src.head_document(src.main()).unwrap());
        assert_eq!(dst.lineage(r.head).len(), 3);
        assert_eq!(dst.nodes().count(), 4);
        assert!(
            dst.events().iter().any(|e| matches!(e.event, Event::CreateBranch { branch, .. } if branch == r.branch))
        );
        // The result still saves and loads.
        let again = History::from_records(&dst.to_records()).unwrap();
        assert_eq!(again.nodes().count(), 4);
    }

    #[test]
    fn reimport_adds_nothing_and_shared_past_is_reused() {
        let (_, bytes) = sample();
        let mut dst = History::new(&doc(&[("z", "9 mm")]), &Ctx::user(5));
        let ctx = Ctx::user(50);
        let lim = BundleLimits::default();
        let first = dst.import_bundle(&ctx, &bytes, &lim).unwrap();
        let (nodes, objs, events, branches) =
            (dst.nodes().count(), dst.objects().len(), dst.events().len(), dst.branches().len());
        let second = dst.import_bundle(&ctx, &bytes, &lim).unwrap();
        assert_eq!((second.nodes_added, second.nodes_reused, second.objects_added), (0, 3, 0));
        assert!(!second.branch_created);
        assert_eq!(second.branch, first.branch);
        assert_eq!(
            (dst.nodes().count(), dst.objects().len(), dst.events().len(), dst.branches().len()),
            (nodes, objs, events, branches)
        );
        // A longer bundle from the same source shares the first three nodes.
        let mut src = History::new(&doc(&[("a", "1 mm")]), &Ctx::user(1));
        chain(&mut src, &["2 mm", "3 mm"]);
        let mut peer = History::new(&doc(&[("a", "1 mm")]), &Ctx::user(1));
        chain(&mut peer, &["2 mm", "3 mm", "4 mm"]);
        let mut local = History::new(&doc(&[("a", "1 mm")]), &Ctx::user(1));
        chain(&mut local, &["2 mm"]);
        let r = local.import_bundle(&ctx, &peer.export_bundle(peer.main()).unwrap(), &lim).unwrap();
        assert_eq!((r.nodes_added, r.nodes_reused), (2, 2));
        // The imported branch shares an ancestor with main, so it can be merged.
        let main = local.main();
        let base = local.merge_base(local.head(main).unwrap(), r.head).unwrap();
        assert_eq!(base, local.head(main).unwrap());
        // A clashing name gets a suffix.
        local.create_branch(&ctx, base, Some("from/Main 3")).unwrap();
        let mut other = History::new(&doc(&[("a", "1 mm")]), &Ctx::user(1));
        chain(&mut other, &["2 mm", "7 mm"]);
        let r2 = local.import_bundle(&ctx, &other.export_bundle(other.main()).unwrap(), &lim).unwrap();
        assert_eq!(r2.branch_name, "from/Main 2");
        let r3 = local.import_bundle(&ctx, &src.export_bundle(src.main()).unwrap(), &lim).unwrap();
        assert!(r3.branch_name.starts_with("from/Main"));
    }

    fn refused(h: &mut History, bytes: &[u8], lim: &BundleLimits) -> bool {
        let before = (h.nodes().count(), h.objects().len(), h.events().len(), h.branches().len());
        let r = h.import_bundle(&Ctx::user(9), bytes, lim);
        let after = (h.nodes().count(), h.objects().len(), h.events().len(), h.branches().len());
        if matches!(r, Err(HistoryError::Bundle(_))) {
            assert_eq!(before, after, "a refused bundle changed history");
            true
        } else {
            false
        }
    }

    #[test]
    fn tampered_and_oversize_bundles_are_refused() {
        let (_, bytes) = sample();
        let lim = BundleLimits::default();
        let mut dst = History::new(&doc(&[]), &Ctx::user(5));
        let mut bad = bytes.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(refused(&mut dst, &bad, &lim));
        let mut bad = bytes.clone();
        bad[10] ^= 1; // checksum
        assert!(refused(&mut dst, &bad, &lim));
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(refused(&mut dst, &bad, &lim));
        assert!(refused(&mut dst, b"", &lim));
        assert!(refused(&mut dst, &bytes, &BundleLimits { max_bundle_bytes: 10, ..lim.clone() }));
        assert!(refused(&mut dst, &bytes, &BundleLimits { max_decompressed_bytes: 20, ..lim.clone() }));
        assert!(refused(&mut dst, &bytes, &BundleLimits { max_nodes: 2, ..lim.clone() }));
        assert!(refused(&mut dst, &bytes, &BundleLimits { max_objects: 1, ..lim.clone() }));
        assert!(refused(&mut dst, &bytes, &BundleLimits { max_object_bytes: 5, ..lim.clone() }));
        assert!(dst.import_bundle(&Ctx::user(9), &bytes, &lim).is_ok());
    }

    /// Builds a bundle with a valid checksum around a chosen payload.
    fn seal(payload: &[u8]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.push(VERSION);
        out.extend_from_slice(blake3::hash(payload).as_bytes());
        out.extend_from_slice(&zstd::stream::encode_all(payload, 3).unwrap());
        out
    }

    fn payload_of(bytes: &[u8]) -> Vec<u8> {
        decompress(&bytes[HEADER_LEN..], &BundleLimits::default()).unwrap()
    }

    #[test]
    fn wrong_object_hash_and_bad_structure_are_refused() {
        let (_, bytes) = sample();
        let p = payload_of(&bytes);
        let lim = BundleLimits::default();
        let mut dst = History::new(&doc(&[]), &Ctx::user(5));
        // Flip a byte inside the last object: the checksum is recomputed but the object
        // no longer matches its hash.
        let mut bad = p.clone();
        let n = bad.len();
        bad[n - 1] ^= 1;
        assert!(refused(&mut dst, &seal(&bad), &lim));
        // Trailing junk.
        let mut bad = p.clone();
        bad.push(0);
        assert!(refused(&mut dst, &seal(&bad), &lim));
        // A parent outside the bundle: rewrite the second node's parent id.
        let mut r = Reader { b: &p, pos: 0 };
        r.str().unwrap();
        r.u32().unwrap();
        r.u32().unwrap();
        r.u64().unwrap();
        r.u64().unwrap();
        r.str().unwrap();
        assert_eq!(r.u8().unwrap(), 0);
        r.u64().unwrap();
        assert_eq!(r.u8().unwrap(), 0);
        r.hash().unwrap();
        r.u64().unwrap(); // second node id
        let at = r.pos;
        let mut bad = p.clone();
        bad[at..at + 8].copy_from_slice(&999u64.to_le_bytes());
        assert!(refused(&mut dst, &seal(&bad), &lim));
        // A second root.
        let mut bad = p.clone();
        bad[at..at + 8].copy_from_slice(&0u64.to_le_bytes());
        assert!(refused(&mut dst, &seal(&bad), &lim));
        // An extra unused object is refused.
        let mut extra = p[..p.len()].to_vec();
        let data = b"junk";
        // object count + 1
        let mut r = Reader { b: &p, pos: 0 };
        r.str().unwrap();
        r.u32().unwrap();
        let cnt_at = r.pos;
        let cnt = r.u32().unwrap();
        extra[cnt_at..cnt_at + 4].copy_from_slice(&(cnt + 1).to_le_bytes());
        extra.push(3);
        extra.extend_from_slice(&Hash::of(data).0);
        extra.extend_from_slice(&4u32.to_le_bytes());
        extra.extend_from_slice(data);
        assert!(refused(&mut dst, &seal(&extra), &lim));
        // The original still imports.
        assert!(dst.import_bundle(&Ctx::user(9), &bytes, &lim).is_ok());
    }

    #[test]
    fn truncation_and_mutation_never_panic() {
        let (_, bytes) = sample();
        let lim = BundleLimits::default();
        let base = History::new(&doc(&[]), &Ctx::user(5));
        for n in 0..bytes.len() {
            let mut h = base.clone();
            assert!(h.import_bundle(&Ctx::user(9), &bytes[..n], &lim).is_err());
        }
        let p = payload_of(&bytes);
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..600 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mut m = p.clone();
            let i = (seed >> 33) as usize % m.len();
            m[i] ^= (seed >> 8) as u8 | 1;
            let mut h = base.clone();
            let _ = h.import_bundle(&Ctx::user(9), &seal(&m), &lim);
            let mut raw = bytes.clone();
            let j = (seed >> 20) as usize % raw.len();
            raw[j] ^= 0x55;
            let _ = h.import_bundle(&Ctx::user(9), &raw, &lim);
            if let Ok(back) = History::from_records(&h.to_records()) {
                assert_eq!(back.nodes().count(), h.nodes().count());
            }
        }
    }

    #[test]
    fn branch_names_are_sanitized() {
        assert_eq!(branch_name_for("a\u{7}b\n"), "from/ab");
        assert_eq!(branch_name_for("   "), "from/bundle");
        assert!(branch_name_for(&"x".repeat(500)).chars().count() <= MAX_NAME_LEN);
    }
}
