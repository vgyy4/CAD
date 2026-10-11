//! The single history model (ADR 0006): node tree, branches, event log and undo.

use crate::diff::{DocDiff, diff_documents};
use crate::objects::{Hash, ObjectError, ObjectStore};
use gusset_doc::Document;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BranchId(pub u64);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}

impl fmt::Display for BranchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "b{}", self.0)
    }
}

/// Who did something. Agents are identified by their session name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Actor {
    User,
    Agent(String),
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Actor::User => f.write_str("user"),
            Actor::Agent(s) => write!(f, "agent:{s}"),
        }
    }
}

/// Context of a mutating call: who, and when (unix seconds). Time is passed in rather
/// than read from the clock so history is deterministic and testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    pub actor: Actor,
    pub time: u64,
}

impl Ctx {
    pub fn user(time: u64) -> Self {
        Ctx { actor: Actor::User, time }
    }
    pub fn agent(name: &str, time: u64) -> Self {
        Ctx { actor: Actor::Agent(name.to_string()), time }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeState {
    Ok,
    /// The edit was recorded but the rebuild failed; the user can fix or discard it.
    Broken {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub snapshot: Hash,
    /// What happened ("Edit Extrude1", "Add Fillet2").
    pub label: String,
    pub actor: Actor,
    pub time: u64,
    pub state: NodeState,
    /// User-given iteration name; named nodes are never pruned.
    pub name: Option<String>,
    pub starred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub id: BranchId,
    pub name: String,
    pub head: NodeId,
    /// Color tag (a design-token name such as "agent" or a hex color).
    pub color: Option<String>,
    pub notes: String,
    pub pinned: bool,
    pub created_by: Actor,
    pub created_at: u64,
    /// Soft-deletion time; deleted branches live in "Recently deleted" until retention ends.
    pub deleted_at: Option<u64>,
    /// Feature-timeline rollback marker: number of active features, `None` = all.
    pub rollback: Option<usize>,
}

/// Everything that changes the project, in order. Persisted as the audit log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Edit {
        branch: BranchId,
        from: NodeId,
        to: NodeId,
    },
    MoveHead {
        branch: BranchId,
        from: NodeId,
        to: NodeId,
    },
    CreateBranch {
        branch: BranchId,
        at: NodeId,
    },
    RenameBranch {
        branch: BranchId,
        old: String,
        new: String,
    },
    SetMain {
        old: BranchId,
        new: BranchId,
    },
    DeleteBranch {
        branch: BranchId,
    },
    RestoreBranch {
        branch: BranchId,
    },
    SetRollback {
        branch: BranchId,
        old: Option<usize>,
        new: Option<usize>,
    },
    Star {
        node: NodeId,
        starred: bool,
    },
    NameNode {
        node: NodeId,
        old: Option<String>,
        new: Option<String>,
    },
    Undo {
        event: usize,
    },
    Redo {
        event: usize,
    },
    /// An agent ran a command (audit only; never undone). `detail` is untrusted text.
    AgentAction {
        command: String,
        branch: Option<BranchId>,
        outcome: String,
        detail: String,
    },
}

impl Event {
    /// The branch an event is scoped to for undo purposes (`None` = project-wide).
    fn context(&self) -> Option<BranchId> {
        match self {
            Event::Edit { branch, .. } | Event::MoveHead { branch, .. } | Event::SetRollback { branch, .. } => {
                Some(*branch)
            }
            _ => None,
        }
    }

