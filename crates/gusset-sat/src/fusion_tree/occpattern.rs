//! Rectangular patterns of components (`R-Pattern` of occurrences): the pattern makes new
//! occurrences of one component, and the file stores each copy's occurrence with its own
//! placement (round 12 notes, section 20 of `docs/formats/fusion-design-stream.md`).
//!
//! The pattern record names the original occurrence (`2D6E13A1`) and then each copy; an
//! occurrence's first `CE2913AA` record names its component (`E03784ED`) and, for a copy, holds
//! the copy's 4x4 placement in the parent component (row-major, centimetres). The original's
//! record holds no matrix: it sits at the parent's origin. The copies are read only when they
//! are exactly the lattice the pattern's parameters describe (`countU` x `CountV` occurrences,
//! the first direction's `uSpaceDistance` steps, steps of `|vSpaceDistance|` in one other
//! direction, no rotation), so a wrong reading cannot place a body.

use super::bodyops::matrix;
use super::geom::mm;
use super::stream::{Design, param, ref_ids};
use super::{PARAM, component_bodies, feature};
use crate::fusion::Placement;

/// Most copies one pattern may make.
const MAX_COPIES: usize = 10_000;

/// The occurrences one pattern of components made.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternCopies {
    /// The pattern feature's record id.
    pub source_id: u64,
    /// The component the pattern repeats (its record id).
    pub component: u64,
    /// The component's body blobs (`BREP.<id>.smb`).
    pub blobs: Vec<String>,
    /// Each copy's placement relative to the original occurrence (millimetres).
    pub copies: Vec<Placement>,
}

/// The value of each named parameter of a feature (both holder kinds), centimetres or counts as
/// stored.
fn values(d: &Design, fid: u64, name: &str) -> Vec<f64> {
    let mut out = Vec::new();
    for q in ref_ids(d, d.rec(fid)) {
        if !(d.is(q, "D91D429C") || d.is(q, "A085449A")) {
            continue;
        }
        for q2 in ref_ids(d, d.rec(q)) {
            if d.is(q2, PARAM)
                && let Some(p) = param(d, q2)
                && p.name.as_deref() == Some(name)
                && let Some(v) = p.value
                && !out.contains(&v)
            {
                out.push(v);
            }
        }
    }
    out
}

/// The single value of a parameter, if the feature has exactly one.
pub(super) fn value(d: &Design, fid: u64, name: &str) -> Option<f64> {
    match values(d, fid, name).as_slice() {
        [v] => Some(*v),
        _ => None,
    }
}

/// The first reference of a class in a record.
fn first_ref(d: &Design, q: u64, class: &str) -> Option<u64> {
    ref_ids(d, d.rec(q)).into_iter().find(|r| d.is(*r, class))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * (1.0 + a.abs().max(b.abs()))
}

fn same3(a: &[f64; 3], b: &[f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| close(*x, *y))
}

/// `i a + j b`.
fn step(a: [f64; 3], i: usize, b: [f64; 3], j: usize) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    let (i, j) = (i as f64, j as f64);
    [ax * i + bx * j, ay * i + by * j, az * i + bz * j]
}

