//! A live project session: history, evaluation, per-actor branches, policy and audit.

use crate::error::CommandError;
use gusset_doc::Document;
use gusset_eval::{BlobSource, Evaluation, Evaluator};
use gusset_history::{Actor, BranchId, Ctx, Hash, History, NodeId, NodeState};
use gusset_kernel::Kernel;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

/// Context of one command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdCtx {
    pub actor: Actor,
    pub time: u64,
    /// Set **only** by the trusted layer (GUI prompt or MCP elicitation answered by the
    /// human). Agents can never set it through tool arguments.
    pub approved: bool,
    /// How approval was given when not by a prompt answer for this very call (a standing
    /// "always allow" grant); recorded in the audit log. Set only by the trusted layer.
    pub approved_by: Option<String>,
}

impl CmdCtx {
    pub fn user(time: u64) -> Self {
        CmdCtx { actor: Actor::User, time, approved: true, approved_by: None }
    }
    pub fn agent(name: &str, time: u64) -> Self {
        CmdCtx { actor: Actor::Agent(name.to_string()), time, approved: false, approved_by: None }
    }
    pub fn is_agent(&self) -> bool {
        matches!(self.actor, Actor::Agent(_))
    }
    pub(crate) fn hctx(&self) -> Ctx {
        Ctx { actor: self.actor.clone(), time: self.time }
    }
}

/// Branch heads whose evaluation the session keeps.
const EVAL_CACHE_BRANCHES: usize = 8;

/// Session policy (Settings; never changeable by agents).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    /// Require human approval for high-impact agent actions (default on).
    pub approvals_required: bool,
    /// Reject every mutating agent command.
    pub read_only_agents: bool,
    /// Agent edits touching more items than this need approval.
    pub bulk_threshold: usize,
    /// Maximum agent commands per minute per agent.
    pub agent_rate_per_minute: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            approvals_required: true,
            read_only_agents: false,
            bulk_threshold: 25,
            agent_rate_per_minute: 600,
        }
    }
}

/// One audited agent action (visible in the app; spec "Keep an audit log").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub time: u64,
    pub actor: Actor,
    pub command: String,
    pub branch: Option<String>,
    pub outcome: String,
}

/// Change notification for the GUI and MCP clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub actor: Actor,
    pub branch: BranchId,
    pub node: NodeId,
    pub label: String,
}

/// Imports exact-geometry files (STEP/IGES) into kernel-native B-rep bodies. The app
/// and CLI plug in the sandboxed out-of-process worker (`gusset_worker::WorkerImporter`,
/// ADR 0008); tests use doubles. Called off the model thread (see [`crate::import`]).
pub trait Importer: Send + Sync {
    /// Returns (body name, native B-rep bytes) per body. Should stop soon after `cancel`
    /// becomes true (the worker is killed).
    fn import(
        &self,
        format: &str,
        bytes: &[u8],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<(String, Vec<u8>)>, String>;
}

pub struct Session {
    pub importer: Option<Arc<dyn Importer>>,
    history: History,
    evaluator: Evaluator,
    current: BTreeMap<Actor, BranchId>,
    pub config: SessionConfig,
    audit: Vec<AuditEntry>,
    /// Recent evaluations by (node, rollback), most recent first. Several entries so a
    /// GUI and agents on different branches do not evict each other (review M10).
    cache: VecDeque<(NodeId, Option<usize>, Arc<Evaluation>)>,
    rate: BTreeMap<Actor, VecDeque<u64>>,
    /// Project folder: the only place file commands may read/write (spec: MCP scope).
    pub project_dir: Option<PathBuf>,
    /// True when there are changes not yet written to the project file.
    pub dirty: bool,
    changes: Vec<Change>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("main", &self.history.main()).field("dirty", &self.dirty).finish()
    }
}

struct HistoryBlobs<'a>(&'a History);

impl BlobSource for HistoryBlobs<'_> {
    fn blob(&self, hash_hex: &str) -> Option<Arc<[u8]>> {
        let h = Hash::from_hex(hash_hex)?;
        self.0.objects().get(h).ok().cloned()
    }
}

impl Session {
    pub fn new(doc: &Document, kernel: Arc<dyn Kernel>, ctx: &CmdCtx) -> Session {
        Session::from_history(History::new(doc, &ctx.hctx()), kernel)
    }

