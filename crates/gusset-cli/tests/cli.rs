//! End-to-end CLI tests. The geometry checks run only when built with OCCT.

use std::process::Command;

fn gusset() -> Command {
    Command::new(env!("CARGO_BIN_EXE_gusset"))
}

#[test]
fn version_and_usage() {
    let out = gusset().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("gusset "));
    let out = gusset().arg("frobnicate").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = gusset().args(["build", "/nonexistent/x.gus"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[cfg(gusset_occt)]
#[test]
fn bracket_golden_volume_and_exports() {
    let dir = std::env::temp_dir().join(format!("gusset-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ex = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/bracket.gus.txt");
    let step = dir.join("b.step");
    let mf = dir.join("b.3mf");
    let out = gusset().args(["build", ex, "--export"]).arg(&step).arg("--export").arg(&mf).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("volume 11272.584 mm³"), "{text}");
    assert!(std::fs::read(&step).unwrap().starts_with(b"ISO-10303-21;"));
    assert!(std::fs::read(&mf).unwrap().starts_with(b"PK"));
    // Pack to .gus and rebuild from the binary project.
    let gus = dir.join("b.gus");
    assert!(gusset().args(["pack", ex, "-o"]).arg(&gus).status().unwrap().success());
    let out = gusset().arg("build").arg(&gus).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("volume 11272.584 mm³"));
    let out = gusset().arg("self-test").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Dogfood F-B1 / review H5: `gusset build --import` imports STEP through the sandboxed
/// worker found next to the executable, and STL as a reference mesh.
#[cfg(gusset_occt)]
#[test]
fn build_imports_step_and_stl() {
    let dir = std::env::temp_dir().join(format!("gusset-cli-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ex = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/bracket.gus.txt");
    let (step, stl) = (dir.join("b.step"), dir.join("b.stl"));
    let out = gusset().args(["build", ex, "--export"]).arg(&step).arg("--export").arg(&stl).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let empty = dir.join("empty.gus.txt");
    std::fs::write(&empty, "gusset 1\nunits mm\n").unwrap();
    let out = gusset().arg("build").arg(&empty).arg("--import").arg(&step).arg("--import").arg(&stl).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("exact geometry"), "{text}");
    assert!(text.contains("volume 11272.584 mm³"), "{text}");
    assert!(text.contains("reference mesh Import2"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// IGES goes to the worker as a private temp file it may read (ADR 0008 §4): export
/// bracket to IGES and import it back. Skipped where Landlock is unavailable (the import
/// is then refused with a clear message, never run unsandboxed).
#[cfg(gusset_occt)]
#[test]
fn build_imports_iges_through_the_sandboxed_worker() {
    let dir = std::env::temp_dir().join(format!("gusset-cli-import-iges-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ex = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/bracket.gus.txt");
    let iges = dir.join("b.igs");
    let out = gusset().args(["build", ex, "--export"]).arg(&iges).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let empty = dir.join("empty.gus.txt");
    std::fs::write(&empty, "gusset 1\nunits mm\n").unwrap();
    let out = gusset().arg("build").arg(&empty).arg("--import").arg(&iges).output().unwrap();
    let (text, err) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
    if format!("{text}{err}").contains("needs Linux Landlock") {
        eprintln!("skipped: no Landlock on this kernel");
        return;
    }
    assert!(out.status.success(), "{text}\n{err}");
    assert!(text.contains("exact geometry"), "{text}");
    assert!(text.contains("volume 11272.584 mm³"), "{text}");
}
