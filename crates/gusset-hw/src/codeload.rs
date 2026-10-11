//! Code-loading hardening, called as the first statement of every Gusset binary's
//! `main` (ADR 0008 §6, security review #2).
//!
//! On Windows this restricts where `LoadLibrary` looks for DLLs that are loaded by
//! bare name (wgpu loads `dxcompiler.dll`, `libEGL.dll` and others that way): only the
//! application directory, `System32` and directories added with `AddDllDirectory`.
//! The current directory and `PATH` are never searched, so a DLL planted next to a
//! project file cannot be loaded. Optional DLLs that are not installed then simply fail
//! to load: wgpu falls back from DXC to FXC (`d3dcompiler_47.dll` in System32) and the
//! GL backend reports itself unavailable.
//!
//! Elsewhere it is a no-op: Linux and macOS binaries use install-relative RPATHs.

/// Applies the hardening. Returns a description of what failed, if anything; callers
/// keep running (the process is still usable) but should surface it in diagnostics.
pub fn harden() -> Option<String> {
    #[cfg(windows)]
    {
        crate::sys::windows::restrict_dll_search().err()
    }
    #[cfg(not(windows))]
    {
        None
    }
}
