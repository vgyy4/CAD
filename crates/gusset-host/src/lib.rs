//! The model thread (ADR 0012, "Session and concurrency").
//!
//! [`Host::spawn`] moves a [`Session`] (and optionally the on-disk
//! [`gusset_project::Project`]) onto one dedicated **model thread**. Every command from
//! the GUI, the CLI and MCP connections is queued to that thread and executed
//! **serially**, so edits from people and agents get a deterministic order and agent
//! edits appear live in the GUI.
//!
//! # Threading model
//!
//! - The session and project live in one `Mutex<State>` shared by the model thread and
//!   the [`HostHandle`]s.
//! - Commands ([`HostHandle::call`], [`HostHandle::call_async`]), saves and recovery
//!   snapshots are *jobs*: they are sent over a channel and run one at a time on the
//!   model thread, which holds the lock only while a single job runs.
//! - [`HostHandle::exec_detached`] queues a job without waiting; its callback gets the
//!   result. The GUI uses it (and [`HostHandle::try_with_session`], which never waits for
//!   a running job) so its UI thread never blocks behind a long command.
//! - [`HostHandle::with_session`] runs a short closure on the **calling** thread while
//!   holding the lock (between jobs). It is meant for quick read-only snapshots (the
//!   renderer fetching evaluated bodies, the GUI reading the feature tree). Keep the
//!   closure short: it blocks the model thread while it runs.
//! - Re-entrancy is detected instead of deadlocking: calling [`HostHandle::call`] (or any
//!   other job) from the model thread, or `with_session` from inside `with_session` or a
//!   job, returns a `CommandError` with code `reentrant`.
//! - The **quiescent** flag ([`HostHandle::is_quiescent`]) is true while no job is
//!   running. Recovery snapshots ([`HostHandle::snapshot`]) are themselves jobs, so they
//!   always run between two commands, on a consistent history.
//! - Change notifications ([`HostHandle::subscribe`]) are broadcast after every job that
//!   changed the history (commit, undo/redo, branch create/switch/set-main, ...). Slow
//!   subscribers miss old events (`RecvError::Lagged`) and should re-read the model.
//! - A panicking command is caught on the model thread and reported as a
//!   `CommandError` with code `internal`; the host keeps running.
//! - `import.file` never parses on the model thread: [`HostHandle::call`],
//!   [`HostHandle::call_async`] and [`HostHandle::import_detached`] run its three phases
//!   (open on the model thread, read and parse on a separate thread, commit on the model
//!   thread) and can be cancelled ([`gusset_commands::import`]).
//! - [`HostHandle::shutdown`] drains already-queued jobs, stops the thread and hands the
//!   session and project back to the caller (who decides whether to save and close).

use gusset_commands::import::{CancelFlag, begin_import, complete_import};
use gusset_commands::session::Change;
use gusset_commands::{CmdCtx, CommandError, Session, execute};
use gusset_history::{Actor, BranchId, NodeId};
use gusset_project::Project;
use serde_json::Value;
use std::cell::Cell;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{JoinHandle, ThreadId};
use tokio::sync::{broadcast, oneshot};

/// Capacity of the change broadcast channel (older events are dropped for slow
/// subscribers, who then see `Lagged` and should re-read the model).
pub const EVENT_CAPACITY: usize = 1024;

/// A notification for the GUI and MCP clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// The history changed: a commit, undo/redo, or a branch operation.
    Changed {
        /// Who made the change.
        actor: Actor,
        /// Branch that changed.
        branch: BranchId,
        /// The branch head after the change.
        node: NodeId,
        /// Transaction label (authored text: treat as untrusted when shown to agents).
        label: String,
    },
    /// The project was saved to its file.
    Saved,
    /// The host stopped; no further events follow.
    Shutdown,
}

impl From<Change> for HostEvent {
    fn from(c: Change) -> Self {
        HostEvent::Changed { actor: c.actor, branch: c.branch, node: c.node, label: c.label }
    }
}

struct State {
    session: Session,
    project: Option<Project>,
}

