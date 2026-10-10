//! 2D sketches for Gusset: geometry and constraints.
//!
//! ```
//! use gusset_sketch::{Constraint, Sketch};
//! let mut s = Sketch::new();
//! let l = s.add_line([0.0, 0.0], [9.0, 1.0]).unwrap();
//! let (a, _) = s.line_points(l).unwrap();
//! s.add_constraint(Constraint::Fixed(a)).unwrap();
//! s.add_constraint(Constraint::Horizontal(l)).unwrap();
//! s.add_constraint(Constraint::Distance { a: l, b: l, value: 10.0 }).unwrap();
//! assert!(s.validate().is_ok());
//! ```

#![cfg_attr(test, allow(clippy::float_cmp))]

mod geom2;
pub mod sketch;

pub use sketch::{Constraint, ConstraintId, Entity, EntityId, EntityKind, Geom, Sketch, SketchError};
