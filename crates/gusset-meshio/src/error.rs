//! Structured errors with human-readable messages.

use std::fmt;

use crate::Format;

/// Everything that can go wrong while importing or exporting a mesh file.
///
/// Every variant renders to a sentence a user can act on, saying *what* is wrong and
/// *where* (triangle number, line number, ZIP entry, …).
#[derive(Debug)]
#[non_exhaustive]
pub enum MeshIoError {
    /// The input exceeds a configured [`crate::Limits`] value.
    LimitExceeded {
        /// What was counted, e.g. "file size (bytes)" or "triangle count".
        what: &'static str,
        /// The value found, when it is known.
        actual: Option<u64>,
        /// The configured limit.
        limit: u64,
    },
    /// The file is structurally invalid.
    Malformed {
        format: Format,
        /// Where the problem is, e.g. "line 12" or "object 3, triangle 7".
        location: String,
        detail: String,
    },
    /// A coordinate or transform value is NaN or infinite.
    NonFinite {
        format: Format,
        /// e.g. "triangle 1234" or "line 88".
        location: String,
        /// True for NaN, false for ±infinity.
        nan: bool,
    },
    /// A path inside a container is absolute, escapes with `..`, or is otherwise unsafe.
    UnsafePath { path: String },
    /// The file uses a feature this importer does not implement.
    Unsupported { format: Format, detail: String },
    /// The format could not be determined from the file name or its content.
    UnknownFormat,
    /// The file contains no triangles at all.
    Empty { format: Format },
    /// A mesh handed to an exporter is invalid (bad indices, NaN coordinates, …).
    InvalidMesh { body: String, detail: String },
    /// Reading or writing failed.
    Io(std::io::Error),
    /// The operation was cancelled through [`crate::ImportOptions::cancel`].
    Cancelled,
}

impl fmt::Display for MeshIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshIoError::LimitExceeded { what, actual: Some(a), limit } => {
                write!(f, "{what} {a} exceeds the limit of {limit}")
            }
            MeshIoError::LimitExceeded { what, actual: None, limit } => {
                write!(f, "{what} exceeds the limit of {limit}")
            }
            MeshIoError::Malformed { format, location, detail } => write!(f, "{format} {location}: {detail}"),
            MeshIoError::NonFinite { format, location, nan } => {
                let what = if *nan { "a NaN" } else { "an infinite" };
                write!(f, "{format} {location} has {what} coordinate")
            }
            MeshIoError::UnsafePath { path } => {
                write!(f, "unsafe path inside the file: {path:?} (absolute, '..' or invalid characters)")
            }
            MeshIoError::Unsupported { format, detail } => write!(f, "{format} feature not supported: {detail}"),
            MeshIoError::UnknownFormat => {
                f.write_str("unrecognised mesh format (expected STL, 3MF or OBJ by extension or content)")
            }
            MeshIoError::Empty { format } => write!(f, "{format} file contains no triangles"),
            MeshIoError::InvalidMesh { body, detail } => write!(f, "body {body:?} cannot be exported: {detail}"),
            MeshIoError::Io(e) => write!(f, "I/O error: {e}"),
            MeshIoError::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for MeshIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MeshIoError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for MeshIoError {
    fn from(e: std::io::Error) -> Self {
        MeshIoError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, MeshIoError>;

pub(crate) fn malformed(format: Format, location: impl Into<String>, detail: impl Into<String>) -> MeshIoError {
    MeshIoError::Malformed { format, location: location.into(), detail: detail.into() }
}

pub(crate) fn non_finite(format: Format, location: impl Into<String>, v: f64) -> MeshIoError {
    MeshIoError::NonFinite { format, location: location.into(), nan: v.is_nan() }
}

pub(crate) fn limit(what: &'static str, actual: u64, limit: u64) -> MeshIoError {
    MeshIoError::LimitExceeded { what, actual: Some(actual), limit }
}
