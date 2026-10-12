//! Windows dependent-DLL search flags for the app binary (ADR 0008 §6), as for the CLI
//! and the import worker, and the development RPATH to OCCT.
fn main() {
    println!("cargo::rerun-if-env-changed=DEP_GUSSET_OCCT_LIB_DIR");
    // The loader resolves this binary's own imports (OCCT, the C++ runtime, the GPU
    // stack) only from the application directory and System32.
    // 0xA00 = LOAD_LIBRARY_SEARCH_APPLICATION_DIR (0x200) | LOAD_LIBRARY_SEARCH_SYSTEM32 (0x800).
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/DEPENDENTLOADFLAG:0xA00");
    }
    // GNU ld (the default linker on Linux arm64) resolves OCCT's own inter-library
    // dependencies (TKXCAF -> TKVCAF, ...) only through the RPATH, not the -L search
    // path. Release bundles replace it with an install-relative one (packaging/stage.sh).
    if let Ok(dir) = std::env::var("DEP_GUSSET_OCCT_LIB_DIR") {
        match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
            Ok("linux") => println!("cargo::rustc-link-arg=-Wl,--disable-new-dtags,-rpath,{dir}"),
            Ok("windows") => {}
            _ => println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}"),
        }
    }
}
