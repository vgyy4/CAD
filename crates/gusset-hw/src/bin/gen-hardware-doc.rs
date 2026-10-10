//! Regenerates the registry-derived sections of `docs/HARDWARE.md`.
//!
//! ```text
//! cargo run -p gusset-hw --bin gen-hardware-doc            # rewrite in place
//! cargo run -p gusset-hw --bin gen-hardware-doc -- --check # exit 1 if out of date
//! cargo run -p gusset-hw --bin gen-hardware-doc -- PATH    # another file
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut check = false;
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("docs").join("HARDWARE.md");
    for arg in std::env::args().skip(1) {
        if arg == "--check" {
            check = true;
        } else {
            path = PathBuf::from(arg);
        }
    }
    if let Ok(p) = path.canonicalize() {
        path = p;
    }
    let doc = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let new = match gusset_hw::docgen::update_document(&doc) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("error: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    if new == doc {
        println!("{} is up to date", path.display());
        return ExitCode::SUCCESS;
    }
    if check {
        eprintln!("{} is out of date; run `cargo run -p gusset-hw --bin gen-hardware-doc`", path.display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(&path, new) {
        eprintln!("error: cannot write {}: {e}", path.display());
        return ExitCode::FAILURE;
    }
    println!("updated {}", path.display());
    ExitCode::SUCCESS
}
