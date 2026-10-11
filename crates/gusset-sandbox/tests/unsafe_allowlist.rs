//! Enforces ADR 0008 §2: only the listed crates may relax the workspace's
//! `unsafe_code = "forbid"`. Every other crate must inherit the workspace lints (a
//! `forbid` cannot be lowered by `#[allow]` in source), and the ADR's list must name
//! exactly the crates allowed here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

/// The crates that may contain `unsafe` (keep in sync with ADR 0008 §2).
const ALLOWED: &[&str] = &["gusset-occt-sys", "gusset-store", "gusset-hw", "gusset-sandbox"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn crates() -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(root().join("crates"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), p))
        .collect();
    v.sort();
    v
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        let ft = e.file_type().unwrap();
        if ft.is_dir() && p.file_name().is_some_and(|n| n != "target") {
            rust_files(&p, out);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn only_listed_crates_relax_the_unsafe_lint() {
    let all = crates();
    assert!(all.len() > 10, "crate scan found too few crates: {all:?}");
    for (name, dir) in &all {
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let inherits = manifest.contains("[lints]\nworkspace = true");
        let listed = ALLOWED.contains(&name.as_str());
        if !listed {
            assert!(inherits, "{name} must use `[lints] workspace = true` (unsafe forbidden; ADR 0008 §2)");
            assert!(!manifest.contains("unsafe_code"), "{name} must not configure unsafe_code");
            let mut files = Vec::new();
            rust_files(dir, &mut files);
            for f in files {
                let src = std::fs::read_to_string(&f).unwrap();
                assert!(
                    !src.contains("allow(unsafe_code)"),
                    "{} allows unsafe code outside the ADR 0008 list",
                    f.display()
                );
            }
        } else {
            assert!(manifest.contains("unsafe_code"), "{name} is listed but does not configure unsafe_code");
        }
    }
}

#[test]
fn adr_lists_exactly_the_allowed_crates() {
    let adr = std::fs::read_to_string(root().join("docs/adr/0008-security-architecture.md")).unwrap();
    let start = adr.find("**Memory safety.**").expect("ADR 0008 §2 list");
    let end = start + adr[start..].find("The reviewer checks").expect("end of the list");
    let section = &adr[start..end];
    let named: Vec<&str> =
        section.lines().filter_map(|l| l.trim().strip_prefix("- `")).filter_map(|l| l.split('`').next()).collect();
    let mut want: Vec<&str> = ALLOWED.to_vec();
    want.sort_unstable();
    let mut got = named.clone();
    got.sort_unstable();
    assert_eq!(got, want, "ADR 0008 §2 and this test disagree");
}
