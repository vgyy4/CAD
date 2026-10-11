//! Windows dependent-DLL search flags for the app binary (ADR 0008 §6), as for the CLI
//! and the import worker.
fn main() {
    // The loader resolves this binary's own imports (OCCT, the C++ runtime, the GPU
    // stack) only from the application directory and System32.
    // 0xA00 = LOAD_LIBRARY_SEARCH_APPLICATION_DIR (0x200) | LOAD_LIBRARY_SEARCH_SYSTEM32 (0x800).
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/DEPENDENTLOADFLAG:0xA00");
    }
}