    /// Short human description for the audit log.
    pub fn describe(&self) -> String {
        match self {
            Event::Edit { branch, to, .. } => format!("edited {branch} → {to}"),
            Event::MoveHead { branch, to, .. } => format!("moved {branch} to {to}"),
            Event::CreateBranch { branch, at } => format!("created {branch} at {at}"),
            Event::RenameBranch { branch, new, .. } => format!("renamed {branch} to \"{new}\""),
            Event::SetMain { new, .. } => format!("set {new} as main"),
            Event::DeleteBranch { branch } => format!("deleted {branch}"),
            Event::RestoreBranch { branch } => format!("restored {branch}"),
            Event::SetRollback { branch, new, .. } => match new {
                Some(n) => format!("rolled {branch} back to feature {n}"),
                None => format!("rolled {branch} forward to the end"),
            },
            Event::Star { node, starred } => format!("{} {node}", if *starred { "starred" } else { "unstarred" }),
            Event::NameNode { node, new, .. } => match new {
                Some(n) => format!("named {node} \"{n}\""),
                None => format!("removed the name of {node}"),
            },
            Event::Undo { event } => format!("undid event {event}"),
            Event::Redo { event } => format!("redid event {event}"),
            Event::AgentAction { command, outcome, .. } => format!("agent ran {command}: {outcome}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub time: u64,
    pub actor: Actor,
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HistoryError {
    UnknownBranch(BranchId),
    UnknownNode(NodeId),
    BranchDeleted(String),
    /// Exactly one branch is main; it cannot be deleted until another branch is main.
    MainCannotBeDeleted,
    /// A destructive operation needs explicit confirmation; `reason` explains the stakes.
    ConfirmationRequired {
        reason: String,
    },
    NameTaken(String),
    InvalidName(String),
    NothingToUndo,
    NothingToRedo,
    /// The branch changed since the event being undone (e.g. another actor edited it).
    Conflict(String),
    Object(ObjectError),
    InvalidDocument(String),
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HistoryError::UnknownBranch(b) => write!(f, "branch {b} does not exist"),
            HistoryError::UnknownNode(n) => write!(f, "iteration {n} does not exist"),
            HistoryError::BranchDeleted(n) => write!(f, "branch \"{n}\" is deleted; restore it first"),
            HistoryError::MainCannotBeDeleted => {
                f.write_str("the main branch cannot be deleted; make another branch main first")
            }
            HistoryError::ConfirmationRequired { reason } => write!(f, "confirmation required: {reason}"),
            HistoryError::NameTaken(n) => write!(f, "a branch named \"{n}\" already exists"),
            HistoryError::InvalidName(m) => write!(f, "invalid name: {m}"),
            HistoryError::NothingToUndo => f.write_str("nothing to undo"),
            HistoryError::NothingToRedo => f.write_str("nothing to redo"),
            HistoryError::Conflict(m) => write!(f, "cannot undo: {m}"),
            HistoryError::Object(e) => write!(f, "{e}"),
            HistoryError::InvalidDocument(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for HistoryError {}

impl From<ObjectError> for HistoryError {
    fn from(e: ObjectError) -> Self {
        HistoryError::Object(e)
    }
}

pub type HResult<T> = Result<T, HistoryError>;

/// Pruning/GC policy (user-visible, configurable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcPolicy {
    /// Soft-deleted branches are purged after this many seconds (0 = immediately).
    pub deleted_retention: u64,
    /// Unnamed intermediate nodes older than this (seconds) may be squashed; `None` = never.
    pub squash_after: Option<u64>,
}

impl Default for GcPolicy {
    fn default() -> Self {
        GcPolicy { deleted_retention: 7 * 24 * 3600, squash_after: Some(30 * 24 * 3600) }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcReport {
    pub branches_purged: usize,
    pub nodes_squashed: usize,
    pub nodes_removed: usize,
    pub objects_removed: usize,
    pub bytes_before: usize,
    pub bytes_after: usize,
}

/// The project history. See the crate docs and ADR 0006.
#[derive(Debug, Clone)]
pub struct History {
    pub(crate) objects: ObjectStore,
    pub(crate) nodes: BTreeMap<NodeId, Node>,
    pub(crate) branches: BTreeMap<BranchId, Branch>,
    pub(crate) main: BranchId,
    pub(crate) events: Vec<EventRecord>,
    pub(crate) next_node: u64,
    pub(crate) next_branch: u64,
    /// Session-only undo/redo stacks per actor (indices into `events`).
    undo: BTreeMap<Actor, Vec<usize>>,
    redo: BTreeMap<Actor, Vec<usize>>,
}

pub const MAX_NAME_LEN: usize = 120;

fn check_branch_name(name: &str) -> HResult<()> {
    let t = name.trim();
    if t.is_empty() {
        return Err(HistoryError::InvalidName("a branch name cannot be empty".into()));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(HistoryError::InvalidName(format!("a branch name is limited to {MAX_NAME_LEN} characters")));
    }
    if name.chars().any(char::is_control) {
        return Err(HistoryError::InvalidName("a branch name cannot contain control characters".into()));
    }
    Ok(())
}

impl History {
    /// Creates a history whose root node holds `doc`, on a main branch named "Main".
    pub fn new(doc: &Document, ctx: &Ctx) -> History {
        let mut objects = ObjectStore::default();
        let snapshot = objects.put_document(doc);
        let root = NodeId(1);
        let mut nodes = BTreeMap::new();
        nodes.insert(
            root,
            Node {
                id: root,
                parent: None,
                snapshot,
                label: "New project".into(),
                actor: ctx.actor.clone(),
                time: ctx.time,
                state: NodeState::Ok,
                name: None,
                starred: false,
            },
        );
        let main = BranchId(1);
        let mut branches = BTreeMap::new();
        branches.insert(
            main,
            Branch {
                id: main,
                name: "Main".into(),
                head: root,
                color: None,
                notes: String::new(),
                pinned: false,
                created_by: ctx.actor.clone(),
                created_at: ctx.time,
                deleted_at: None,
                rollback: None,
            },
        );
        History {
            objects,
            nodes,
            branches,
            main,
            events: Vec::new(),
            next_node: 2,
            next_branch: 2,
            undo: BTreeMap::new(),
            redo: BTreeMap::new(),
        }
    }

    // ------------------------------------------------------------ queries

    pub fn main(&self) -> BranchId {
        self.main
    }

    pub fn branch(&self, id: BranchId) -> HResult<&Branch> {
        self.branches.get(&id).ok_or(HistoryError::UnknownBranch(id))
    }

    /// Live (not deleted) branches, main first, then by id.
    pub fn branches(&self) -> Vec<&Branch> {
        let mut v: Vec<&Branch> = self.branches.values().filter(|b| b.deleted_at.is_none()).collect();
        v.sort_by_key(|b| (b.id != self.main, b.id));
        v
    }

    /// The "Recently deleted" list.
    pub fn deleted_branches(&self) -> Vec<&Branch> {
        self.branches.values().filter(|b| b.deleted_at.is_some()).collect()
    }

    pub fn branch_by_name(&self, name: &str) -> Option<&Branch> {
        self.branches.values().find(|b| b.deleted_at.is_none() && b.name == name)
    }

    /// A live branch whose name equals `name` ignoring case and surrounding space
    /// (names that look the same must not coexist: spoofing "Main").
    pub fn branch_by_name_folded(&self, name: &str) -> Option<&Branch> {
        let n = name.trim().to_lowercase();
        self.branches.values().find(|b| b.deleted_at.is_none() && b.name.trim().to_lowercase() == n)
    }

    /// Appends an agent audit entry to the persisted event log (not undoable).
    pub fn record_agent_action(
        &mut self,
        ctx: &Ctx,
        command: &str,
        branch: Option<BranchId>,
        outcome: &str,
        detail: &str,
    ) {
        let clip = |s: &str, n: usize| s.chars().filter(|c| !c.is_control()).take(n).collect::<String>();
        self.record(
            ctx,
            Event::AgentAction {
                command: clip(command, 64),
                branch,
                outcome: clip(outcome, 128),
                detail: clip(detail, 512),
            },
        );
    }

    pub fn node(&self, id: NodeId) -> HResult<&Node> {
        self.nodes.get(&id).ok_or(HistoryError::UnknownNode(id))
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    /// Children of a node in creation order (the "redo futures").
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes.values().filter(|n| n.parent == Some(id)).map(|n| n.id).collect()
    }

    pub fn events(&self) -> &[EventRecord] {
        &self.events
    }

    pub fn objects(&self) -> &ObjectStore {
        &self.objects
    }

    /// Stores a blob (imported file) and returns its hash.
    pub fn put_blob(&mut self, bytes: &[u8]) -> Hash {
        self.objects.put(crate::objects::ObjectKind::Blob, bytes)
    }

    pub fn document(&self, node: NodeId) -> HResult<Document> {
        Ok(self.objects.document(self.node(node)?.snapshot)?)
    }

    pub fn head(&self, branch: BranchId) -> HResult<NodeId> {
        Ok(self.branch(branch)?.head)
    }

    pub fn head_document(&self, branch: BranchId) -> HResult<Document> {
        self.document(self.head(branch)?)
    }

    /// Ancestors of a node, nearest first, including the node itself.
    pub fn lineage(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = Some(node);
        while let Some(n) = cur {
            if out.len() > self.nodes.len() {
                break; // defensive: a cycle would be corruption
            }
            out.push(n);
            cur = self.nodes.get(&n).and_then(|x| x.parent);
        }
        out
    }

    /// A default name "Iteration N" not used by any live branch.
    pub fn default_branch_name(&self) -> String {
        (2..)
            .map(|i| format!("Iteration {i}"))
            .find(|n| self.branch_by_name_folded(n).is_none())
            .unwrap_or_else(|| "Iteration".into())
    }

    // ------------------------------------------------------------ mutations

    fn live_branch_mut(&mut self, id: BranchId) -> HResult<&mut Branch> {
        let b = self.branches.get_mut(&id).ok_or(HistoryError::UnknownBranch(id))?;
        if b.deleted_at.is_some() {
            return Err(HistoryError::BranchDeleted(b.name.clone()));
        }
        Ok(b)
    }

    fn record(&mut self, ctx: &Ctx, event: Event) -> usize {
        let undoable = !matches!(event, Event::Undo { .. } | Event::Redo { .. } | Event::AgentAction { .. });
        self.events.push(EventRecord { time: ctx.time, actor: ctx.actor.clone(), event });
        let idx = self.events.len() - 1;
        if undoable {
            self.undo.entry(ctx.actor.clone()).or_default().push(idx);
            self.redo.entry(ctx.actor.clone()).or_default().clear();
        }
        idx
    }

    /// Records a new document state on `branch`. Returns the new head. A document equal
    /// to the current head (and an Ok state) records nothing.
    pub fn commit(
        &mut self,
        ctx: &Ctx,
        branch: BranchId,
        doc: &Document,
        label: &str,
        state: NodeState,
    ) -> HResult<NodeId> {
        let head = self.live_branch_mut(branch)?.head;
        let snapshot = self.objects.put_document(doc);
        if state == NodeState::Ok && self.node(head)?.snapshot == snapshot {
            return Ok(head);
        }
        let id = NodeId(self.next_node);
        self.next_node += 1;
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(head),
                snapshot,
                label: label.chars().take(200).collect(),
                actor: ctx.actor.clone(),
                time: ctx.time,
                state,
                name: None,
                starred: false,
            },
        );
        self.live_branch_mut(branch)?.head = id;
        self.record(ctx, Event::Edit { branch, from: head, to: id });
        Ok(id)
    }

    /// Points a branch at any node (e.g. "go to this iteration"); non-destructive.
    pub fn move_head(&mut self, ctx: &Ctx, branch: BranchId, to: NodeId) -> HResult<()> {
        self.node(to)?;
        let b = self.live_branch_mut(branch)?;
        let from = b.head;
        if from == to {
            return Ok(());
        }
        b.head = to;
        b.rollback = None;
        self.record(ctx, Event::MoveHead { branch, from, to });
        Ok(())
    }

    pub fn create_branch(&mut self, ctx: &Ctx, at: NodeId, name: Option<&str>) -> HResult<BranchId> {
        self.node(at)?;
        let name = match name {
            Some(n) => {
                check_branch_name(n)?;
                n.trim().to_string()
            }
            None => self.default_branch_name(),
        };
        if self.branch_by_name_folded(&name).is_some() {
            return Err(HistoryError::NameTaken(name));
        }
        let id = BranchId(self.next_branch);
        self.next_branch += 1;
        let color = match &ctx.actor {
            Actor::Agent(_) => Some("agent".to_string()),
            Actor::User => None,
        };
        self.branches.insert(
            id,
            Branch {
                id,
                name,
                head: at,
                color,
                notes: String::new(),
                pinned: false,
                created_by: ctx.actor.clone(),
                created_at: ctx.time,
                deleted_at: None,
                rollback: None,
            },
        );
        self.record(ctx, Event::CreateBranch { branch: id, at });
        Ok(id)
    }

    pub fn rename_branch(&mut self, ctx: &Ctx, branch: BranchId, name: &str) -> HResult<()> {
        check_branch_name(name)?;
        let name = name.trim().to_string();
        if let Some(other) = self.branch_by_name_folded(&name)
            && other.id != branch
        {
            return Err(HistoryError::NameTaken(name));
        }
        let b = self.live_branch_mut(branch)?;
        let old = std::mem::replace(&mut b.name, name.clone());
        if old != name {
            self.record(ctx, Event::RenameBranch { branch, old, new: name });
        }
        Ok(())
    }

    /// Makes `branch` the main branch. (Agents need human approval; enforced by the
    /// command layer.)
    pub fn set_main(&mut self, ctx: &Ctx, branch: BranchId) -> HResult<()> {
        self.live_branch_mut(branch)?;
        let old = self.main;
        if old != branch {
            self.main = branch;
            self.record(ctx, Event::SetMain { old, new: branch });
        }
        Ok(())
    }

    /// Starred nodes that only this branch reaches (deleting it would hide them).
    pub fn exclusive_starred(&self, branch: BranchId) -> HResult<Vec<NodeId>> {
        let head = self.head(branch)?;
        let mut others = BTreeSet::new();
        for b in self.branches.values().filter(|b| b.id != branch && b.deleted_at.is_none()) {
            others.extend(self.lineage(b.head));
        }
        Ok(self
            .lineage(head)
            .into_iter()
            .filter(|n| !others.contains(n) && self.nodes.get(n).is_some_and(|x| x.starred))
            .collect())
    }

    /// Soft-deletes a branch into "Recently deleted". Requires `confirm`; the error
    /// without it explains what would be affected.
    pub fn delete_branch(&mut self, ctx: &Ctx, branch: BranchId, confirm: bool) -> HResult<()> {
        if branch == self.main {
            return Err(HistoryError::MainCannotBeDeleted);
        }
        let name = self.live_branch_mut(branch)?.name.clone();
        if !confirm {
            let starred = self.exclusive_starred(branch)?;
            let mut reason = format!("delete branch \"{name}\"? It moves to Recently deleted and can be restored");
            if !starred.is_empty() {
                reason.push_str(&format!(
                    "; it contains {} starred iteration{} not on any other branch",
                    starred.len(),
                    if starred.len() == 1 { "" } else { "s" }
                ));
            }
            return Err(HistoryError::ConfirmationRequired { reason });
        }
        self.live_branch_mut(branch)?.deleted_at = Some(ctx.time);
        self.record(ctx, Event::DeleteBranch { branch });
        Ok(())
    }

    pub fn restore_branch(&mut self, ctx: &Ctx, branch: BranchId) -> HResult<()> {
        let b = self.branches.get(&branch).ok_or(HistoryError::UnknownBranch(branch))?;
        if b.deleted_at.is_none() {
            return Ok(());
        }
        self.undelete(branch)?;
        self.record(ctx, Event::RestoreBranch { branch });
        Ok(())
    }

    /// Brings a soft-deleted branch back. If a live branch now has the same name
    /// (ignoring case), the restored one gets a free "<name> (restored)" variant, so two
    /// live branches never share a name (review M8).
    fn undelete(&mut self, branch: BranchId) -> HResult<()> {
        let b = self.branches.get(&branch).ok_or(HistoryError::UnknownBranch(branch))?;
        if b.deleted_at.is_none() {
            return Ok(());
        }
        let name = b.name.clone();
        let taken = |h: &History, n: &str| h.branch_by_name_folded(n).is_some_and(|o| o.id != branch);
        let final_name = if taken(self, &name) {
            let base: String = name.chars().take(MAX_NAME_LEN.saturating_sub(16)).collect();
            (1..)
                .map(|i| if i == 1 { format!("{base} (restored)") } else { format!("{base} (restored {i})") })
                .find(|n| !taken(self, n))
                .unwrap_or_else(|| format!("{base} (restored)"))
        } else {
            name
        };
        let b = self.branches.get_mut(&branch).ok_or(HistoryError::UnknownBranch(branch))?;
        b.deleted_at = None;
        b.name = final_name;
        Ok(())
    }

    pub fn set_rollback(&mut self, ctx: &Ctx, branch: BranchId, marker: Option<usize>) -> HResult<()> {
        let n = self.head_document(branch)?.features.len();
        let marker = marker.filter(|&m| m < n);
        let b = self.live_branch_mut(branch)?;
        let old = b.rollback;
        if old != marker {
            b.rollback = marker;
            self.record(ctx, Event::SetRollback { branch, old, new: marker });
        }
        Ok(())
    }

    pub fn set_starred(&mut self, ctx: &Ctx, node: NodeId, starred: bool) -> HResult<()> {
        let n = self.nodes.get_mut(&node).ok_or(HistoryError::UnknownNode(node))?;
        if n.starred != starred {
            n.starred = starred;
            self.record(ctx, Event::Star { node, starred });
        }
        Ok(())
    }

    pub fn name_node(&mut self, ctx: &Ctx, node: NodeId, name: Option<&str>) -> HResult<()> {
        if let Some(n) = name {
            check_branch_name(n)?;
        }
        let new = name.map(|s| s.trim().to_string());
        let n = self.nodes.get_mut(&node).ok_or(HistoryError::UnknownNode(node))?;
        let old = n.name.clone();
        if old != new {
            n.name = new.clone();
            self.record(ctx, Event::NameNode { node, old, new });
        }
        Ok(())
    }

    /// Updates color/notes/pinned (not undoable; cosmetic metadata).
    pub fn set_branch_meta(
        &mut self,
        branch: BranchId,
        color: Option<Option<String>>,
        notes: Option<String>,
        pinned: Option<bool>,
    ) -> HResult<()> {
        let b = self.live_branch_mut(branch)?;
        if let Some(c) = color {
            b.color = c.map(|s| s.chars().take(32).collect());
        }
        if let Some(n) = notes {
            b.notes = n.chars().take(10_000).collect();
        }
        if let Some(p) = pinned {
            b.pinned = p;
        }
        Ok(())
    }

    // ------------------------------------------------------------ undo / redo

    /// Undoes `ctx.actor`'s most recent undoable action in the context of `branch`
    /// (edits on that branch, or project-wide actions). Returns a description.
    pub fn undo(&mut self, ctx: &Ctx, branch: BranchId) -> HResult<String> {
        let stack = self.undo.get(&ctx.actor).cloned().unwrap_or_default();
        let pos = stack
            .iter()
            .rposition(|&i| self.events.get(i).is_some_and(|e| e.event.context().is_none_or(|b| b == branch)))
            .ok_or(HistoryError::NothingToUndo)?;
        let idx = stack.get(pos).copied().ok_or(HistoryError::NothingToUndo)?;
        let ev = self.events.get(idx).map(|e| e.event.clone()).ok_or(HistoryError::NothingToUndo)?;
        self.apply(&ev, true, ctx.time)?;
        if let Some(s) = self.undo.get_mut(&ctx.actor) {
            s.remove(pos);
        }
        self.redo.entry(ctx.actor.clone()).or_default().push(idx);
        self.record(ctx, Event::Undo { event: idx });
        Ok(format!("Undo: {}", ev.describe()))
    }

    pub fn redo(&mut self, ctx: &Ctx, branch: BranchId) -> HResult<String> {
        let stack = self.redo.get(&ctx.actor).cloned().unwrap_or_default();
        let pos = stack
            .iter()
            .rposition(|&i| self.events.get(i).is_some_and(|e| e.event.context().is_none_or(|b| b == branch)))
            .ok_or(HistoryError::NothingToRedo)?;
        let idx = stack.get(pos).copied().ok_or(HistoryError::NothingToRedo)?;
        let ev = self.events.get(idx).map(|e| e.event.clone()).ok_or(HistoryError::NothingToRedo)?;
        self.apply(&ev, false, ctx.time)?;
        if let Some(s) = self.redo.get_mut(&ctx.actor) {
            s.remove(pos);
        }
        self.undo.entry(ctx.actor.clone()).or_default().push(idx);
        self.events.push(EventRecord { time: ctx.time, actor: ctx.actor.clone(), event: Event::Redo { event: idx } });
        Ok(format!("Redo: {}", ev.describe()))
    }

    pub fn can_undo(&self, actor: &Actor, branch: BranchId) -> bool {
        self.undo.get(actor).is_some_and(|s| {
            s.iter().any(|&i| self.events.get(i).is_some_and(|e| e.event.context().is_none_or(|b| b == branch)))
        })
    }

    pub fn can_redo(&self, actor: &Actor, branch: BranchId) -> bool {
        self.redo.get(actor).is_some_and(|s| {
            s.iter().any(|&i| self.events.get(i).is_some_and(|e| e.event.context().is_none_or(|b| b == branch)))
        })
    }

    /// Applies an event backwards (`inverse`) or forwards.
    fn apply(&mut self, ev: &Event, inverse: bool, now: u64) -> HResult<()> {
        fn pick<T>(a: T, b: T, inverse: bool) -> (T, T) {
            if inverse { (b, a) } else { (a, b) }
        }
        match ev {
            Event::Edit { branch, from, to } | Event::MoveHead { branch, from, to } => {
                let (expect, set) = pick(*from, *to, inverse);
                let b = self.live_branch_mut(*branch)?;
                if b.head != expect {
                    return Err(HistoryError::Conflict(format!(
                        "branch \"{}\" has changed since (another edit happened after it)",
                        b.name
                    )));
                }
                b.head = set;
            }
            Event::CreateBranch { branch, .. } => {
                if *branch == self.main {
                    return Err(HistoryError::Conflict("that branch is now main".into()));
                }
                if inverse {
                    let b = self.branches.get_mut(branch).ok_or(HistoryError::UnknownBranch(*branch))?;
                    b.deleted_at = Some(now);
                } else {
                    self.undelete(*branch)?;
                }
            }
            Event::DeleteBranch { branch } | Event::RestoreBranch { branch } => {
                let delete = matches!(ev, Event::DeleteBranch { .. }) != inverse;
                if delete && *branch == self.main {
                    return Err(HistoryError::MainCannotBeDeleted);
                }
                if delete {
                    let b = self.branches.get_mut(branch).ok_or(HistoryError::UnknownBranch(*branch))?;
                    b.deleted_at = Some(now);
                } else {
                    self.undelete(*branch)?;
                }
            }
            Event::RenameBranch { branch, old, new } => {
                let (_, set) = pick(old.clone(), new.clone(), inverse);
                if self.branch_by_name_folded(&set).is_some_and(|b| b.id != *branch) {
                    return Err(HistoryError::NameTaken(set));
                }
                self.live_branch_mut(*branch)?.name = set;
            }
            Event::SetMain { old, new } => {
                let (_, set) = pick(*old, *new, inverse);
                self.live_branch_mut(set)?;
                self.main = set;
            }
            Event::SetRollback { branch, old, new } => {
                let (_, set) = pick(*old, *new, inverse);
                self.live_branch_mut(*branch)?.rollback = set;
            }
            Event::Star { node, starred } => {
                let n = self.nodes.get_mut(node).ok_or(HistoryError::UnknownNode(*node))?;
                n.starred = if inverse { !starred } else { *starred };
            }
            Event::NameNode { node, old, new } => {
                let (_, set) = pick(old.clone(), new.clone(), inverse);
                self.nodes.get_mut(node).ok_or(HistoryError::UnknownNode(*node))?.name = set;
            }
            Event::Undo { .. } | Event::Redo { .. } | Event::AgentAction { .. } => {}
        }
        Ok(())
    }

    // ------------------------------------------------------------ compare / cherry-pick

    pub fn compare(&self, a: NodeId, b: NodeId) -> HResult<DocDiff> {
        Ok(diff_documents(&self.document(a)?, &self.document(b)?))
    }

    /// Builds the document that results from applying `features` and `params` from node
    /// `source` onto the head of `target`. The caller rebuilds it and commits (a failing
    /// rebuild is committed as a broken node).
    pub fn cherry_pick(
        &self,
        source: NodeId,
        target: BranchId,
        features: &[String],
        params: &[String],
    ) -> HResult<Document> {
        let src = self.document(source)?;
        let mut doc = self.head_document(target)?;
        for p in params {
            let sp = src.params.iter().find(|x| &x.name == p).ok_or_else(|| {
                HistoryError::InvalidDocument(format!("parameter '{p}' does not exist in the source"))
            })?;
            match doc.params.iter_mut().find(|x| &x.name == p) {
                Some(tp) => *tp = sp.clone(),
                None => doc.params.push(sp.clone()),
            }
        }
        for name in features {
            let f = src
                .feature(name)
                .ok_or_else(|| HistoryError::InvalidDocument(format!("feature '{name}' does not exist in the source")))?
                .clone();
            if let Some(i) = doc.feature_index(name) {
                if let Some(slot) = doc.features.get_mut(i) {
                    *slot = f;
                }
            } else {
                // Insert after the last dependency present in the target, else at the end.
                let deps = f.kind.dependencies();
                let pos = deps.iter().filter_map(|d| doc.feature_index(d)).max().map(|i| i + 1);
                match pos {
                    Some(i) if i < doc.features.len() => doc.features.insert(i, f),
                    _ => doc.features.push(f),
                }
            }
        }
        let issues = gusset_doc::validate::validate(&doc);
        if !issues.is_empty() {
            let msg: Vec<String> = issues.iter().map(ToString::to_string).collect();
            return Err(HistoryError::InvalidDocument(format!("cherry-pick result is invalid: {}", msg.join("; "))));
        }
        Ok(doc)
    }

    // ------------------------------------------------------------ garbage collection

    /// Bytes of history data (all stored objects).
    pub fn byte_size(&self) -> usize {
        self.objects.byte_size()
    }

    /// Purges expired deleted branches, squashes old unnamed intermediate nodes and drops
    /// unreachable nodes and objects. Never touches starred/named nodes, branch heads or
    /// nodes shared by several branches.
    pub fn gc(&mut self, now: u64, policy: &GcPolicy) -> GcReport {
        let mut rep = GcReport { bytes_before: self.objects.byte_size(), ..GcReport::default() };
        // 1. Purge expired soft-deleted branches.
        let expired: Vec<BranchId> = self
            .branches
            .values()
            .filter(|b| b.deleted_at.is_some_and(|t| now.saturating_sub(t) >= policy.deleted_retention))
            .map(|b| b.id)
            .collect();
        for b in expired {
            self.branches.remove(&b);
            rep.branches_purged += 1;
        }
        // 2. Squash old unnamed intermediate nodes.
        if let Some(age) = policy.squash_after {
            let heads: BTreeSet<NodeId> = self.branches.values().map(|b| b.head).collect();
            let mut child_count: BTreeMap<NodeId, usize> = BTreeMap::new();
            for n in self.nodes.values() {
                if let Some(p) = n.parent {
                    *child_count.entry(p).or_default() += 1;
                }
            }
            let victims: Vec<NodeId> = self
                .nodes
                .values()
                .filter(|n| {
                    n.parent.is_some()
                        && !n.starred
                        && n.name.is_none()
                        && !heads.contains(&n.id)
                        && child_count.get(&n.id).copied() == Some(1)
                        && now.saturating_sub(n.time) >= age
                })
                .map(|n| n.id)
                .collect();
            for v in victims {
                let Some(parent) = self.nodes.get(&v).and_then(|n| n.parent) else { continue };
                for n in self.nodes.values_mut() {
                    if n.parent == Some(v) {
                        n.parent = Some(parent);
                    }
                }
                self.nodes.remove(&v);
                rep.nodes_squashed += 1;
            }
        }
        // 3. Reachability from branch heads (incl. recently deleted) and starred/named nodes.
        let mut roots: Vec<NodeId> = self.branches.values().map(|b| b.head).collect();
        roots.extend(self.nodes.values().filter(|n| n.starred || n.name.is_some()).map(|n| n.id));
        let mut reachable = BTreeSet::new();
        for r in roots {
            reachable.extend(self.lineage(r));
        }
        let before = self.nodes.len();
        self.nodes.retain(|id, _| reachable.contains(id));
        rep.nodes_removed = before - self.nodes.len();
        // 4. Objects reachable from remaining snapshots.
        let mut keep = BTreeSet::new();
        for n in self.nodes.values() {
            self.objects.snapshot_closure(n.snapshot, &mut keep);
        }
        let before = self.objects.len();
        self.objects.retain(&keep);
        rep.objects_removed = before - self.objects.len();
        rep.bytes_after = self.objects.byte_size();
        // Undo stacks may reference removed nodes; they are session state, so drop them.
        self.undo.clear();
        self.redo.clear();
        rep
    }

    // ------------------------------------------------------------ persistence helpers

    pub(crate) fn from_parts(
        objects: ObjectStore,
        nodes: BTreeMap<NodeId, Node>,
        branches: BTreeMap<BranchId, Branch>,
        main: BranchId,
        events: Vec<EventRecord>,
    ) -> History {
        let next_node = nodes.keys().map(|n| n.0).max().unwrap_or(0) + 1;
        let next_branch = branches.keys().map(|b| b.0).max().unwrap_or(0) + 1;
        History {
            objects,
            nodes,
            branches,
            main,
            events,
            next_node,
            next_branch,
            undo: BTreeMap::new(),
            redo: BTreeMap::new(),
        }
    }
}