    pub fn from_history(history: History, kernel: Arc<dyn Kernel>) -> Session {
        // The persisted part of the agent audit log comes back with the project.
        let audit = history
            .events()
            .iter()
            .filter_map(|e| match &e.event {
                gusset_history::Event::AgentAction { command, branch, outcome, .. } => Some(AuditEntry {
                    time: e.time,
                    actor: e.actor.clone(),
                    command: command.clone(),
                    branch: branch.and_then(|b| history.branch(b).ok()).map(|b| b.name.clone()),
                    outcome: outcome.clone(),
                }),
                _ => None,
            })
            .collect();
        Session {
            importer: None,
            history,
            evaluator: Evaluator::new(kernel),
            current: BTreeMap::new(),
            config: SessionConfig::default(),
            audit,
            cache: VecDeque::new(),
            rate: BTreeMap::new(),
            project_dir: None,
            dirty: false,
            changes: Vec::new(),
        }
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub(crate) fn history_mut(&mut self) -> &mut History {
        self.dirty = true;
        &mut self.history
    }

    pub fn kernel(&self) -> &Arc<dyn Kernel> {
        self.evaluator.kernel()
    }

    pub fn audit_log(&self) -> &[AuditEntry] {
        &self.audit
    }

    pub(crate) fn audit(&mut self, e: AuditEntry) {
        self.audit.push(e);
        if self.audit.len() > 100_000 {
            self.audit.drain(..10_000);
        }
    }

    /// Takes the pending change notifications.
    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    /// The branch `actor` works on. Agents get their own `agent/<name>` branch from main
    /// on first use (ADR 0012); the user starts on main.
    pub fn branch_for(&mut self, ctx: &CmdCtx) -> Result<BranchId, CommandError> {
        if let Some(b) = self.current.get(&ctx.actor)
            && self.history.branch(*b).is_ok_and(|x| x.deleted_at.is_none())
        {
            return Ok(*b);
        }
        let b = match &ctx.actor {
            Actor::User => self.history.main(),
            Actor::Agent(name) => {
                let base = format!("agent/{name}");
                // Only a branch this agent created, and never main: a branch merely
                // *named* like the agent's (e.g. main renamed to it) is not reused.
                let main = self.history.main();
                let existing = self
                    .history
                    .branch_by_name(&base)
                    .filter(|b| b.id != main && b.created_by == ctx.actor)
                    .map(|b| b.id);
                match existing {
                    Some(b) => b,
                    None => {
                        let head = self.history.head(self.history.main()).map_err(CommandError::history)?;
                        let name = (1..)
                            .map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") })
                            .find(|n| self.history.branch_by_name_folded(n).is_none())
                            .unwrap_or(base);
                        self.history_mut()
                            .create_branch(&ctx.hctx(), head, Some(&name))
                            .map_err(CommandError::history)?
                    }
                }
            }
        };
        self.current.insert(ctx.actor.clone(), b);
        Ok(b)
    }

    /// The branch `actor` is on, without creating anything (for read-only queries).
    pub fn current_branch(&self, actor: &Actor) -> BranchId {
        self.current
            .get(actor)
            .copied()
            .filter(|b| self.history.branch(*b).is_ok_and(|x| x.deleted_at.is_none()))
            .unwrap_or_else(|| self.history.main())
    }

    /// Whether the actor has chosen (or been given) a branch already.
    pub fn current_is_explicit(&self, actor: &Actor) -> bool {
        self.current.contains_key(actor)
    }

    pub fn switch_branch(&mut self, actor: &Actor, b: BranchId) -> Result<(), CommandError> {
        let br = self.history.branch(b).map_err(CommandError::history)?;
        if br.deleted_at.is_some() {
            return Err(CommandError::new("not_found", format!("branch {b} is deleted"))
                .hint("restore it from Recently deleted first")
                .details(serde_json::json!({ "name_user_text": br.name })));
        }
        self.current.insert(actor.clone(), b);
        Ok(())
    }

    /// The document at the head of the actor's branch.
    pub fn document(&self, actor: &Actor) -> Result<Document, CommandError> {
        self.history.head_document(self.current_branch(actor)).map_err(CommandError::history)
    }

    /// Evaluates the head of `branch`, cached by node and rollback marker.
    pub fn evaluate_branch(&mut self, branch: BranchId) -> Result<Arc<Evaluation>, CommandError> {
        let b = self.history.branch(branch).map_err(CommandError::history)?;
        let (node, rollback) = (b.head, b.rollback);
        if let Some(i) = self.cache.iter().position(|(n, r, _)| *n == node && *r == rollback)
            && let Some(hit) = self.cache.remove(i)
        {
            let e = hit.2.clone();
            self.cache.push_front(hit);
            return Ok(e);
        }
        let doc = self.history.document(node).map_err(CommandError::history)?;
        let blobs = HistoryBlobs(&self.history);
        let e = Arc::new(self.evaluator.evaluate(&doc, rollback, &blobs));
        self.remember_eval(node, rollback, e.clone());
        Ok(e)
    }

    pub fn evaluate(&mut self, actor: &Actor) -> Result<Arc<Evaluation>, CommandError> {
        let b = self.current_branch(actor);
        self.evaluate_branch(b)
    }

    /// Evaluates an arbitrary document (e.g. for previews) without committing it.
    pub fn evaluate_doc(&mut self, doc: &Document, rollback: Option<usize>) -> Evaluation {
        let blobs = HistoryBlobs(&self.history);
        self.evaluator.evaluate(doc, rollback, &blobs)
    }

    /// Commits a new document on the actor's branch as one undoable transaction. The
    /// node is marked broken if the edit introduced feature errors (the history keeps it
    /// so the user can fix or discard it; spec "Robustness").
    pub fn commit(&mut self, ctx: &CmdCtx, label: &str, doc: &Document) -> Result<CommitOutcome, CommandError> {
        self.commit_with_rollback(ctx, label, doc, None)
    }

    /// [`Session::commit`]; `new_rollback` is the marker the branch will have after this
    /// edit (set by the caller right after), so the new document is evaluated, its errors
    /// recorded and its node state decided as it will actually be shown (review N1).
    pub(crate) fn commit_with_rollback(
        &mut self,
        ctx: &CmdCtx,
        label: &str,
        doc: &Document,
        new_rollback: Option<Option<usize>>,
    ) -> Result<CommitOutcome, CommandError> {
        let issues = gusset_doc::validate::validate(doc);
        if !issues.is_empty() {
            return Err(CommandError::invalid_document(&issues));
        }
        let branch = self.branch_for(ctx)?;
        let before: Vec<String> = self.evaluate_branch(branch)?.errors().iter().map(|e| e.to_string()).collect();
        let current = self.history.branch(branch).map_err(CommandError::history)?.rollback;
        let rollback = new_rollback.unwrap_or(current);
        let eval = Arc::new(self.evaluate_doc(doc, rollback));
        let new_errors: Vec<String> =
            eval.errors().iter().map(|e| e.to_string()).filter(|e| !before.contains(e)).collect();
        let state = if new_errors.is_empty() {
            NodeState::Ok
        } else {
            NodeState::Broken { message: new_errors.join("; ").chars().take(2000).collect() }
        };
        let node = self.history_mut().commit(&ctx.hctx(), branch, doc, label, state).map_err(CommandError::history)?;
        self.remember_eval(node, rollback, eval.clone());
        self.changes.push(Change { actor: ctx.actor.clone(), branch, node, label: label.to_string() });
        Ok(CommitOutcome { node, branch, evaluation: eval, new_errors })
    }

    /// Records a rate-limit hit; returns false if the actor exceeded its budget.
    pub(crate) fn rate_ok(&mut self, ctx: &CmdCtx) -> bool {
        if !ctx.is_agent() {
            return true;
        }
        let limit = self.config.agent_rate_per_minute;
        let q = self.rate.entry(ctx.actor.clone()).or_default();
        while q.front().is_some_and(|t| ctx.time.saturating_sub(*t) >= 60) {
            q.pop_front();
        }
        if q.len() >= limit {
            return false;
        }
        q.push_back(ctx.time);
        true
    }

    /// Stores imported data once, content-addressed (shared by every branch).
    pub(crate) fn put_blob(&mut self, bytes: &[u8]) -> gusset_history::Hash {
        self.history_mut().put_blob(bytes)
    }

    pub(crate) fn invalidate_cache(&mut self) {
        self.cache.clear();
    }

    fn remember_eval(&mut self, node: NodeId, rollback: Option<usize>, e: Arc<Evaluation>) {
        self.cache.retain(|(n, r, _)| !(*n == node && *r == rollback));
        self.cache.push_front((node, rollback, e));
        self.cache.truncate(EVAL_CACHE_BRANCHES);
    }

    pub(crate) fn note_change(&mut self, ctx: &CmdCtx, branch: BranchId, label: &str) {
        if let Ok(b) = self.history.branch(branch) {
            let node = b.head;
            self.changes.push(Change { actor: ctx.actor.clone(), branch, node, label: label.to_string() });
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommitOutcome {
    pub node: NodeId,
    pub branch: BranchId,
    pub evaluation: Arc<Evaluation>,
    /// Feature errors this edit introduced (empty = the edit rebuilt cleanly).
    pub new_errors: Vec<String>,
}

impl Session {
    /// Mutable history access for commands (marks the session dirty and drops the
    /// evaluation cache, since heads may move).
    pub(crate) fn history_mut_pub(&mut self) -> &mut gusset_history::History {
        self.invalidate_cache();
        self.history_mut()
    }
}
