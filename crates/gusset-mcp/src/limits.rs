//! Rate limits per agent identity, across connections (security review #10, #15; dogfood
//! S6; code review L6).
//!
//! An agent's *identity* is its client name without the per-connection counter
//! (`claude-code-3` → `claude-code`), so reconnecting does not reset its budget. Every
//! budget also has a global share for all agents together (one bearer token is one
//! principal; client names are self-chosen). Reads are limited too: `render_view`
//! (expensive: evaluate, tessellate, render), `get_text` and resources, with at most
//! [`MAX_CONCURRENT_RENDERS`] renders at a time. A refusal says when to retry.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// What a request costs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// Registry commands (edits and queries).
    Command,
    /// `render_view`.
    Render,
    /// `get_text`.
    Text,
    /// `resources/read`.
    Resource,
}

impl Class {
    fn label(self) -> &'static str {
        match self {
            Class::Command => "tool calls",
            Class::Render => "render_view calls",
            Class::Text => "get_text calls",
            Class::Resource => "resource reads",
        }
    }
}

/// Requests per minute: (per agent identity, all agents together).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    pub command: (usize, usize),
    pub render: (usize, usize),
    pub text: (usize, usize),
    pub resource: (usize, usize),
}

impl Default for Budgets {
    fn default() -> Self {
        Budgets { command: (600, 1200), render: (30, 60), text: (300, 600), resource: (300, 600) }
    }
}

impl Budgets {
    fn of(&self, c: Class) -> (usize, usize) {
        match c {
            Class::Command => self.command,
            Class::Render => self.render,
            Class::Text => self.text,
            Class::Resource => self.resource,
        }
    }
}

/// Renders running at once (all agents).
pub const MAX_CONCURRENT_RENDERS: usize = 2;
/// Identities tracked at once; beyond this they share one overflow budget.
const MAX_IDENTITIES: usize = 1024;
const WINDOW: Duration = Duration::from_secs(60);
const GLOBAL: &str = "\0all";
const OVERFLOW: &str = "\0overflow";

/// A refusal: retry after this many whole seconds (at least 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryAfter(pub u64);

/// The agent identity behind an agent name: the name without a trailing `-<n>`.
#[must_use]
pub fn identity_of(agent: &str) -> &str {
    match agent.rsplit_once('-') {
        Some((base, n)) if !base.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => agent,
    }
}

/// Sliding-window limiter keyed by (identity, class), plus a global window per class.
#[derive(Debug)]
pub struct RateLimiter {
    budgets: Budgets,
    windows: Mutex<HashMap<(String, Class), VecDeque<Instant>>>,
    renders: AtomicUsize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        RateLimiter::new(Budgets::default())
    }
}

/// Holds one of the [`MAX_CONCURRENT_RENDERS`] render slots.
#[derive(Debug)]
pub struct RenderSlot<'a>(&'a AtomicUsize);

impl Drop for RenderSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn prune(q: &mut VecDeque<Instant>, now: Instant) {
    while q.front().is_some_and(|t| now.duration_since(*t) >= WINDOW) {
        q.pop_front();
    }
}

fn retry(q: &VecDeque<Instant>, now: Instant) -> RetryAfter {
    let wait = q.front().map_or(Duration::from_secs(1), |t| WINDOW.saturating_sub(now.duration_since(*t)));
    RetryAfter(wait.as_secs().max(1))
}

impl RateLimiter {
    #[must_use]
    pub fn new(budgets: Budgets) -> Self {
        RateLimiter { budgets, windows: Mutex::new(HashMap::new()), renders: AtomicUsize::new(0) }
    }

    /// Counts one request of `class` by `agent` (an agent name such as `claude-code-2`).
    ///
    /// # Errors
    /// [`RetryAfter`] when the identity's or the global budget is used up.
    pub fn check(&self, agent: &str, class: Class) -> Result<(), RetryAfter> {
        self.check_at(agent, class, Instant::now())
    }

    fn check_at(&self, agent: &str, class: Class, now: Instant) -> Result<(), RetryAfter> {
        let (per, all) = self.budgets.of(class);
        let mut m = self.windows.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut id = identity_of(agent).to_string();
        if !m.contains_key(&(id.clone(), class)) && m.len() >= MAX_IDENTITIES {
            m.retain(|_, q| {
                prune(q, now);
                !q.is_empty()
            });
            if m.len() >= MAX_IDENTITIES {
                id = OVERFLOW.to_string();
            }
        }
        let global = m.entry((GLOBAL.to_string(), class)).or_default();
        prune(global, now);
        if global.len() >= all {
            return Err(retry(global, now));
        }
        let q = m.entry((id, class)).or_default();
        prune(q, now);
        if q.len() >= per {
            return Err(retry(q, now));
        }
        q.push_back(now);
        m.entry((GLOBAL.to_string(), class)).or_default().push_back(now);
        Ok(())
    }

    /// Takes a render slot, or `None` while [`MAX_CONCURRENT_RENDERS`] renders run.
    pub fn render_slot(&self) -> Option<RenderSlot<'_>> {
        let prev = self.renders.fetch_add(1, Ordering::SeqCst);
        let slot = RenderSlot(&self.renders);
        (prev < MAX_CONCURRENT_RENDERS).then_some(slot)
    }

    /// The error returned to the agent.
    #[must_use]
    pub fn error(class: Class, r: RetryAfter) -> gusset_commands::CommandError {
        gusset_commands::CommandError::new(
            "rate_limited",
            format!("too many {} from this agent (or from all agents together); slow down", class.label()),
        )
        .hint(format!("wait {} s before retrying; do not reconnect to get around it", r.0))
        .details(serde_json::json!({ "retry_after_s": r.0 }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_drop_the_connection_counter() {
        assert_eq!(identity_of("claude-code-12"), "claude-code");
        assert_eq!(identity_of("claude-code"), "claude-code");
        assert_eq!(identity_of("agent"), "agent");
        assert_eq!(identity_of("-3"), "-3");
    }

    #[test]
    fn budgets_hold_across_connections_and_globally() {
        let b = Budgets { command: (3, 5), render: (1, 1), text: (1, 1), resource: (1, 1) };
        let r = RateLimiter::new(b);
        let t = Instant::now();
        for n in 1..=3 {
            r.check_at(&format!("bot-{n}"), Class::Command, t).unwrap();
        }
        // A fourth connection of the same client is still the same identity.
        let e = r.check_at("bot-4", Class::Command, t).unwrap_err();
        assert!(e.0 >= 1 && e.0 <= 60);
        // Other identities share the global budget.
        r.check_at("x-1", Class::Command, t).unwrap();
        r.check_at("y-1", Class::Command, t).unwrap();
        assert!(r.check_at("z-1", Class::Command, t).is_err(), "global budget");
        // Classes are separate, and the window slides.
        r.check_at("bot-1", Class::Render, t).unwrap();
        assert!(r.check_at("bot-2", Class::Render, t).is_err());
        r.check_at("bot-1", Class::Command, t + WINDOW).unwrap();
    }

    #[test]
    fn render_slots_are_bounded() {
        let r = RateLimiter::default();
        let a = r.render_slot().unwrap();
        let _b = r.render_slot().unwrap();
        assert!(r.render_slot().is_none());
        drop(a);
        assert!(r.render_slot().is_some());
    }
}
