//! Parts of Open CASCADE Technology (OCCT 8.0.1) translated to Rust (ADR 0025 pilot).
//!
//! Each module follows one OCCT package closely, so it can be checked against the C++
//! original: `tests/` compares results bit for bit with reference vectors printed by the
//! programs in `oracle/`, which call the real OCCT.
//!
//! Copyright (c) 1999-2025 OPEN CASCADE SAS (the original C++ code); this translation is
//! distributed under the same terms: GNU LGPL 2.1 with the OCCT exception.

pub mod bsplclib;
pub mod bsplslib;
pub mod elclib;
pub mod elslib;
pub mod gp;
pub mod math1d;
pub mod mathglob;
pub mod mathlin;
pub mod mathnd;
pub mod plib;
