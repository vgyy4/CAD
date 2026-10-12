//! Numerical constraint solver: graph decomposition, Levenberg–Marquardt with
//! minimum-movement damping, and rank-based degree-of-freedom / over-constraint
//! analysis. See the crate README for the algorithm and its tolerances.

use std::fmt;

use crate::dual::N;
use crate::equations::{self, Equation, Layout, Owner};
use crate::linalg::{RowEchelon, cholesky_solve, rcm};
use crate::sketch::{ConstraintId, EntityId, Geom, MAX_COORD, Sketch, SketchError};

/// Upper bound for [`SolveOptions::max_iterations`].
pub const HARD_MAX_ITERATIONS: u32 = 10_000;
/// Upper bound for [`SolveOptions::max_component_params`]. The dense algebra uses
/// `O(n²)` memory and `O(n³)` time per connected component of `n` unknowns.
pub const HARD_MAX_COMPONENT_PARAMS: usize = 4_000;
/// Damping weight of dragged points relative to all other unknowns.
const DRAG_WEIGHT: f64 = 1.0e4;
/// Most damping increases tried within one Levenberg–Marquardt iteration.
const MAX_TRIES: u32 = 60;
/// An unknown is "free" when the squared norm of its unit vector's projection onto
/// the Jacobian's null space exceeds this.
const FREE_TOL: f64 = 1e-9;
/// At an unsatisfied stopping point, `‖Jᵀr‖∞ ≤ STATIONARY_TOL · ‖J‖ · ‖r‖` means a
/// least-squares minimum was reached, i.e. the constraints conflict.
const STATIONARY_TOL: f64 = 1e-6;
/// At a least-squares optimum, a constraint is reported as conflicting when its
/// residual exceeds the tolerance and this fraction of the component's largest
/// residual (smaller residuals are leftovers of the damped iteration).
const CONFLICT_REL: f64 = 1e-6;
/// Extra Newton steps taken after the tolerance is met.
const POLISH_STEPS: u32 = 2;

/// Solver settings. Out-of-range values are clamped (see [`SolveOptions::sanitized`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolveOptions {
    /// Converged when every residual is at most this (mm for lengths, rad for
    /// angles). Default `1e-10`, clamped to `[1e-14, 1e-3]`.
    pub tolerance: f64,
    /// Levenberg–Marquardt iterations per connected component. Default 100.
    pub max_iterations: u32,
    /// Relative tolerance of the rank decision: a Jacobian row is dependent on the
    /// previous ones when its orthogonal remainder is at most this fraction of its
    /// norm. Default `1e-8`, clamped to `[1e-14, 1e-2]`.
    pub rank_tolerance: f64,
    /// Largest connected component (in unknowns) the solver accepts. Default 1000.
    pub max_component_params: usize,
}

impl Default for SolveOptions {
    fn default() -> Self {
        SolveOptions { tolerance: 1e-10, max_iterations: 100, rank_tolerance: 1e-8, max_component_params: 1000 }
    }
}

impl SolveOptions {
    /// The options with every field clamped to its allowed range; NaN falls back to
    /// the default.
    pub fn sanitized(&self) -> SolveOptions {
        let d = SolveOptions::default();
        let clamp = |v: f64, lo: f64, hi: f64, def: f64| if v.is_nan() { def } else { v.clamp(lo, hi) };
        SolveOptions {
            tolerance: clamp(self.tolerance, 1e-14, 1e-3, d.tolerance),
            max_iterations: self.max_iterations.min(HARD_MAX_ITERATIONS),
            rank_tolerance: clamp(self.rank_tolerance, 1e-14, 1e-2, d.rank_tolerance),
            max_component_params: self.max_component_params.min(HARD_MAX_COMPONENT_PARAMS),
        }
    }
}

/// Overall outcome of a solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveStatus {
    /// All constraints satisfied, no degrees of freedom left, nothing redundant.
    WellConstrained,
    /// All constraints satisfied, some degrees of freedom left.
    UnderConstrained,
    /// Some constraints are redundant or conflicting (see the result's lists).
    OverConstrained,
    /// Invalid input, a limit was exceeded, or the solver did not converge.
    Failed,
}

