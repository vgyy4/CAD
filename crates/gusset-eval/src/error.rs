//! Structured, actionable feature errors shared by the GUI and MCP (ADR 0012).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    Expression,
    InvalidSketch,
    SketchSolve,
    SketchConflict,
    OpenProfile,
    InvalidReference,
    MissingReference,
    AmbiguousReference,
    NoTargetBody,
    KernelFailure,
    FilletTooLarge,
    EmptyResult,
    Unsupported,
    MissingBlob,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Expression => "expression",
            ErrorCode::InvalidSketch => "invalid_sketch",
            ErrorCode::SketchSolve => "sketch_solve",
            ErrorCode::SketchConflict => "sketch_conflict",
            ErrorCode::OpenProfile => "open_profile",
            ErrorCode::InvalidReference => "invalid_reference",
            ErrorCode::MissingReference => "missing_reference",
            ErrorCode::AmbiguousReference => "ambiguous_reference",
            ErrorCode::NoTargetBody => "no_target_body",
            ErrorCode::KernelFailure => "kernel_failure",
            ErrorCode::FilletTooLarge => "fillet_too_large",
            ErrorCode::EmptyResult => "empty_result",
            ErrorCode::Unsupported => "unsupported",
            ErrorCode::MissingBlob => "missing_blob",
        }
    }
}

/// Why a feature failed, in human terms, with a suggested fix where we have one.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalError {
    pub feature: String,
    pub code: ErrorCode,
    pub message: String,
    pub hint: Option<String>,
    /// A reference involved (e.g. the edge that no longer exists).
    pub reference: Option<String>,
    /// A suggested replacement reference (never applied automatically; ADR 0003).
    pub suggestion: Option<String>,
}

impl EvalError {
    pub fn new(feature: &str, code: ErrorCode, message: impl Into<String>) -> Self {
        EvalError {
            feature: feature.to_string(),
            code,
            message: message.into(),
            hint: None,
            reference: None,
            suggestion: None,
        }
    }
    pub fn with_hint(mut self, h: impl Into<String>) -> Self {
        self.hint = Some(h.into());
        self
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.feature, self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " ({h})")?;
        }
        Ok(())
    }
}

impl std::error::Error for EvalError {}
