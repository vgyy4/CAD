//! Builds the C++ shim against OCCT when `GUSSET_OCCT_DIR` points to an OCCT install
//! prefix (with `include/opencascade` and `lib`). Without it the crate builds with no
//! native code and reports OCCT as unavailable (`cfg(gusset_occt)` is not set), so the
//! rest of the workspace builds and tests without a C++ toolchain.
//!
//! This is a *build-time* setting. The shipped app never reads library paths from the
//! environment (ADR 0008); it loads the OCCT libraries bundled next to the binary.

use std::path::PathBuf;

const OCCT_LIBS: &[&str] = &[
    "TKernel",
    "TKMath",
    "TKG2d",
    "TKG3d",
    "TKGeomBase",
    "TKBRep",
    "TKGeomAlgo",
    "TKTopAlgo",
    "TKPrim",
    "TKBO",
    "TKShHealing",
    "TKFillet",
    "TKMesh",
    "TKBool",
    "TKOffset",
    "TKFeat",
    "TKXSBase",
    "TKDE",
    "TKDESTEP",
    "TKDEIGES",
    "TKLCAF",
    "TKCAF",
    "TKCDF",
    "TKXCAF",
    "TKService",
    "TKV3d",
    "TKHLR",
];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(gusset_occt)");
    println!("cargo::rerun-if-env-changed=GUSSET_OCCT_DIR");
    println!("cargo::rerun-if-changed=cpp/gusset_occt.cpp");
    println!("cargo::rerun-if-changed=cpp/gusset_occt.h");
    let Some(dir) = std::env::var_os("GUSSET_OCCT_DIR").map(PathBuf::from) else {
        println!("cargo::warning=GUSSET_OCCT_DIR not set: building gusset-occt-sys without OCCT");
        return;
    };
    let include = dir.join("include").join("opencascade");
    let lib = dir.join("lib");
    if !include.join("Standard.hxx").exists() {
        panic!("GUSSET_OCCT_DIR={} does not contain include/opencascade/Standard.hxx", dir.display());
    }
    let mut b = cc::Build::new();
    b.cpp(true).file("cpp/gusset_occt.cpp").include(&include).include("cpp").warnings(false);
    if b.get_compiler().is_like_msvc() {
        b.flag("/std:c++17").flag("/EHsc").flag("/bigobj");
    } else {
        b.flag("-std=c++17");
    }
    b.compile("gusset_occt_shim");
    println!("cargo::rustc-link-search=native={}", lib.display());
    for l in OCCT_LIBS {
        if lib.join(format!("lib{l}.so")).exists()
            || lib.join(format!("{l}.lib")).exists()
            || lib.join(format!("lib{l}.dylib")).exists()
        {
            println!("cargo::rustc-link-lib=dylib={l}");
        }
    }
    // This crate's own test binary needs to find the libraries too (dependents add the
    // same RPATH in their build scripts; release bundles use install-relative paths).
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => println!("cargo::rustc-link-arg=-Wl,--disable-new-dtags,-rpath,{}", lib.display()),
        Ok("windows") => {}
        _ => println!("cargo::rustc-link-arg=-Wl,-rpath,{}", lib.display()),
    }
    println!("cargo::rustc-cfg=gusset_occt");
    // Dependents read DEP_GUSSET_OCCT_LIB_DIR in their build scripts to enable cfg(gusset_occt).
    println!("cargo::metadata=lib_dir={}", lib.display());
}
