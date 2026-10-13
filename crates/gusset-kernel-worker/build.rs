//! Enables `cfg(gusset_occt)` when gusset-occt-sys was built against OCCT, and sets the
//! Windows dependent-DLL search flags.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(gusset_occt)");
    println!("cargo::rerun-if-env-changed=DEP_GUSSET_OCCT_LIB_DIR");
    // Windows: the loader resolves this binary's own imports (OCCT, the C++ runtime)
    // only from the application directory and System32 (ADR 0008 §6).
    // 0xA00 = LOAD_LIBRARY_SEARCH_APPLICATION_DIR (0x200) | LOAD_LIBRARY_SEARCH_SYSTEM32 (0x800).
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/DEPENDENTLOADFLAG:0xA00");
    }
    if let Ok(dir) = std::env::var("DEP_GUSSET_OCCT_LIB_DIR") {
        println!("cargo::rustc-cfg=gusset_occt");
        // Lets this crate's tests find the shared libraries without installing them.
        // (Release bundles use an install-relative RPATH instead; ADR 0008.)
        match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
            // DT_RPATH (not RUNPATH) so OCCT's own inter-library dependencies resolve too.
            Ok("linux") => println!("cargo::rustc-link-arg=-Wl,--disable-new-dtags,-rpath,{dir}"),
            Ok("windows") => {}
            _ => println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}"),
        }
    }
}
