//! Work limits and cancellation of an evaluation (security review V1, review H3).
//!
//! Geometric evaluation of a small document can be made arbitrarily expensive: a body
//! pattern multiplies faces, chained patterns double the body per line, and every boolean
//! costs time in the kernel roughly in proportion to the faces it handles. So every
//! feature that can multiply a body (holes, body patterns, mirror with merge and `combine`)
//! is charged before the kernel is asked:
//!
//! - its **faces in** (the target's faces plus every tool's), which also bounds the faces
//!   of its result: more than [`EvalLimits::max_body_faces`] fails the feature;
//! - the same number added to the evaluation's running **boolean work**, at most
//!   [`EvalLimits::max_boolean_work`];
//! - its **tools** (copies, holes, tool bodies), at most [`EvalLimits::max_tools`] per
//!   evaluation.
//!
//! The running totals live in the per-feature state, so whether a feature is within the
//! limits depends only on the document (and the limits), never on what happened to be
//! cached. There is no wall-clock limit for the same reason (results must not depend on
//! the machine's speed); a host stops a long rebuild with [`crate::Evaluator::cancel_flag`]
//! instead, which is checked between features and before every charged boolean.

use crate::{ErrorCode, EvalError};

/// Limits of one evaluation. The defaults allow, for example, a plate with 1,000 counterbored
/// holes, or a 500-instance pattern of a 20-face body, and stop a chain of doubling patterns
/// of a box at its 11th line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalLimits {
    /// Most faces one boolean feature may put in (target plus tools; about the faces of
    /// its result).
    pub max_body_faces: u64,
    /// Most faces put through booleans in one evaluation, summed over the features.
    pub max_boolean_work: u64,
    /// Most boolean tools (pattern instances, holes, tool bodies) in one evaluation.
    pub max_tools: u64,
    /// Most joint placements the loop solver computes in one evaluation, all closing joints
    /// together (security review v0.3 W1).
    pub max_joint_steps: u64,
}

/// See [`EvalLimits::max_body_faces`].
pub const DEFAULT_MAX_BODY_FACES: u64 = 10_000;
/// See [`EvalLimits::max_boolean_work`].
pub const DEFAULT_MAX_BOOLEAN_WORK: u64 = 1_000_000;
/// See [`EvalLimits::max_tools`].
pub const DEFAULT_MAX_TOOLS: u64 = 20_000;
/// See [`EvalLimits::max_joint_steps`].
pub const DEFAULT_MAX_JOINT_STEPS: u64 = 2_000_000;

impl Default for EvalLimits {
    fn default() -> Self {
        EvalLimits {
            max_body_faces: DEFAULT_MAX_BODY_FACES,
            max_boolean_work: DEFAULT_MAX_BOOLEAN_WORK,
            max_tools: DEFAULT_MAX_TOOLS,
            max_joint_steps: DEFAULT_MAX_JOINT_STEPS,
        }
    }
}

/// The work charged so far in an evaluation (part of the per-feature state).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Work {
    pub(crate) faces: u64,
    pub(crate) tools: u64,
    /// Joint placements computed by the loop solver so far (security review v0.3 W1).
    pub(crate) joint_steps: u64,
}

impl Work {
    /// Charges one boolean feature: `tools` tools and `faces_in` faces in. `what` names
    /// the operation in the error ("the pattern", "the holes").
    pub(crate) fn charge(
        &mut self,
        limits: &EvalLimits,
        feature: &str,
        what: &str,
        tools: usize,
        faces_in: u64,
    ) -> Result<(), EvalError> {
        let too = |msg: String, hint: &str| EvalError::new(feature, ErrorCode::TooComplex, msg).with_hint(hint);
        if faces_in > limits.max_body_faces {
            return Err(too(
                format!("{what} would make a body of about {faces_in} faces (the limit is {})", limits.max_body_faces),
                "use fewer instances or holes, or pattern a simpler body",
            ));
        }
        let tools = u64::try_from(tools).unwrap_or(u64::MAX);
        self.tools = self.tools.saturating_add(tools);
        if self.tools > limits.max_tools {
            return Err(too(
                format!("the part has more than {} pattern instances, holes and tool bodies in all", limits.max_tools),
                "use fewer instances or holes",
            ));
        }
        self.faces = self.faces.saturating_add(faces_in);
        if self.faces > limits.max_boolean_work {
            return Err(too(
                format!(
                    "rebuilding the part up to here puts {} faces through boolean operations (the limit is {})",
                    self.faces, limits.max_boolean_work
                ),
                "the part is too complex: use fewer or smaller patterns and holes",
            ));
        }
        Ok(())
    }
}

/// The error of a feature that was not (fully) evaluated because the rebuild was cancelled.
pub(crate) fn cancelled(feature: &str) -> EvalError {
    EvalError::new(feature, ErrorCode::Cancelled, "the rebuild was cancelled")
        .with_hint("the part is rebuilt again with the next change")
}
