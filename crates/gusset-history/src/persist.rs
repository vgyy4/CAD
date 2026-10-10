//! Deterministic text serialization of the history graph for the `.gus` container.
//!
//! Objects (features, params, snapshots, blobs) are stored as individual records by the
//! store layer. The graph itself is three text records: nodes, branches and events.
//! Each is tab-separated lines with %-escaped fields.

use crate::history::*;
use crate::objects::{Hash, ObjectKind, ObjectStore};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Limits for hostile files.
pub const MAX_NODES: usize = 5_000_000;
pub const MAX_BRANCHES: usize = 100_000;
pub const MAX_EVENTS: usize = 10_000_000;

/// Everything needed to persist a history.
#[derive(Debug, Clone)]
pub struct HistoryRecords {
    pub objects: Vec<(ObjectKind, Hash, Arc<[u8]>)>,
    pub nodes: String,
    pub branches: String,
    pub events: String,
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' => o.push_str("%25"),
            '\t' => o.push_str("%09"),
            '\n' => o.push_str("%0A"),
            '\r' => o.push_str("%0D"),
            c => o.push(c),
        }
    }
    if o.is_empty() { "%".to_string() } else { o }
}

fn unesc(s: &str) -> Result<String, String> {
    if s == "%" {
        return Ok(String::new());
    }
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or("bad escape")?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "bad escape")?);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "invalid UTF-8".to_string())
}

fn opt(s: &Option<String>) -> String {
    match s {
        Some(v) => format!("={}", esc(v)),
        None => "-".into(),
    }
}

fn unopt(s: &str) -> Result<Option<String>, String> {
    if s == "-" { Ok(None) } else { s.strip_prefix('=').map(unesc).transpose().map(|o| o.or(Some(String::new()))) }
}

fn actor(a: &Actor) -> String {
    match a {
        Actor::User => "user".into(),
        Actor::Agent(s) => format!("agent:{}", esc(s)),
    }
}

fn unactor(s: &str) -> Result<Actor, String> {
    if s == "user" {
        Ok(Actor::User)
    } else {
        s.strip_prefix("agent:").map(|a| unesc(a).map(Actor::Agent)).unwrap_or(Err(format!("bad actor '{s}'")))
    }
}

fn num<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("bad number '{s}'"))
}

fn node_id(s: &str) -> Result<NodeId, String> {
    s.strip_prefix('n').ok_or_else(|| format!("bad node id '{s}'")).and_then(num).map(NodeId)
}

fn branch_id(s: &str) -> Result<BranchId, String> {
    s.strip_prefix('b').ok_or_else(|| format!("bad branch id '{s}'")).and_then(num).map(BranchId)
}

fn opt_usize(v: Option<usize>) -> String {
    v.map_or("-".into(), |x| x.to_string())
}

fn unopt_usize(s: &str) -> Result<Option<usize>, String> {
    if s == "-" { Ok(None) } else { num(s).map(Some) }
}