/// Why a solve failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveError {
    /// The sketch is malformed.
    Invalid(SketchError),
    /// A connected component has more unknowns than allowed.
    ComponentTooLarge { params: usize, limit: usize },
    /// A drag target refers to a non-point entity or a non-finite position.
    BadDrag(EntityId),
    /// The equations could not be evaluated to finite numbers.
    NonFinite,
    /// The iteration limit was reached before convergence, without a detectable
    /// conflict.
    NotConverged,
    /// A circle ended up with a non-positive radius.
    NonPositiveRadius(EntityId),
}

impl fmt::Display for SolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SolveError::Invalid(e) => write!(f, "invalid sketch: {e}"),
            SolveError::ComponentTooLarge { params, limit } => {
                write!(f, "a connected group of geometry has {params} unknowns (limit {limit})")
            }
            SolveError::BadDrag(e) => write!(f, "cannot drag entity {e}: must be a point and a finite target"),
            SolveError::NonFinite => write!(f, "constraint equations are not finite"),
            SolveError::NotConverged => write!(f, "the solver did not converge"),
            SolveError::NonPositiveRadius(e) => write!(f, "circle {e} would get a non-positive radius"),
        }
    }
}

impl std::error::Error for SolveError {}

/// A point pulled toward a target during an interactive drag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub point: EntityId,
    pub target: [f64; 2],
}

/// Result of [`solve`].
#[derive(Debug, Clone, PartialEq)]
pub struct SolveResult {
    pub status: SolveStatus,
    /// Remaining degrees of freedom (sum over components of unknowns − Jacobian rank).
    pub dof: u32,
    /// The sketch with solved point positions and circle radii. On invalid input it
    /// is the input unchanged; when constraints conflict it holds the least-squares
    /// compromise.
    pub sketch: Sketch,
    /// Entities that can still move (a point with a free coordinate, a circle with a
    /// free centre or radius, a line or arc with a free defining point), by id.
    pub free_entities: Vec<EntityId>,
    /// Consistent constraints that are implied by earlier (lower-id) ones, by id.
    pub redundant: Vec<ConstraintId>,
    /// Constraints that are unsatisfied at the least-squares optimum, by id.
    pub conflicting: Vec<ConstraintId>,
    /// Largest number of iterations used by any component.
    pub iterations: u32,
    /// Largest absolute residual (mm or rad) after solving.
    pub max_residual: f64,
    /// Set when `status` is [`SolveStatus::Failed`].
    pub error: Option<SolveError>,
}

/// Solves the sketch, moving geometry as little as possible from its current
/// position.
pub fn solve(sketch: &Sketch, opts: &SolveOptions) -> SolveResult {
    solve_drag(sketch, opts, &[])
}

