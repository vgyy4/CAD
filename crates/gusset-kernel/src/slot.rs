//! Which kernel worker the work on this thread prefers (ADR 0024 §3): a kernel made of
//! several workers puts new shapes on the worker of the calling thread's slot. Others
//! ignore it.

use std::cell::Cell;

thread_local! {
    static SLOT: Cell<usize> = const { Cell::new(0) };
}

/// Runs `f` with `slot` as this thread's worker slot.
pub fn scope<R>(slot: usize, f: impl FnOnce() -> R) -> R {
    struct Restore(usize);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0;
            SLOT.with(|s| s.set(prev));
        }
    }
    let _restore = Restore(SLOT.with(|s| s.replace(slot)));
    f()
}

/// This thread's worker slot (0 outside any [`scope`]).
#[must_use]
pub fn current() -> usize {
    SLOT.with(Cell::get)
}