/// The copies of one pattern feature `fid`, when it is a rectangular pattern of the occurrences
/// of one component whose stored copies are exactly its parameters' lattice.
pub(super) fn copies_of(d: &Design, fid: u64) -> Result<PatternCopies, String> {
    if !(d.is(fid, "AE12AD3F") || d.is(fid, "E8816367")) {
        return Err("not a rectangular pattern".into());
    }
    let mut occs: Vec<u64> = Vec::new();
    for q in ref_ids(d, d.rec(fid)) {
        if d.is(q, "2D6E13A1") && !occs.contains(&q) {
            occs.push(q);
        }
    }
    let [original, copies @ ..] = occs.as_slice() else { return Err("the pattern names no occurrence".into()) };
    if copies.is_empty() || copies.len() > MAX_COPIES {
        return Err("the pattern names no copy of an occurrence".into());
    }
    // Each occurrence's component and placement in the parent component.
    let read = |q: u64| -> Result<(u64, Option<Placement>), String> {
        let ce = first_ref(d, q, "CE2913AA").ok_or("an occurrence has no placement record")?;
        let comp = first_ref(d, ce, "E03784ED").ok_or("an occurrence names no component")?;
        let m = matrix(d.rec(ce)).map(|(r, t)| Placement { r, t: [mm(t[0]), mm(t[1]), mm(t[2])] });
        Ok((comp, m))
    };
    let (component, at) = read(*original)?;
    if at.is_some() {
        return Err("the original occurrence is moved in its component".into());
    }
    let mut placed = Vec::with_capacity(copies.len());
    for q in copies {
        let (c, m) = read(*q)?;
        if c != component {
            return Err("the pattern repeats occurrences of several components".into());
        }
        let m = m.ok_or("a copy of an occurrence has no placement")?;
        let identity = Placement::IDENTITY;
        if m.r.iter().flatten().zip(identity.r.iter().flatten()).any(|(a, b)| !close(*a, *b)) {
            return Err("a copy of an occurrence is turned".into());
        }
        placed.push(m);
    }
    // The lattice the parameters describe: countU x CountV positions, i steps of the first
    // direction times uSpaceDistance and j steps of |vSpaceDistance| in one other direction.
    let nu = value(d, fid, "countU").ok_or("the pattern has no single countU")?;
    let nv = value(d, fid, "CountV").unwrap_or(1.0);
    let su = mm(value(d, fid, "uSpaceDistance").ok_or("the pattern has no single uSpaceDistance")?);
    let sv = value(d, fid, "vSpaceDistance").map_or(0.0, mm);
    let dirs = feature::pattern_dirs(d, fid);
    let [(_, du)] = dirs.as_slice() else { return Err("the pattern has not one direction record".into()) };
    let count = |n: f64| (n.fract() == 0.0 && (1.0..=MAX_COPIES as f64).contains(&n)).then_some(n as usize);
    let (Some(nu), Some(nv)) = (count(nu), count(nv)) else { return Err("the pattern's counts are not whole".into()) };
    if nu * nv != occs.len() {
        return Err("the pattern's counts do not match its occurrences".into());
    }
    let a = [du[0] * su, du[1] * su, du[2] * su];
    // The second step: the copy that is not on the first direction's line, nearest the origin.
    let on_u = |t: &[f64; 3]| (0..nu).any(|i| same3(t, &step(a, i, [0.0; 3], 0)));
    let b = placed.iter().map(|p| p.t).filter(|t| !on_u(t)).min_by(|x, y| {
        let n = |t: &[f64; 3]| t.iter().map(|c| c * c).sum::<f64>();
        n(x).total_cmp(&n(y))
    });
    let b = match (nv, b) {
        (1, None) => [0.0; 3],
        (_, Some(b)) if nv > 1 && close(super::geom::norm(b), sv.abs()) => b,
        _ => return Err("the copies are not the pattern's second direction".into()),
    };
    let mut want: Vec<[f64; 3]> = Vec::new();
    for j in 0..nv {
        for i in 0..nu {
            if i + j > 0 {
                want.push(step(a, i, b, j));
            }
        }
    }
    let mut left: Vec<[f64; 3]> = placed.iter().map(|p| p.t).collect();
    for w in &want {
        let Some(k) = left.iter().position(|t| same3(t, w)) else {
            return Err("the copies are not where the pattern's parameters put them".into());
        };
        left.swap_remove(k);
    }
    if !left.is_empty() {
        return Err("the pattern has more copies than its parameters make".into());
    }
    let blobs = component_bodies(d, component);
    if blobs.is_empty() {
        return Err("the repeated component has no bodies".into());
    }
    Ok(PatternCopies { source_id: fid, component, blobs, copies: placed })
}