/// Solves with some points dragged toward targets. Dragged points start at their
/// target and are damped [`DRAG_WEIGHT`] times more strongly than other unknowns,
/// so the rest of the sketch moves to follow them where the constraints allow.
/// [`crate::Constraint::Fixed`] still refers to the positions stored in `sketch`.
pub fn solve_drag(sketch: &Sketch, opts: &SolveOptions, drags: &[Drag]) -> SolveResult {
    let o = opts.sanitized();
    let fail = |e: SolveError| SolveResult {
        status: SolveStatus::Failed,
        dof: 0,
        sketch: sketch.clone(),
        free_entities: Vec::new(),
        redundant: Vec::new(),
        conflicting: Vec::new(),
        iterations: 0,
        max_residual: f64::INFINITY,
        error: Some(e),
    };
    if let Err(e) = sketch.validate() {
        return fail(SolveError::Invalid(e));
    }
    let lay = Layout::new(sketch);
    let Some(eqs) = equations::build(sketch, &lay) else {
        return fail(SolveError::Invalid(SketchError::InvalidValue { what: "inconsistent sketch references" }));
    };
    let n = lay.x0.len();
    let mut is_radius = vec![false; n];
    for (e, first) in sketch.entities().iter().zip(&lay.first) {
        if let (Geom::Circle { .. }, Some(g)) = (e.geom, first)
            && let Some(r) = is_radius.get_mut(*g)
        {
            *r = true;
        }
    }
    let mut x = lay.x0.clone();
    let mut w = vec![1.0; n];
    for d in drags {
        let ok = d.target.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD);
        let first = sketch.entity_index(d.point).and_then(|i| lay.first.get(i).copied().flatten());
        let is_point = matches!(sketch.entity(d.point).map(|e| e.geom), Some(Geom::Point { .. }));
        let (Some(g), true, true) = (first, ok, is_point) else {
            return fail(SolveError::BadDrag(d.point));
        };
        for (k, t) in d.target.iter().enumerate() {
            if let Some(xi) = x.get_mut(g + k) {
                *xi = *t;
            }
            if let Some(wi) = w.get_mut(g + k) {
                *wi = DRAG_WEIGHT;
            }
        }
    }

    let comps = components(n, &eqs);
    let ncons = sketch.constraints().len();
    let mut col_map = vec![usize::MAX; n];
    let mut free_param = vec![false; n];
    let mut dependent = vec![false; ncons];
    let mut unsatisfied = vec![false; ncons];
    let mut conflict_comp = vec![false; ncons];
    let mut dof: u64 = 0;
    let mut iterations = 0u32;
    let mut max_residual: f64 = 0.0;
    let mut error = None;

    for comp in &comps {
        if comp.eqs.is_empty() {
            dof += comp.cols.len() as u64;
            for &c in &comp.cols {
                if let Some(f) = free_param.get_mut(c) {
                    *f = true;
                }
            }
            continue;
        }
        if comp.cols.len() > o.max_component_params {
            return fail(SolveError::ComponentTooLarge { params: comp.cols.len(), limit: o.max_component_params });
        }
        for (j, &c) in comp.cols.iter().enumerate() {
            if let Some(m) = col_map.get_mut(c) {
                *m = j;
            }
        }
        let lm = levenberg_marquardt(&eqs, comp, &col_map, &mut x, &w, &is_radius, &o);
        if !lm.finite {
            return fail(SolveError::NonFinite);
        }
        iterations = iterations.max(lm.iterations);

        // Rank analysis at the final point.
        let nc = comp.cols.len();
        // Banded column order for the echelon form (only speed depends on it: which
        // rows are dependent does not depend on the column order).
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nc];
        for &ei in &comp.eqs {
            let Some(eq) = eqs.get(ei) else { continue };
            let cs: Vec<usize> = eq.locals.iter().take(eq.nloc).filter_map(|g| col_map.get(*g).copied()).collect();
            for &a in &cs {
                for &b in &cs {
                    if a != b
                        && let Some(l) = adj.get_mut(a)
                        && !l.contains(&b)
                    {
                        l.push(b);
                    }
                }
            }
        }
        let order = rcm(&adj);
        let mut pos = vec![0usize; nc];
        for (new, &old) in order.iter().enumerate() {
            if let Some(p) = pos.get_mut(old) {
                *p = new;
            }
        }
        let mut basis = RowEchelon::new(nc, o.rank_tolerance);
        let mut row = vec![0.0; nc];
        let mut prow = vec![0.0; nc];
        let mut res = Vec::with_capacity(comp.eqs.len());
        for &ei in &comp.eqs {
            let Some(eq) = eqs.get(ei) else { continue };
            let r = sparse_row(eq, &col_map, &x, &mut row);
            for (j, v) in row.iter().enumerate() {
                if let Some(slot) = pos.get(j).and_then(|&p| prow.get_mut(p)) {
                    *slot = *v;
                }
            }
            let independent = basis.add(&prow);
            if let Owner::Constraint(ci) = eq.owner
                && !independent
                && let Some(d) = dependent.get_mut(ci)
            {
                *d = true;
            }
            res.push((eq.owner, r.abs()));
        }
        let comp_max = res.iter().fold(0.0f64, |m, r| m.max(r.1));
        max_residual = max_residual.max(comp_max);
        let threshold = o.tolerance.max(CONFLICT_REL * comp_max);
        for (owner, r) in res {
            if let Owner::Constraint(ci) = owner
                && r > threshold
                && let Some(u) = unsatisfied.get_mut(ci)
            {
                *u = true;
            }
        }
        dof += (nc - basis.rank()) as u64;
        let free = basis.free_columns(FREE_TOL);
        for (j, &c) in comp.cols.iter().enumerate() {
            if pos.get(j).and_then(|&p| free.get(p)).copied().unwrap_or(false)
                && let Some(f) = free_param.get_mut(c)
            {
                *f = true;
            }
        }
        if !lm.converged {
            if lm.stationary {
                for &ei in &comp.eqs {
                    if let Some(Owner::Constraint(ci)) = eqs.get(ei).map(|e| e.owner)
                        && let Some(cc) = conflict_comp.get_mut(ci)
                    {
                        *cc = true;
                    }
                }
            } else if error.is_none() {
                error = Some(SolveError::NotConverged);
            }
        }
    }

    // Write the solution back.
    let mut out = sketch.clone();
    let ids: Vec<EntityId> = sketch.entities().iter().map(|e| e.id).collect();
    for (i, e) in out.entities_mut().iter_mut().enumerate() {
        let Some(g) = lay.first.get(i).copied().flatten() else { continue };
        let xv = |k: usize| x.get(g + k).copied().unwrap_or(0.0);
        match &mut e.geom {
            Geom::Point { p } => *p = [xv(0), xv(1)],
            Geom::Circle { radius, .. } => {
                *radius = xv(0);
                if crate::geom2::not_gt(*radius, 0.0) && error.is_none() {
                    error = Some(SolveError::NonPositiveRadius(e.id));
                }
            }
            Geom::Line { .. } | Geom::Arc { .. } | Geom::Spline { .. } => {}
        }
    }
    let free_entities = free_entities(sketch, &lay, &free_param, &ids);

    let cids = |pred: &dyn Fn(usize) -> bool| -> Vec<ConstraintId> {
        sketch.constraints().iter().enumerate().filter(|(i, _)| pred(*i)).map(|(_, (id, _))| *id).collect()
    };
    let flag = |v: &Vec<bool>, i: usize| v.get(i).copied().unwrap_or(false);
    let conflicting = cids(&|i| flag(&conflict_comp, i) && flag(&unsatisfied, i));
    let redundant = cids(&|i| flag(&dependent, i) && !(flag(&conflict_comp, i) && flag(&unsatisfied, i)));
    let any_conflict_comp = conflict_comp.iter().any(|&b| b);
    if any_conflict_comp && conflicting.is_empty() && error.is_none() {
        // Stationary but no user constraint is unsatisfied: only internal arc
        // equations are violated, which cannot be explained to the user.
        error = Some(SolveError::NotConverged);
    }
    let status = if error.is_some() {
        SolveStatus::Failed
    } else if !conflicting.is_empty() || !redundant.is_empty() {
        SolveStatus::OverConstrained
    } else if dof > 0 {
        SolveStatus::UnderConstrained
    } else {
        SolveStatus::WellConstrained
    };
    SolveResult {
        status,
        dof: u32::try_from(dof).unwrap_or(u32::MAX),
        sketch: out,
        free_entities,
        redundant,
        conflicting,
        iterations,
        max_residual,
        error,
    }
}

