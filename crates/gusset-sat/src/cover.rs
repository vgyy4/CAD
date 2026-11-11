//! Byte coverage of the Fusion design-stream readers: which bytes of a record a reader took as a
//! known field. The readers call [`field`] and [`skip`] where they accept a value; outside a
//! [`Recording`] (every production read) the calls do nothing. The coverage dumper
//! (`fusion_tree::coverage`) turns recording on, runs the ordinary reading and maps the claimed
//! byte ranges back to the records of the stream.
//!
//! Claims are kept by address (the readers see sub-slices of the stream), so a claim on bytes
//! outside the stream being dumped (copies, other blobs) is simply never matched to a record.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// What a claimed byte range is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Consumed by a field the reader knows.
    Decoded,
    /// Bytes a reader knowingly steps over (padding, a repeat it does not need).
    Skipped,
}

/// Most claims kept per recording (distinct ranges); beyond it claims are dropped.
const MAX_CLAIMS: usize = 20_000_000;

#[derive(Default)]
struct State {
    claims: HashMap<(usize, usize), (Kind, &'static str)>,
    /// Keys in insertion order since recording started (for [`rollback`]).
    journal: Vec<(usize, usize)>,
}

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn put(rec: &[u8], off: usize, len: usize, kind: Kind, name: &'static str) {
    if !ON.with(Cell::get) || len == 0 || off >= rec.len() {
        return;
    }
    let len = len.min(rec.len() - off);
    let addr = (rec.as_ptr() as usize).saturating_add(off);
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut()
            && s.claims.len() < MAX_CLAIMS
        {
            let e = s.claims.entry((addr, len));
            if let std::collections::hash_map::Entry::Vacant(v) = e {
                v.insert((kind, name));
                s.journal.push((addr, len));
            } else if let std::collections::hash_map::Entry::Occupied(mut o) = e {
                let (k0, n0) = *o.get();
                let upgrade = kind == Kind::Decoded && k0 == Kind::Skipped;
                if upgrade || (kind == k0 && generic(n0) && !generic(name)) {
                    o.insert((kind, name));
                }
            }
        }
    });
}

/// Names of claims that say only what a byte range is (a reference, a string, a list count,
/// a section head): a reader's own name for the same bytes wins over them.
pub fn generic(name: &str) -> bool {
    matches!(name, "ref" | "wstr" | "own-id" | "list.count" | "list.pad" | "section.head" | "section.zero")
}

/// `rec[off..off + len]` is a field the reader knows, called `name`.
pub(crate) fn field(rec: &[u8], off: usize, len: usize, name: &'static str) {
    put(rec, off, len, Kind::Decoded, name);
}

/// `rec[off..off + len]` is stepped over knowingly (padding, a repeat), called `name`.
pub(crate) fn skip(rec: &[u8], off: usize, len: usize, name: &'static str) {
    put(rec, off, len, Kind::Skipped, name);
}

/// Whether a recording is on (to avoid work that only feeds claims).
pub(crate) fn on() -> bool {
    ON.with(Cell::get)
}

/// A point to [`rollback`] to: claims made by a parse that then fails are taken back.
pub(crate) fn mark() -> usize {
    STATE.with(|s| s.borrow().as_ref().map_or(0, |s| s.journal.len()))
}

/// Takes back every claim made since `m` (see [`mark`]).
pub(crate) fn rollback(m: usize) {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            while s.journal.len() > m {
                if let Some(k) = s.journal.pop() {
                    s.claims.remove(&k);
                }
            }
        }
    });
}

/// One claimed range, relative to the slice it was recorded against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub start: usize,
    pub len: usize,
    pub kind: Kind,
    pub name: &'static str,
}

/// Recording on this thread while it lives; [`Recording::finish`] gives the claims on a slice.
#[derive(Debug)]
pub(crate) struct Recording(());

impl Recording {
    pub(crate) fn start() -> Recording {
        STATE.with(|s| *s.borrow_mut() = Some(State::default()));
        ON.with(|o| o.set(true));
        Recording(())
    }

    /// Stops recording and returns the claims that fall inside `within`, sorted by start.
    pub(crate) fn finish(self, within: &[u8]) -> Vec<Claim> {
        ON.with(|o| o.set(false));
        let state = STATE.with(|s| s.borrow_mut().take()).unwrap_or_default();
        let base = within.as_ptr() as usize;
        let end = base.saturating_add(within.len());
        let mut out: Vec<Claim> = state
            .claims
            .into_iter()
            .filter(|((a, l), _)| *a >= base && a.saturating_add(*l) <= end)
            .map(|((a, l), (kind, name))| Claim { start: a - base, len: l, kind, name })
            .collect();
        out.sort_by(|x, y| (x.start, x.kind, x.len, x.name).cmp(&(y.start, y.kind, y.len, y.name)));
        out
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        ON.with(|o| o.set(false));
        STATE.with(|s| *s.borrow_mut() = None);
    }
}
