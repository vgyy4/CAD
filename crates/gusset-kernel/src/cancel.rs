//! The cancel flag of the work running on this thread (ADR 0022 §3).
//!
//! A caller that can be stopped (the evaluator, for "Stop rebuild" or an MCP deadline)
//! runs its kernel calls inside [`scope`]. A kernel that can abandon an operation part way
//! (the worker process, which it kills) watches [`current`] while the call runs and fails
//! the call as [`crate::KernelErrorKind::Cancelled`]; in-process kernels ignore it.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

thread_local! {
    static CURRENT: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// Restores the previous flag when the scope ends, also on unwinding.
struct Restore(Option<Arc<AtomicBool>>);

impl Drop for Restore {
    fn drop(&mut self) {
        let prev = self.0.take();
        CURRENT.with(|c| *c.borrow_mut() = prev);
    }
}

/// Runs `f` with `flag` as this thread's cancel flag.
pub fn scope<R>(flag: &Arc<AtomicBool>, f: impl FnOnce() -> R) -> R {
    let prev = CURRENT.with(|c| c.borrow_mut().replace(flag.clone()));
    let _restore = Restore(prev);
    f()
}

/// This thread's cancel flag, if a [`scope`] is active.
#[must_use]
pub fn current() -> Option<Arc<AtomicBool>> {
    CURRENT.with(|c| c.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_nest_and_restore() {
        assert!(current().is_none());
        let (a, b) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(true)));
        scope(&a, || {
            assert!(current().is_some_and(|f| Arc::ptr_eq(&f, &a)));
            scope(&b, || assert!(current().is_some_and(|f| Arc::ptr_eq(&f, &b))));
            assert!(current().is_some_and(|f| Arc::ptr_eq(&f, &a)));
        });
        assert!(current().is_none());
    }
}
