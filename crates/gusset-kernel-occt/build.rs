//! Enables `cfg(gusset_occt)` when gusset-occt-sys was built against OCCT.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(gusset_occt)");
    println!("cargo::rerun-if-env-changed=DEP_GUSSET_OCCT_LIB_DIR");
    if let Ok(dir) = std::env::var("DEP_GUSSET_OCCT_LIB_DIR") {
        println!("cargo::rustc-cfg=gusset_occt");
        // Lets this crate's tests find the shared libraries without installing them.
        // (Release bundles use an install-relative RPATH instead; ADR 0008.)
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
        }
    }
}
