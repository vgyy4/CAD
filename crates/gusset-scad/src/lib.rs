//! OpenSCAD reader (ADR 0033): a `.scad` file is lexed, parsed and evaluated into a CSG
//! tree of exact primitives (cubes, spheres, cylinders, cones, extruded and revolved 2D
//! shapes) with their placement, joined by unions, differences and intersections.
//! The geometry kernel then builds exact B-rep from the tree.
//!
//! Everything here is written from the OpenSCAD User Manual; none of OpenSCAD's (GPL)
//! sources is used. The file is untrusted: lexing, parsing and evaluation are bounded
//! (nesting depth, evaluation steps, primitive count, vector sizes).

mod eval;
mod parse;

pub use eval::Limits;
use std::sync::atomic::AtomicBool;

/// A 4x4 affine transform, row-major; the last row is `[0, 0, 0, 1]`.
pub type M4 = [[f64; 4]; 4];

pub const IDENTITY: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// `a * b`.
pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut out = [[0.0; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4)
                .map(|k| {
                    a.get(i).and_then(|r| r.get(k)).copied().unwrap_or(0.0)
                        * b.get(k).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
                })
                .sum();
        }
    }
    out
}

/// A 2D shape with its 2D placement already applied.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape2 {
    Circle {
        center: [f64; 2],
        radius: f64,
    },
    /// Closed outlines; the first is the outside, others are holes (OpenSCAD's `paths`).
    Polygon(Vec<Vec<[f64; 2]>>),
}

/// A 2D CSG tree (inside `linear_extrude` / `rotate_extrude`).
#[derive(Debug, Clone, PartialEq)]
pub enum Csg2 {
    Shape(Shape2),
    Op(Op, Vec<Csg2>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Union,
    Difference,
    Intersection,
}

/// A 3D primitive in its own frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    /// From the origin to `size` (or centred).
    Cube {
        size: [f64; 3],
        center: bool,
    },
    Sphere {
        radius: f64,
    },
    /// Along +Z from 0 to `h` (or centred), radius `r1` at the bottom and `r2` at the top.
    Cylinder {
        h: f64,
        r1: f64,
        r2: f64,
        center: bool,
    },
    /// A 2D shape extruded along +Z from 0 to `h` (or centred).
    Prism {
        shape: Shape2,
        h: f64,
        center: bool,
    },
    /// A 2D shape in the XZ plane (x = radius, y = height) turned about Z by `angle` degrees.
    Revolve {
        shape: Shape2,
        angle: f64,
    },
}

/// A 3D CSG tree. Leaves carry their full placement.
#[derive(Debug, Clone, PartialEq)]
pub enum Csg {
    Leaf { prim: Prim, at: M4, line: u32 },
    Op(Op, Vec<Csg>),
}

/// What a file evaluates to: one tree per top-level object, and what was skipped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    pub objects: Vec<Csg>,
    pub warnings: Vec<String>,
}

/// A problem in the file, with its line (0 when unknown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScadError {
    pub line: u32,
    pub message: String,
    /// Text written in the file itself (an `assert` message): kept out of `message`, so
    /// callers can label it as untrusted (security review X5).
    pub user_text: Option<String>,
}

impl ScadError {
    pub(crate) fn at(line: u32, m: impl Into<String>) -> ScadError {
        ScadError { line, message: m.into(), user_text: None }
    }
}

impl std::fmt::Display for ScadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 { write!(f, "line {}: {}", self.line, self.message) } else { f.write_str(&self.message) }
    }
}

impl std::error::Error for ScadError {}

/// Parses and evaluates OpenSCAD source.
///
/// # Errors
/// Syntax errors, unsupported constructs that change the geometry, exceeded limits.
pub fn read(src: &str) -> Result<Model, ScadError> {
    read_with(src, &Limits::default())
}

/// [`read`] with explicit limits.
///
/// # Errors
/// As [`read`].
pub fn read_with(src: &str, limits: &Limits) -> Result<Model, ScadError> {
    read_cancellable(src, limits, None)
}

/// [`read_with`] that stops soon after `cancel` is set (security review X3).
///
/// # Errors
/// As [`read`]; "cancelled" when stopped.
pub fn read_cancellable(src: &str, limits: &Limits, cancel: Option<&AtomicBool>) -> Result<Model, ScadError> {
    // Parsing and evaluation recurse up to the depth limits; give them a stack that
    // holds that depth whatever thread the caller runs on.
    std::thread::scope(|sc| {
        std::thread::Builder::new()
            .name("gusset-scad".into())
            .stack_size(STACK)
            .spawn_scoped(sc, || {
                let toks = parse::lex(src)?;
                let prog = parse::Parser::new(toks).program()?;
                eval::evaluate(&prog, limits, cancel)
            })
            .map_err(|_| ScadError::at(0, "could not start the OpenSCAD evaluator"))?
            .join()
            .map_err(|_| ScadError::at(0, "the OpenSCAD evaluator failed"))?
    })
}

/// Stack for parsing and evaluation (the depth limits fit with a wide margin).
const STACK: usize = 256 << 20;

#[cfg(test)]
mod tests;
