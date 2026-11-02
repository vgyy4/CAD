//! Parts of Open CASCADE Technology (OCCT 8.0.1) translated to Rust (ADR 0025 pilot).
//!
//! Each module follows one OCCT package closely, so it can be checked against the C++
//! original: `tests/` compares results bit for bit with reference vectors printed by the
//! programs in `oracle/`, which call the real OCCT.
//!
//! Copyright (c) 1999-2025 OPEN CASCADE SAS (the original C++ code); this translation is
//! distributed under the same terms: GNU LGPL 2.1 with the OCCT exception.

pub mod adaptor2d;
pub mod adaptor3d;
pub mod advapp2var;
pub mod advapprox;
pub mod appparcurves;
pub mod bnd;
pub mod bsplclib;
pub mod bsplclib_cache;
pub mod bsplslib;
pub mod bvh;
pub mod convert;
pub mod cslib;
pub mod elclib;
pub mod elslib;
pub mod geom;
pub mod geom2d;
pub mod geom2dadaptor;
pub mod geom2deval;
pub mod geom2dgrideval;
pub mod geom2dhash;
pub mod geomadaptor;
pub mod gp;
pub mod gprop;
pub mod math1d;
pub mod mathglob;
pub mod mathinteg;
pub mod mathlin;
pub mod mathlin_pkg;
pub mod mathnd;
pub mod mathopt;
pub mod mathpoly;
pub mod mathroot;
pub mod mathsys;
pub mod mathutils;
pub mod plib;
pub mod poly;
pub mod topabs;
pub mod toploc;
