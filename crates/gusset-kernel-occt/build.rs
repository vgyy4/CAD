//! Enables `cfg(gusset_occt)` when gusset-occt-sys was built against OCCT.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(gusset_occt)");
    println!("cargo::rerun-if-env-changed=DEP_GUSSET_OCCT_LIB_DIR");
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
