//! 2D sketches for Gusset: geometry and a geometric constraint solver with
//! degree-of-freedom analysis.
//!
//! ```
//! use gusset_sketch::{Constraint, Sketch, SolveOptions, SolveStatus, solve};
//! let mut s = Sketch::new();
//! let l = s.add_line([0.0, 0.0], [9.0, 1.0]).unwrap();
//! let (a, _) = s.line_points(l).unwrap();
//! s.add_constraint(Constraint::Fixed(a)).unwrap();
//! s.add_constraint(Constraint::Horizontal(l)).unwrap();
//! s.add_constraint(Constraint::Distance { a: l, b: l, value: 10.0 }).unwrap();
//! let r = solve(&s, &SolveOptions::default());
//! assert_eq!(r.status, SolveStatus::WellConstrained);
//! let (_, b) = r.sketch.line_points(l).unwrap();
//! let p = r.sketch.point(b).unwrap();
//! assert!((p[0] - 10.0).abs() < 1e-9 && p[1].abs() < 1e-9);
//! ```

#![cfg_attr(test, allow(clippy::float_cmp))]

mod dual;
mod equations;
mod geom2;
mod linalg;
pub mod sketch;
pub mod solver;

pub use sketch::{Constraint, ConstraintId, Entity, EntityId, EntityKind, Geom, Sketch, SketchError};
pub use solver::{Drag, SolveError, SolveOptions, SolveResult, SolveStatus, solve, solve_drag};