fn free_entities(sketch: &Sketch, lay: &Layout, free_param: &[bool], ids: &[EntityId]) -> Vec<EntityId> {
    let param_free = |g: usize| free_param.get(g).copied().unwrap_or(false);
    let entity_free = |id: EntityId| -> bool {
        let Some(i) = sketch.entity_index(id) else { return false };
        match lay.first.get(i).copied().flatten() {
            Some(g) => match sketch.entity(id).map(|e| e.geom) {
                Some(Geom::Point { .. }) => param_free(g) || param_free(g + 1),
                _ => param_free(g),
            },
            None => false,
        }
    };
    let mut out = Vec::new();
    for &id in ids {
        let free = entity_free(id) || sketch.referenced_points(id).into_iter().any(entity_free);
        if free {
            out.push(id);
        }
    }
    out
}

/// One connected component of the unknown/equation graph.
#[derive(Debug, Clone, Default)]
struct Component {
    /// Global unknown indices, ascending.
    cols: Vec<usize>,
    /// Equation indices, ascending.
    eqs: Vec<usize>,
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    // Path halving; bounded by the number of nodes.
    for _ in 0..parent.len() {
        let p = parent.get(i).copied().unwrap_or(i);
        if p == i {
            return i;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = gp;
        }
        i = gp;
    }
    i
}