/// A job runs with the state locked and returns its reply, which is delivered only
/// after the lock is released, events are broadcast and the host is quiescent again.
type Reply = Box<dyn FnOnce() + Send>;
type Job = Box<dyn FnOnce(&mut State) -> Reply + Send>;

enum Msg {
    Job(Job),
    Shutdown,
}

struct Inner {
    state: Mutex<Option<State>>,
    tx: Mutex<Option<mpsc::Sender<Msg>>>,
    events: broadcast::Sender<HostEvent>,
    quiescent: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
    model_thread: Mutex<Option<ThreadId>>,
}

/// Namespace for [`Host::spawn`].
#[derive(Debug)]
pub struct Host;

/// A cheap, cloneable handle to the model thread (`Send + Sync`).
#[derive(Clone)]
pub struct HostHandle {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for HostHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostHandle").field("quiescent", &self.is_quiescent()).finish_non_exhaustive()
    }
}

thread_local! {
    /// True while this thread holds the state lock through the host (job or
    /// `with_session`), to turn re-entrant use into an error instead of a deadlock.
    static IN_HOST: Cell<bool> = const { Cell::new(false) };
}

struct InHostGuard;

impl InHostGuard {
    fn enter() -> Option<InHostGuard> {
        IN_HOST.with(|f| {
            if f.get() {
                None
            } else {
                f.set(true);
                Some(InHostGuard)
            }
        })
    }
}

impl Drop for InHostGuard {
    fn drop(&mut self) {
        IN_HOST.with(|f| f.set(false));
    }
}

fn reentrant() -> CommandError {
    CommandError::new("reentrant", "the model thread cannot wait for itself")
        .hint("do not call the host from inside with_session or a running command")
}

fn stopped() -> CommandError {
    CommandError::new("shutdown", "the project session has been closed")
}

fn internal(what: &str) -> CommandError {
    CommandError::new("internal", format!("{what} failed unexpectedly; the model was not changed by it"))
        .hint("this is a bug; please report it")
}

/// Holds the completion callback of [`HostHandle::exec_detached`]; if the job is dropped
/// without running (the host stopped first) it reports `internal` instead of going silent.
struct DoneGuard<R, D: FnOnce(Result<R, CommandError>)>(Option<D>, std::marker::PhantomData<fn(R)>);

impl<R, D: FnOnce(Result<R, CommandError>)> Drop for DoneGuard<R, D> {
    fn drop(&mut self) {
        if let Some(d) = self.0.take() {
            d(Err(internal("the command")));
        }
    }
}

fn is_import(id: &str) -> bool {
    gusset_commands::find(id).is_some_and(|s| s.id == "import.file")
}

/// Sets the flag when dropped (an abandoned async import stops its worker).
struct CancelOnDrop(CancelFlag);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn lock(m: &Mutex<Option<State>>) -> MutexGuard<'_, Option<State>> {
    // A panic inside a job is caught before it can poison the lock, but be robust anyway.
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Host {
    /// Starts the model thread for `session` (and its on-disk `project`, if any). If the
    /// OS refuses to create the thread, the returned handle is already shut down and every
    /// call fails with code `shutdown` (instead of hanging).
    #[must_use]
    pub fn spawn(session: Session, project: Option<Project>) -> HostHandle {
        let (tx, rx) = mpsc::channel::<Msg>();
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let inner = Arc::new(Inner {
            state: Mutex::new(Some(State { session, project })),
            tx: Mutex::new(Some(tx)),
            events,
            quiescent: AtomicBool::new(true),
            thread: Mutex::new(None),
            model_thread: Mutex::new(None),
        });
        let worker = Arc::downgrade(&inner);
        let spawned = std::thread::Builder::new().name("gusset-model".into()).spawn(move || model_loop(&worker, &rx));
        if let Ok(j) = spawned {
            let id = j.thread().id();
            *inner.model_thread.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id);
            *inner.thread.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(j);
        } else {
            // Without a thread nothing would ever run; mark the host stopped so callers get
            // a clear error instead of hanging.
            *inner.tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
        HostHandle { inner }
    }
}

