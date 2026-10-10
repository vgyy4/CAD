//! Units, unit-aware expressions and named parameters for Gusset.
//!
//! ```
//! use gusset_units::{expr, Dim};
//! let q = expr::eval_str("10 mm + 0.5 in", &()).unwrap();
//! assert_eq!(q.dim, Dim::LENGTH);
//! assert!((q.value - 22.7).abs() < 1e-12);
//! ```

#![cfg_attr(test, allow(clippy::float_cmp))]

pub mod expr;
pub mod params;
pub mod quantity;

pub use quantity::{Dim, LengthUnit, Quantity, Unit};