/// Splits the unknowns into independent groups. Components are ordered by their
/// smallest unknown index, so the order is deterministic.
fn components(n: usize, eqs: &[Equation]) -> Vec<Component> {
    let mut parent: Vec<usize> = (0..n).collect();
    for eq in eqs {
        let Some(&first) = eq.locals.first() else { continue };
        for &g in eq.locals.iter().take(eq.nloc).skip(1) {
            let (ra, rb) = (find(&mut parent, first), find(&mut parent, g));
            if ra != rb {
                // Attach the larger root to the smaller one (deterministic).
                let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
                if let Some(slot) = parent.get_mut(hi) {
                    *slot = lo;
                }
            }
        }
    }
    let mut comp_of_root = vec![usize::MAX; n];
    let mut comps: Vec<Component> = Vec::new();
    for j in 0..n {
        let r = find(&mut parent, j);
        let Some(slot) = comp_of_root.get_mut(r) else { continue };
        if *slot == usize::MAX {
            *slot = comps.len();
            comps.push(Component::default());
        }
        if let Some(c) = comps.get_mut(*slot) {
            c.cols.push(j);
        }
    }
    for (i, eq) in eqs.iter().enumerate() {
        if eq.nloc == 0 {
            continue;
        }
        let Some(&first) = eq.locals.first() else { continue };
        let r = find(&mut parent, first);
        if let Some(c) = comp_of_root.get(r).and_then(|&ci| comps.get_mut(ci)) {
            c.eqs.push(i);
        }
    }
    comps
}

/// Evaluates one equation into a dense local row (`row` is overwritten) and returns
/// its residual.
fn sparse_row(eq: &Equation, col_map: &[usize], x: &[f64], row: &mut [f64]) -> f64 {
    row.iter_mut().for_each(|v| *v = 0.0);
    let d = equations::eval(eq, x);
    for (k, &g) in eq.locals.iter().enumerate().take(eq.nloc) {
        let j = col_map.get(g).copied().unwrap_or(usize::MAX);
        if let (Some(slot), Some(dk)) = (row.get_mut(j), d.d.get(k)) {
            *slot += dk;
        }
    }
    d.v
}

/// Residuals of a component: `(½‖r‖², max |rᵢ|)`.
fn cost(eqs: &[Equation], comp: &Component, x: &[f64]) -> (f64, f64) {
    let mut c = 0.0;
    let mut m: f64 = 0.0;
    for &ei in &comp.eqs {
        if let Some(eq) = eqs.get(ei) {
            let r = equations::eval(eq, x).v;
            c += 0.5 * r * r;
            m = if r.is_nan() { f64::NAN } else { m.max(r.abs()) };
        }
    }
    (c, m)
}

/// Normal equations `JᵀJ` (dense, row major) and gradient `Jᵀr`.
fn normal_eqs(eqs: &[Equation], comp: &Component, col_map: &[usize], x: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let nc = comp.cols.len();
    let mut a = vec![0.0; nc * nc];
    let mut g = vec![0.0; nc];
    for &ei in &comp.eqs {
        let Some(eq) = eqs.get(ei) else { continue };
        let d = equations::eval(eq, x);
        // Merge aliased locals into (column, derivative) pairs.
        let mut ent: [(usize, f64); N] = [(usize::MAX, 0.0); N];
        let mut ne = 0;
        for (k, &gl) in eq.locals.iter().enumerate().take(eq.nloc) {
            let j = col_map.get(gl).copied().unwrap_or(usize::MAX);
            let dk = d.d.get(k).copied().unwrap_or(0.0);
            if let Some(e) = ent.iter_mut().take(ne).find(|e| e.0 == j) {
                e.1 += dk;
            } else if let Some(e) = ent.get_mut(ne) {
                *e = (j, dk);
                ne += 1;
            }
        }
        let ent = ent.get(..ne).unwrap_or(&[]);
        for &(j1, v1) in ent {
            if let Some(gj) = g.get_mut(j1) {
                *gj += v1 * d.v;
            }
            for &(j2, v2) in ent {
                if let Some(aij) = a.get_mut(j1.saturating_mul(nc).saturating_add(j2)) {
                    *aij += v1 * v2;
                }
            }
        }
    }
    (a, g)
}

#[derive(Debug, Clone, Copy)]
struct LmOutcome {
    iterations: u32,
    converged: bool,
    /// Stopped at a least-squares stationary point with non-zero residual.
    stationary: bool,
    finite: bool,
}