fn model_loop(inner: &std::sync::Weak<Inner>, rx: &mpsc::Receiver<Msg>) {
    while let Ok(msg) = rx.recv() {
        let Some(inner) = inner.upgrade() else { return };
        match msg {
            Msg::Shutdown => return,
            Msg::Job(job) => {
                inner.quiescent.store(false, Ordering::SeqCst);
                let (reply, changes) = {
                    let _g = InHostGuard::enter();
                    let mut guard = lock(&inner.state);
                    match guard.as_mut() {
                        Some(state) => {
                            // A panicking job drops its reply sender; the caller sees
                            // `internal`. Catching keeps the model thread alive.
                            let reply = std::panic::catch_unwind(AssertUnwindSafe(|| job(state))).ok();
                            (reply, state.session.take_changes())
                        }
                        None => (None, Vec::new()),
                    }
                };
                for c in changes {
                    let _ = inner.events.send(HostEvent::from(c));
                }
                inner.quiescent.store(true, Ordering::SeqCst);
                if let Some(reply) = reply {
                    reply();
                }
            }
        }
    }
}

impl HostHandle {
    fn on_model_thread(&self) -> bool {
        let id = *self.inner.model_thread.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        id == Some(std::thread::current().id())
    }

    fn send(&self, job: Job) -> Result<(), CommandError> {
        let tx = self.inner.tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        tx.as_ref().ok_or_else(stopped)?.send(Msg::Job(job)).map_err(|_| stopped())
    }

