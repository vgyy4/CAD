//! The only place in this crate (besides the kernel dispatch call sites in
//! `crate::kernels`) that contains `unsafe` code: thin, audited wrappers around OS APIs.
//!
//! Rules for this module:
//! * every `unsafe` block has a `// SAFETY:` comment (enforced by
//!   `clippy::undocumented_unsafe_blocks = "deny"`);
//! * wrappers expose safe functions with owned/primitive results only;
//! * no pointers or handles escape a wrapper.

#[cfg(target_vendor = "apple")]
pub(crate) mod apple;
#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(target_arch = "x86_64")]
pub(crate) mod x86;