fn levenberg_marquardt(
    eqs: &[Equation],
    comp: &Component,
    col_map: &[usize],
    x: &mut [f64],
    w_all: &[f64],
    is_radius: &[bool],
    o: &SolveOptions,
) -> LmOutcome {
    let nc = comp.cols.len();
    let w: Vec<f64> = comp.cols.iter().map(|&c| w_all.get(c).copied().unwrap_or(1.0)).collect();
    let radii: Vec<usize> = comp.cols.iter().copied().filter(|&c| is_radius.get(c).copied().unwrap_or(false)).collect();
    let (mut c, mut maxres) = cost(eqs, comp, x);
    if !c.is_finite() || !maxres.is_finite() {
        return LmOutcome { iterations: 0, converged: false, stationary: false, finite: false };
    }
    let (mut a, mut g) = normal_eqs(eqs, comp, col_map, x);
    let maxdiag = (0..nc).map(|j| a.get(j * nc + j).copied().unwrap_or(0.0)).fold(0.0, f64::max);
    let lam_min = 1e-12 * maxdiag.max(1.0);
    let mut lambda = (1e-3 * maxdiag).max(lam_min);
    let mut nu = 2.0;
    let mut old = vec![0.0; nc];
    let mut iterations = 0;
    let mut stuck = false;
    let mut polish = 0;
    while iterations < o.max_iterations {
        if maxres <= o.tolerance {
            // A couple of extra Newton steps after convergence take the residual to
            // round-off level at negligible cost (convergence is quadratic there).
            if polish >= POLISH_STEPS || maxres <= 0.0 {
                break;
            }
            polish += 1;
        }
        iterations += 1;
        let mut accepted = false;
        let mut small_step = false;
        for _ in 0..MAX_TRIES {
            let mut m = a.clone();
            for (j, wj) in w.iter().enumerate() {
                if let Some(v) = m.get_mut(j * nc + j) {
                    *v += lambda * wj;
                }
            }
            let mut delta: Vec<f64> = g.iter().map(|v| -v).collect();
            if !cholesky_solve(&mut m, nc, &mut delta) {
                lambda = (lambda * nu).min(1e300);
                nu = (nu * 2.0).min(1e30);
                continue;
            }
            for ((&col, od), dj) in comp.cols.iter().zip(old.iter_mut()).zip(delta.iter()) {
                if let Some(xv) = x.get_mut(col) {
                    *od = *xv;
                    *xv += dj;
                }
            }
            let (c_new, m_new) = cost(eqs, comp, x);
            let mut pred = 0.0;
            for ((dj, gj), wj) in delta.iter().zip(g.iter()).zip(w.iter()) {
                pred += 0.5 * dj * (lambda * wj * dj - gj);
            }
            let actual = c - c_new;
            // Circles must keep a positive radius: otherwise the solver can slide
            // into the mirror-image solution family with negative radii.
            let radii_ok = radii.iter().all(|&rc| x.get(rc).is_some_and(|&r| r > 0.0));
            if radii_ok && c_new.is_finite() && m_new.is_finite() && actual > 0.0 && pred > 0.0 {
                let rho = actual / pred;
                let t = 2.0 * rho - 1.0;
                lambda = (lambda * (1.0 / 3.0f64).max(1.0 - t * t * t)).max(lam_min);
                nu = 2.0;
                let xmax = comp.cols.iter().map(|&col| x.get(col).copied().unwrap_or(0.0).abs()).fold(0.0, f64::max);
                let dmax = delta.iter().fold(0.0f64, |acc, v| acc.max(v.abs()));
                small_step = dmax <= 1e-15 * (1.0 + xmax);
                c = c_new;
                maxres = m_new;
                accepted = true;
                break;
            }
            for (&col, od) in comp.cols.iter().zip(old.iter()) {
                if let Some(xv) = x.get_mut(col) {
                    *xv = *od;
                }
            }
            lambda = (lambda * nu).min(1e300);
            nu = (nu * 2.0).min(1e30);
        }
        if !accepted {
            stuck = maxres > o.tolerance;
            break;
        }
        (a, g) = normal_eqs(eqs, comp, col_map, x);
        if small_step && maxres > o.tolerance {
            stuck = true;
            break;
        }
    }
    let converged = maxres <= o.tolerance;
    let mut stationary = false;
    if !converged {
        // ‖Jᵀr‖∞ relative to ‖J‖_F · ‖r‖₂.
        let jnorm = (0..nc).map(|j| a.get(j * nc + j).copied().unwrap_or(0.0)).sum::<f64>().sqrt();
        let rnorm = (2.0 * c).sqrt();
        let gmax = g.iter().fold(0.0f64, |acc, v| acc.max(v.abs()));
        stationary = stuck || gmax <= STATIONARY_TOL * jnorm * rnorm;
    }
    LmOutcome { iterations, converged, stationary, finite: c.is_finite() }
}
