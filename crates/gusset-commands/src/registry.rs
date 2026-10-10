//! The single command registry (ADR 0012). The GUI toolbar, marking menu, command
//! search, shortcuts and MCP `tools/list` are all generated from [`registry()`].

use crate::error::CommandError;
use crate::session::{AuditEntry, CmdCtx, Session};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Where a command shows up in the GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Document,
    Parameters,
    Sketch,
    Solid,
    Timeline,
    Branches,
    Inspect,
    Exchange,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Document => "document",
            Category::Parameters => "parameters",
            Category::Sketch => "sketch",
            Category::Solid => "solid",
            Category::Timeline => "timeline",
            Category::Branches => "branches",
            Category::Inspect => "inspect",
            Category::Exchange => "exchange",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    /// Changes the model or history.
    pub mutating: bool,
    /// Removes something (still undoable); needs an explicit confirm flag.
    pub destructive: bool,
    /// Agents need human approval (set main, delete branches, overwrite exports...).
    pub agent_approval: bool,
    /// Agents may not run this at all.
    pub user_only: bool,
}

pub const READ: Flags = Flags { mutating: false, destructive: false, agent_approval: false, user_only: false };
pub const EDIT: Flags = Flags { mutating: true, destructive: false, agent_approval: false, user_only: false };

/// A typed command: arguments are a JSON-schema'd struct.
pub trait Command: DeserializeOwned + JsonSchema {
    const ID: &'static str;
    const TITLE: &'static str;
    const DESCRIPTION: &'static str;
    const CATEGORY: Category;
    const FLAGS: Flags;
    const SHORTCUT: Option<&'static str> = None;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError>;
}

type RunFn = fn(&mut Session, &CmdCtx, Value) -> Result<Value, CommandError>;

/// Registry entry with metadata, schema and an erased runner.
#[derive(Clone)]
pub struct CommandSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub category: Category,
    pub flags: Flags,
    pub shortcut: Option<&'static str>,
    pub schema: fn() -> Value,
    run: RunFn,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec").field("id", &self.id).finish()
    }
}

impl CommandSpec {
    /// MCP tool name: ids with `.` replaced by `_` (`feature.extrude` → `feature_extrude`).
    pub fn tool_name(&self) -> String {
        self.id.replace('.', "_")
    }
}

fn run_erased<C: Command>(s: &mut Session, ctx: &CmdCtx, v: Value) -> Result<Value, CommandError> {
    let v = if v.is_null() { Value::Object(Default::default()) } else { v };
    let args: C = serde_json::from_value(v).map_err(|e| CommandError::args(format!("{}: {e}", C::ID)))?;
    args.run(s, ctx)
}

fn schema_of<C: Command>() -> Value {
    serde_json::to_value(schemars::schema_for!(C)).unwrap_or(Value::Null)
}

pub fn spec<C: Command>() -> CommandSpec {
    CommandSpec {
        id: C::ID,
        title: C::TITLE,
        description: C::DESCRIPTION,
        category: C::CATEGORY,
        flags: C::FLAGS,
        shortcut: C::SHORTCUT,
        schema: schema_of::<C>,
        run: run_erased::<C>,
    }
}

/// All commands, in a stable order.
pub fn registry() -> &'static [CommandSpec] {
    static REG: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    REG.get_or_init(crate::commands::all)
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    let id2 = id.replace('_', ".");
    registry().iter().find(|c| c.id == id || c.id == id2 || c.tool_name() == id)
}

/// Executes a command with policy enforcement (read-only agents, rate limits, approvals)
/// and audit logging for agents.
pub fn execute(s: &mut Session, ctx: &CmdCtx, id: &str, args: Value) -> Result<Value, CommandError> {
    let spec = find(id).ok_or_else(|| {
        CommandError::not_found(format!("unknown command '{id}'")).hint("use the command search to list commands")
    })?;
    let r = check_policy(s, ctx, spec).and_then(|()| (spec.run)(s, ctx, args));
    if ctx.is_agent() {
        let branch = s.history().branch(s.current_branch(&ctx.actor)).ok().map(|b| b.name.clone());
        let outcome = match &r {
            Ok(_) => "ok".to_string(),
            Err(e) => format!("error: {}", e.code),
        };
        s.audit(AuditEntry { time: ctx.time, actor: ctx.actor.clone(), command: spec.id.to_string(), branch, outcome });
    }
    r
}

fn check_policy(s: &mut Session, ctx: &CmdCtx, spec: &CommandSpec) -> Result<(), CommandError> {
    if !ctx.is_agent() {
        return Ok(());
    }
    if !s.rate_ok(ctx) {
        return Err(CommandError::new("rate_limited", "too many requests; slow down").hint("wait a minute and retry"));
    }
    if spec.flags.user_only {
        return Err(CommandError::new("not_allowed", format!("'{}' is only available to the user", spec.id)));
    }
    if spec.flags.mutating && s.config.read_only_agents {
        return Err(CommandError::new("read_only", "agents are in read-only mode for this project")
            .hint("the user can turn off read-only agent mode in the app"));
    }
    if spec.flags.agent_approval && s.config.approvals_required && !ctx.approved {
        return Err(CommandError::approval(spec.title));
    }
    if spec.flags.mutating && s.config.approvals_required && !ctx.approved {
        // Agents edit their own branch by default; writing to main needs approval.
        if s.current_is_explicit(&ctx.actor) && s.current_branch(&ctx.actor) == s.history().main() {
            return Err(CommandError::approval("editing the main branch"));
        }
    }
    Ok(())
}
