//! Protecting a stdout protocol stream from native libraries.
//!
//! `gusset mcp` speaks JSON-RPC on stdout. Native code in the same process (OCCT's
//! messenger, GPU driver loaders) may print to file descriptor 1 / the C runtime's
//! `stdout`, which would corrupt that stream. [`take_stdout_for_protocol`] hands the
//! original stdout to the caller as an owned [`File`] and points the process's stdout at
//! stderr, so stray output from anywhere ends up in the diagnostics stream instead.
//!
//! The behavior is tested end to end by the `gusset mcp` CLI tests (redirecting the
//! test harness's own stdout here would hide test output).

use std::fs::File;
use std::io::{self, Write};

/// Duplicates the process's stdout into a new owned [`File`] (for the protocol) and then
/// redirects stdout to stderr: on Unix `dup2(2, 1)`; on Windows the standard output
/// handle (`SetStdHandle`) and the C runtime's descriptor 1 (`_dup2`).
///
/// Call it once, early, before any native library is loaded or initialized. Rust's
/// `print!` afterwards also goes to stderr.
///
/// # Errors
/// When stdout cannot be duplicated or redirected. Unsupported platforms return
/// `ErrorKind::Unsupported`.
pub fn take_stdout_for_protocol() -> io::Result<File> {
    // Nothing buffered in Rust's stdout may end up on the wrong side of the switch.
    io::stdout().flush()?;
    crate::sys::stdio::take_stdout()
}