impl History {
    pub fn to_records(&self) -> HistoryRecords {
        let objects = self.objects.iter().map(|(h, k, b)| (k, h, b.clone())).collect();
        let mut nodes = String::from("nodes 1\n");
        for n in self.nodes.values() {
            let state = match &n.state {
                NodeState::Ok => "ok".to_string(),
                NodeState::Broken { message } => format!("broken={}", esc(message)),
            };
            nodes.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                n.id,
                n.parent.map_or("-".into(), |p| p.to_string()),
                n.snapshot,
                n.time,
                actor(&n.actor),
                state,
                u8::from(n.starred),
                opt(&n.name),
                esc(&n.label),
            ));
        }
        let mut branches = format!("branches 1\nmain\t{}\n", self.main);
        for b in self.branches.values() {
            branches.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                b.id,
                b.head,
                esc(&b.name),
                opt(&b.color),
                esc(&b.notes),
                u8::from(b.pinned),
                actor(&b.created_by),
                b.created_at,
                b.deleted_at.map_or("-".into(), |t| t.to_string()),
                opt_usize(b.rollback),
            ));
        }
        let mut events = String::from("events 1\n");
        for e in &self.events {
            let body = match &e.event {
                Event::Edit { branch, from, to } => format!("edit\t{branch}\t{from}\t{to}"),
                Event::MoveHead { branch, from, to } => format!("move\t{branch}\t{from}\t{to}"),
                Event::CreateBranch { branch, at } => format!("create\t{branch}\t{at}"),
                Event::RenameBranch { branch, old, new } => format!("rename\t{branch}\t{}\t{}", esc(old), esc(new)),
                Event::SetMain { old, new } => format!("setmain\t{old}\t{new}"),
                Event::DeleteBranch { branch } => format!("delete\t{branch}"),
                Event::RestoreBranch { branch } => format!("restore\t{branch}"),
                Event::SetRollback { branch, old, new } => {
                    format!("rollback\t{branch}\t{}\t{}", opt_usize(*old), opt_usize(*new))
                }
                Event::Star { node, starred } => format!("star\t{node}\t{}", u8::from(*starred)),
                Event::NameNode { node, old, new } => format!("name\t{node}\t{}\t{}", opt(old), opt(new)),
                Event::Undo { event } => format!("undo\t{event}"),
                Event::Redo { event } => format!("redo\t{event}"),
            };
            events.push_str(&format!("{}\t{}\t{}\n", e.time, actor(&e.actor), body));
        }
        HistoryRecords { objects, nodes, branches, events }
    }

    /// Rebuilds a history from records, verifying object hashes and graph integrity.
    pub fn from_records(r: &HistoryRecords) -> Result<History, String> {
        let mut objects = ObjectStore::default();
        for (k, h, b) in &r.objects {
            objects.insert_verified(*k, *h, b.clone()).map_err(|e| e.to_string())?;
        }
        // Nodes.
        let mut lines = r.nodes.lines();
        if lines.next() != Some("nodes 1") {
            return Err("nodes record: unsupported version".into());
        }
        let mut nodes = BTreeMap::new();
        for (i, l) in lines.enumerate() {
            if i >= MAX_NODES {
                return Err("too many history nodes".into());
            }
            let f: Vec<&str> = l.split('\t').collect();
            let [id, parent, snap, time, act, state, star, name, label] = f.as_slice() else {
                return Err(format!("nodes record line {}: wrong field count", i + 2));
            };
            let id = node_id(id)?;
            let parent = if *parent == "-" { None } else { Some(node_id(parent)?) };
            let snapshot = Hash::from_hex(snap).ok_or("bad snapshot hash")?;
            let state = if *state == "ok" {
                NodeState::Ok
            } else {
                NodeState::Broken { message: unesc(state.strip_prefix("broken=").ok_or("bad node state")?)? }
            };
            nodes.insert(
                id,
                Node {
                    id,
                    parent,
                    snapshot,
                    label: unesc(label)?,
                    actor: unactor(act)?,
                    time: num(time)?,
                    state,
                    name: unopt(name)?,
                    starred: *star == "1",
                },
            );
        }
        // Integrity: parents exist, no cycles, snapshots exist.
        for n in nodes.values() {
            if let Some(p) = n.parent {
                if !nodes.contains_key(&p) {
                    return Err(format!("node {} has a missing parent {p}", n.id));
                }
            }
            if !objects.contains(n.snapshot) {
                return Err(format!("node {} refers to a missing snapshot", n.id));
            }
        }
        let mut depth_ok = BTreeMap::new();
        for &start in nodes.keys() {
            let mut cur = Some(start);
            let mut steps = 0usize;
            while let Some(c) = cur {
                if depth_ok.contains_key(&c) {
                    break;
                }
                steps += 1;
                if steps > nodes.len() {
                    return Err("history graph contains a cycle".into());
                }
                cur = nodes.get(&c).and_then(|n: &Node| n.parent);
            }
            depth_ok.insert(start, ());
        }
        // Branches.
        let mut lines = r.branches.lines();
        if lines.next() != Some("branches 1") {
            return Err("branches record: unsupported version".into());
        }
        let main = lines
            .next()
            .and_then(|l| l.strip_prefix("main\t"))
            .ok_or("branches record: missing main")
            .and_then(|s| branch_id(s).map_err(|_| "branches record: bad main"))?;
        let mut branches = BTreeMap::new();
        for (i, l) in lines.enumerate() {
            if i >= MAX_BRANCHES {
                return Err("too many branches".into());
            }
            let f: Vec<&str> = l.split('\t').collect();
            let [id, head, name, color, notes, pinned, by, at, deleted, rollback] = f.as_slice() else {
                return Err(format!("branches record line {}: wrong field count", i + 3));
            };
            let id = branch_id(id)?;
            let head = node_id(head)?;
            if !nodes.contains_key(&head) {
                return Err(format!("branch {id} points to a missing node"));
            }
            branches.insert(
                id,
                Branch {
                    id,
                    name: unesc(name)?,
                    head,
                    color: unopt(color)?,
                    notes: unesc(notes)?,
                    pinned: *pinned == "1",
                    created_by: unactor(by)?,
                    created_at: num(at)?,
                    deleted_at: if *deleted == "-" { None } else { Some(num(deleted)?) },
                    rollback: unopt_usize(rollback)?,
                },
            );
        }
        if branches.get(&main).is_none_or(|b| b.deleted_at.is_some()) {
            return Err("the main branch is missing".into());
        }
        // Events (audit log). Unknown/malformed lines are an error: the log is checksummed
        // by the container, so damage here means corruption.
        let mut lines = r.events.lines();
        if lines.next() != Some("events 1") {
            return Err("events record: unsupported version".into());
        }
        let mut events = Vec::new();
        for (i, l) in lines.enumerate() {
            if i >= MAX_EVENTS {
                return Err("too many events".into());
            }
            let f: Vec<&str> = l.split('\t').collect();
            let bad = || format!("events record line {}: malformed", i + 2);
            let time: u64 = num(f.first().ok_or_else(bad)?)?;
            let act = unactor(f.get(1).ok_or_else(bad)?)?;
            let g = |k: usize| f.get(k).copied().ok_or_else(bad);
            let event = match g(2)? {
                "edit" => Event::Edit { branch: branch_id(g(3)?)?, from: node_id(g(4)?)?, to: node_id(g(5)?)? },
                "move" => Event::MoveHead { branch: branch_id(g(3)?)?, from: node_id(g(4)?)?, to: node_id(g(5)?)? },
                "create" => Event::CreateBranch { branch: branch_id(g(3)?)?, at: node_id(g(4)?)? },
                "rename" => Event::RenameBranch { branch: branch_id(g(3)?)?, old: unesc(g(4)?)?, new: unesc(g(5)?)? },
                "setmain" => Event::SetMain { old: branch_id(g(3)?)?, new: branch_id(g(4)?)? },
                "delete" => Event::DeleteBranch { branch: branch_id(g(3)?)? },
                "restore" => Event::RestoreBranch { branch: branch_id(g(3)?)? },
                "rollback" => {
                    Event::SetRollback { branch: branch_id(g(3)?)?, old: unopt_usize(g(4)?)?, new: unopt_usize(g(5)?)? }
                }
                "star" => Event::Star { node: node_id(g(3)?)?, starred: g(4)? == "1" },
                "name" => Event::NameNode { node: node_id(g(3)?)?, old: unopt(g(4)?)?, new: unopt(g(5)?)? },
                "undo" => Event::Undo { event: num(g(3)?)? },
                "redo" => Event::Redo { event: num(g(3)?)? },
                _ => return Err(bad()),
            };
            events.push(EventRecord { time, actor: act, event });
        }
        Ok(History::from_parts(objects, nodes, branches, main, events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        for s in ["", "a\tb\nc%d\re", "naïve ✓", "%"] {
            assert_eq!(unesc(&esc(s)).unwrap(), s);
        }
        assert!(unesc("%zz").is_err());
        assert_eq!(unopt(&opt(&Some(String::new()))).unwrap(), Some(String::new()));
        assert_eq!(unopt(&opt(&None)).unwrap(), None);
    }
}
