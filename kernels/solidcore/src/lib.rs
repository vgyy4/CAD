//! [`Kernel`] on SolidCore (github.com/tangency-labs/solidcore, Apache-2.0), a safe-Rust
//! B-rep kernel ported from NVIDIA's SMLib (ADR 0023). It is the second backend the
//! per-operation router can send work to; decisions per operation come from the
//! conformance cases in `tests.rs` and the measurements in
//! `docs/research/solidcore-2026-11.md`.
//!
//! SolidCore is opt-in (feature `solidcore`): without it this crate builds a stub whose
//! [`SolidcoreKernel::new`] reports the kernel unavailable.
//!
//! What the adapter does itself, because SolidCore's operation layer (`solidcore-ops`)
//! does not expose it:
//! - **Face history.** `solidcore-ops` keeps its provenance hook crate-private and returns
//!   no face history, so face origins are recovered geometrically: a result face that lies
//!   on an input face of the same surface kind is that face's (`FaceOrigin::Input`);
//!   extrude and revolve faces are matched to the profile segment they were swept from.
//! - **Per-face measures.** Face area, centroid and normal come from a fine mesh (chord
//!   1/5000 of the body), exact for planes, within the chord for curved faces.

#![cfg_attr(test, allow(clippy::float_cmp))]

use gusset_kernel::KResult;
#[cfg(not(feature = "solidcore"))]
use gusset_kernel::{
    BBox, Kernel, KernelError, KernelErrorKind, MassProps, Mesh, OpResult, Profile, Shape, TessellationParams, Topology,
};

#[cfg(feature = "solidcore")]
mod imp;
#[cfg(all(test, feature = "solidcore"))]
mod tests;

/// The SolidCore commit this adapter is built and measured against (Cargo.toml pins it).
pub const SOLIDCORE_REV: &str = "f4f70127afee1640c43d9f61e035baddd6dc271b";

/// The SolidCore kernel. It holds no state: SolidCore's operation layer runs one kernel per
/// process under a lock.
#[derive(Debug, Clone, Copy)]
pub struct SolidcoreKernel {
    _private: (),
}

impl SolidcoreKernel {
    /// The kernel, with every SolidCore `UpstreamFixes` flag on (the configuration SolidCraft
    /// and SolidCore's own operation tests run), or an error if this build does not include
    /// SolidCore.
    pub fn new() -> KResult<SolidcoreKernel> {
        #[cfg(feature = "solidcore")]
        {
            imp::init()?;
            Ok(SolidcoreKernel { _private: () })
        }
        #[cfg(not(feature = "solidcore"))]
        {
            Err(unavailable())
        }
    }
}

#[cfg(not(feature = "solidcore"))]
fn unavailable() -> KernelError {
    KernelError::new(KernelErrorKind::Unsupported, "this build of Gusset does not include the SolidCore kernel")
        .with_hint("build gusset-kernel-solidcore with the `solidcore` feature")
}

#[cfg(not(feature = "solidcore"))]
impl Kernel for SolidcoreKernel {
    fn name(&self) -> &'static str {
        "solidcore"
    }
    fn extrude(&self, _: &Profile, _: &gusset_kernel::ExtrudeSpec) -> KResult<OpResult> {
        Err(unavailable())
    }
    fn compound(&self, _: &[Shape]) -> KResult<OpResult> {
        Err(unavailable())
    }
    fn topology(&self, _: &Shape) -> KResult<Topology> {
        Err(unavailable())
    }
    fn mass_properties(&self, _: &Shape) -> KResult<MassProps> {
        Err(unavailable())
    }
    fn bbox(&self, _: &Shape) -> KResult<BBox> {
        Err(unavailable())
    }
    fn tessellate(&self, _: &Shape, _: &TessellationParams) -> KResult<Mesh> {
        Err(unavailable())
    }
}

#[cfg(all(test, not(feature = "solidcore")))]
mod stub_tests {
    use super::*;

    #[test]
    fn without_the_feature_the_kernel_is_unavailable() {
        let err = SolidcoreKernel::new().unwrap_err();
        assert_eq!(err.kind, KernelErrorKind::Unsupported);
        assert!(err.hint.is_some());
    }
}