    /// Queues `f` to run on the model thread with exclusive access to the session and
    /// project; `reply` receives its result (it is dropped unanswered if `f` panics).
    fn submit<R, F>(&self, f: F, reply: impl FnOnce(R) + Send + 'static) -> Result<(), CommandError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Session, Option<&mut Project>) -> R + Send + 'static,
    {
        if self.on_model_thread() || IN_HOST.with(Cell::get) {
            return Err(reentrant());
        }
        self.send(Box::new(move |st: &mut State| {
            let r = f(&mut st.session, st.project.as_mut());
            Box::new(move || reply(r)) as Reply
        }))
    }

    /// Runs `f` on the model thread, blocking the caller until it finished. Inside an
    /// async task use [`HostHandle::exec_async`] instead, which does not block the runtime.
    ///
    /// # Errors
    /// `shutdown` after the host stopped, `reentrant` if called from the model thread or
    /// inside `with_session`, `internal` if `f` panicked.
    pub fn exec<R, F>(&self, f: F) -> Result<R, CommandError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Session, Option<&mut Project>) -> R + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel(1);
        self.submit(f, move |r| {
            let _ = tx.send(r);
        })?;
        rx.recv().map_err(|_| internal("the command"))
    }

    /// Async version of [`HostHandle::exec`]: the model thread does the work, the async
    /// task only awaits the result (it never blocks the runtime).
    ///
    /// # Errors
    /// As [`HostHandle::exec`].
    pub async fn exec_async<R, F>(&self, f: F) -> Result<R, CommandError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Session, Option<&mut Project>) -> R + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.submit(f, move |r| {
            let _ = tx.send(r);
        })?;
        rx.await.map_err(|_| internal("the command"))
    }

    /// Queues `f` on the model thread **without waiting** for it. `done` receives the
    /// result on the model thread right after the job (with the lock released and change
    /// events already broadcast), so keep it short: send the value over a channel and wake
    /// the receiver. If `f` panics, `done` receives a `CommandError` with code `internal`.
    /// Jobs queued from one thread run in the order they were queued. A UI thread uses
    /// this to stay responsive while long commands run.
    ///
    /// `done` is called exactly once in every case: also when the job cannot be queued
    /// (then with the same error this function returns).
    ///
    /// # Errors
    /// `shutdown` after the host stopped and `reentrant` from the model thread or inside
    /// `with_session`.
    pub fn exec_detached<R, F, D>(&self, f: F, done: D) -> Result<(), CommandError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Session, Option<&mut Project>) -> R + Send + 'static,
        D: FnOnce(Result<R, CommandError>) + Send + 'static,
    {
        let early = if self.on_model_thread() || IN_HOST.with(Cell::get) {
            Some(reentrant())
        } else if self.is_shut_down() {
            Some(stopped())
        } else {
            None
        };
        if let Some(e) = early {
            done(Err(e.clone()));
            return Err(e);
        }
        let mut guard = DoneGuard(Some(done), std::marker::PhantomData);
        self.send(Box::new(move |st: &mut State| {
            // Caught here (not only in the model loop) so `done` still runs after the
            // job, outside the lock, like a normal reply.
            let r = std::panic::catch_unwind(AssertUnwindSafe(|| f(&mut st.session, st.project.as_mut())))
                .map_err(|_| internal("the command"));
            Box::new(move || {
                if let Some(d) = guard.0.take() {
                    d(r);
                }
            }) as Reply
        }))
    }

    /// Executes a registry command (policy, approvals and audit included) on the model
    /// thread and waits for the result.
    ///
    /// # Errors
    /// The command's own error, or the errors of [`HostHandle::exec`].
    pub fn call(&self, ctx: CmdCtx, id: &str, args: Value) -> Result<Value, CommandError> {
        if is_import(id) {
            return self.import(ctx, args, &CancelFlag::default());
        }
        let id = id.to_string();
        self.exec(move |s, _| execute(s, &ctx, &id, args))?
    }

    /// Async version of [`HostHandle::call`] for tokio (MCP connections).
    ///
    /// # Errors
    /// As [`HostHandle::call`].
    pub async fn call_async(&self, ctx: CmdCtx, id: &str, args: Value) -> Result<Value, CommandError> {
        if is_import(id) {
            return self.import_async(ctx, args).await;
        }
        let id = id.to_string();
        self.exec_async(move |s, _| execute(s, &ctx, &id, args)).await?
    }

    /// `import.file` with the parsing on the **calling** thread (never the model thread):
    /// the model thread only opens the file and later commits the result. Set `cancel`
    /// from another thread to stop it (the import worker is killed).
    ///
    /// # Errors
    /// The import's errors (`cancelled` when cancelled), plus those of [`HostHandle::exec`].
    pub fn import(&self, ctx: CmdCtx, args: Value, cancel: &CancelFlag) -> Result<Value, CommandError> {
        let c1 = ctx.clone();
        let job = self.exec(move |s, _| begin_import(s, &c1, args))??;
        let prepared = job.run(cancel);
        self.exec(move |s, _| complete_import(s, &ctx, prepared))?
    }

    /// Async [`HostHandle::import`] for MCP: the parse runs on its own thread, and
    /// dropping the future (the client cancelled the request) cancels the import.
    ///
    /// # Errors
    /// As [`HostHandle::import`].
    pub async fn import_async(&self, ctx: CmdCtx, args: Value) -> Result<Value, CommandError> {
        let cancel = CancelFlag::default();
        let _guard = CancelOnDrop(cancel.clone());
        let c1 = ctx.clone();
        let job = self.exec_async(move |s, _| begin_import(s, &c1, args)).await??;
        let (tx, rx) = oneshot::channel();
        let c2 = cancel.clone();
        std::thread::Builder::new()
            .name("gusset-import".into())
            .spawn(move || {
                let _ = tx.send(job.run(&c2));
            })
            .map_err(|_| internal("starting the import"))?;
        let prepared = rx.await.map_err(|_| internal("the import"))?;
        self.exec_async(move |s, _| complete_import(s, &ctx, prepared)).await?
    }

    /// [`HostHandle::import`] on a new thread, for the GUI: returns at once; `done`
    /// receives the result (on the import thread). Keep `cancel` to offer a Cancel button.
    ///
    /// # Errors
    /// When the thread cannot be started (`done` is then not called).
    pub fn import_detached<D>(&self, ctx: CmdCtx, args: Value, cancel: CancelFlag, done: D) -> Result<(), CommandError>
    where
        D: FnOnce(Result<Value, CommandError>) + Send + 'static,
    {
        let h = self.clone();
        std::thread::Builder::new()
            .name("gusset-import".into())
            .spawn(move || done(h.import(ctx, args, &cancel)))
            .map(|_| ())
            .map_err(|_| internal("starting the import"))
    }

    /// Runs a short closure on the calling thread with the session locked (between
    /// jobs). Intended for read-only snapshots (GUI, renderer). Mutations made here are
    /// **not** broadcast; use commands to change the model.
    ///
    /// # Errors
    /// `shutdown` after the host stopped; `reentrant` when nested.
    pub fn with_session<R>(&self, f: impl FnOnce(&mut Session) -> R) -> Result<R, CommandError> {
        let _g = InHostGuard::enter().ok_or_else(reentrant)?;
        let mut guard = lock(&self.inner.state);
        let st = guard.as_mut().ok_or_else(stopped)?;
        Ok(f(&mut st.session))
    }

    /// Like [`HostHandle::with_session`], but never waits: returns `Ok(None)` while a job
    /// holds the session (a long command is running). A UI thread uses this so a long
    /// command never freezes it; it retries on a later frame.
    ///
    /// # Errors
    /// `shutdown` after the host stopped; `reentrant` when nested.
    pub fn try_with_session<R>(&self, f: impl FnOnce(&mut Session) -> R) -> Result<Option<R>, CommandError> {
        let _g = InHostGuard::enter().ok_or_else(reentrant)?;
        let mut guard = match self.inner.state.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Ok(None),
        };
        let st = guard.as_mut().ok_or_else(stopped)?;
        Ok(Some(f(&mut st.session)))
    }

    /// Subscribes to change notifications.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<HostEvent> {
        self.inner.events.subscribe()
    }

    /// True while no command or other job is running on the model thread.
    #[must_use]
    pub fn is_quiescent(&self) -> bool {
        self.inner.quiescent.load(Ordering::SeqCst)
    }

    /// Saves the project to its file (on the model thread, between commands).
    ///
    /// # Errors
    /// `not_allowed` without a project or file (use Save As in the GUI), `io` on write
    /// errors, plus the errors of [`HostHandle::exec`].
    pub fn save(&self) -> Result<(), CommandError> {
        let events = self.inner.events.clone();
        self.exec(move |s, p| {
            let p = p.ok_or_else(|| CommandError::new("not_allowed", "this session has no project file"))?;
            if p.path().is_none() {
                return Err(CommandError::new("not_allowed", "the project has no file yet").hint("use Save As"));
            }
            p.save(s.history()).map_err(|e| CommandError::new("io", e.to_string()))?;
            s.dirty = false;
            let _ = events.send(HostEvent::Saved);
            Ok(())
        })?
    }

    /// Writes a crash-recovery snapshot if there are unsaved changes. Runs as a job, so
    /// it always sees a quiescent, consistent history. Returns the snapshot sequence
    /// number, or `None` if nothing needed saving.
    ///
    /// # Errors
    /// `io` on write errors, plus the errors of [`HostHandle::exec`].
    pub fn snapshot(&self) -> Result<Option<u64>, CommandError> {
        self.exec(|s, p| match p {
            Some(p) if s.dirty => p.snapshot(s.history()).map(Some).map_err(|e| CommandError::new("io", e.to_string())),
            _ => Ok(None),
        })?
    }

    /// True once [`HostHandle::shutdown`] ran (or the model thread could not start).
    #[must_use]
    pub fn is_shut_down(&self) -> bool {
        self.inner.tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_none()
    }

    /// Stops the model thread after the jobs already queued, and returns the session and
    /// project to the caller (only the first caller gets them; later calls return
    /// `None`). The caller decides whether to save and how to close the project.
    #[must_use]
    pub fn shutdown(&self) -> Option<(Session, Option<Project>)> {
        if self.on_model_thread() || IN_HOST.with(Cell::get) {
            return None;
        }
        let tx = self.inner.tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(tx) = tx {
            let _ = tx.send(Msg::Shutdown);
        }
        let join = self.inner.thread.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(j) = join {
            let _ = j.join();
            let _ = self.inner.events.send(HostEvent::Shutdown);
        }
        lock(&self.inner.state).take().map(|st| (st.session, st.project))
    }
}

#[cfg(test)]
mod tests;
