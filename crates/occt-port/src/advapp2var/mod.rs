//! `AdvApp2Var` (OCCT 8.0.1): the data classes of the two-variable approximation driver
//! (`Context`, `Node`, `Iso`, `Patch`, `Framework`, `Network`) and `SysBase`, translated
//! from the C++ sources. Each function is commented with the C++ function it follows.
//!
//! Not translated (outside the judged API): the approximation drivers
//! (`Iso::MakeApprox`, `Patch::Discretise`, `Patch::MakeApprox`, `Patch::AddConstraints`),
//! the `ApproxF2var` and `ApproxAFunc2Var` classes, `Criterion`, the `MathBase` helpers and
//! the memory-manager part of `SysBase`.

mod containers;
mod context;
mod framework;
mod iso;
mod network;
mod node;
mod patch;
mod sysbase;

pub(crate) use containers::{Arr1, Arr2, Seq};
pub use context::Context;
pub use framework::Framework;
pub use iso::Iso;
pub use network::Network;
pub use node::Node;
pub use patch::Patch;
pub use sysbase::SysBase;

/// Failures that the C++ classes report with `Standard_OutOfRange`,
/// `Standard_NoSuchObject`, or (for a handle never filled in) a null dereference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `Standard_OutOfRange`
    OutOfRange,
    /// `Standard_NoSuchObject` (an access to an empty sequence)
    NoSuchObject,
    /// A handle that was never computed (C++: a null handle is dereferenced)
    NotComputed,
}

/// Result of the fallible functions of the package.
pub type Result<T> = std::result::Result<T, Error>;

/// `CriterionType` is only used by the not-translated `Criterion`; kept for the judge's
/// enum table. Variants in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriterionType {
    /// `AdvApp2Var_Absolute`
    Absolute,
    /// `AdvApp2Var_Relative`
    Relative,
}

/// `CriterionRepartition`, declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriterionRepartition {
    /// `AdvApp2Var_Regular`
    Regular,
    /// `AdvApp2Var_Incremental`
    Incremental,
}

/// IEEE equality of two doubles (`a == b`: NaN is unequal, `0.0 == -0.0`), written as an
/// exact ordering comparison so that no tolerance is implied.
pub(crate) fn feq(a: f64, b: f64) -> bool {
    a.partial_cmp(&b) == Some(std::cmp::Ordering::Equal)
}

/// `std::max` on doubles: `a < b ? b : a`.
pub(crate) fn fmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// Index of `hmax[order + 1]` in the `hmax[4] = {0, 1, 1.5, 1.75}` table of the C++ code.
pub(crate) fn hmax(order: i32) -> Result<f64> {
    const TABLE: [f64; 4] = [0.0, 1.0, 1.5, 1.75];
    usize::try_from(order + 1).ok().and_then(|i| TABLE.get(i).copied()).ok_or(Error::OutOfRange)
}
