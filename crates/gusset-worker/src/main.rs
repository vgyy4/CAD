//! `gusset-worker`: sandboxed importer process. Reads one request on stdin, writes one
//! response frame on stdout, exits. Never started by users directly.

use gusset_worker::{Body, Format, INPUT_FILE_FLAG, MAX_INPUT, decode_request, encode_response, iges_input};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The only accepted arguments: nothing, or `--input-file <path>` (IGES; the path is
/// trusted, it comes from the parent and is needed before the sandbox is applied).
fn input_file() -> Result<Option<PathBuf>, ()> {
    let mut args = std::env::args_os().skip(1);
    let file = match args.next() {
        None => return Ok(None),
        Some(flag) if flag == INPUT_FILE_FLAG => args.next().map(PathBuf::from).ok_or(())?,
        Some(_) => return Err(()),
    };
    if args.next().is_some() || !file.is_absolute() {
        return Err(());
    }
    Ok(Some(file))
}

fn main() -> ExitCode {
    let Ok(file) = input_file() else { return ExitCode::from(2) };
    // Code-loading hardening first (Windows DLL search path; ADR 0008 §6). A failure is
    // reported with the sandbox report.
    let codeload = gusset_hw::codeload::harden();
    // Sandbox next, before any untrusted byte is read.
    let mut sandbox = gusset_sandbox::lockdown(&gusset_sandbox::Limits {
        read_file: file.clone(),
        ..gusset_sandbox::Limits::default()
    });
    if let Some(e) = codeload {
        sandbox.missing.push(format!("DLL search-path hardening ({e})"));
    }
    let report = sandbox.to_text();
    let mut input = Vec::new();
    if std::io::stdin().take(MAX_INPUT + 64).read_to_end(&mut input).is_err() {
        return ExitCode::from(3);
    }
    let result = match decode_request(&input) {
        Ok((format, data)) => run(format, data, file.as_deref(), sandbox.read_file.as_deref()),
        Err(e) => Err(e),
    };
    let frame = encode_response(&result, &report);
    let mut out = std::io::stdout().lock();
    if out.write_all(&frame).and_then(|()| out.flush()).is_err() {
        return ExitCode::from(4);
    }
    ExitCode::SUCCESS
}

#[cfg(gusset_occt)]
fn run(format: Format, data: &[u8], file: Option<&Path>, granted: Option<&Path>) -> Result<Vec<Body>, String> {
    use gusset_occt_sys::safe;
    let err = |e: safe::ShimError| format!("geometry kernel error: {e}");
    match format {
        Format::Step | Format::Iges => {
            let shapes = if format == Format::Iges {
                // Only with a sandbox-confined file; never unsandboxed.
                safe::read_iges_file(iges_input(file, granted)?).map_err(err)?
            } else {
                safe::read_shapes(data).map_err(err)?
            };
            if shapes.is_empty() {
                return Err("the file contains no shapes".into());
            }
            shapes
                .iter()
                .enumerate()
                .map(|(i, s)| Ok(Body { name: format!("Body {}", i + 1), brep: safe::brep_write(s).map_err(err)? }))
                .collect()
        }
        Format::BrepText => {
            let s = safe::brep_text_read(data).map_err(err)?;
            safe::topology(&s).map_err(err)?;
            let brep = safe::brep_write(&s).map_err(err)?;
            Ok(vec![Body { name: String::new(), brep }])
        }
        Format::Brep => {
            // Parse, walk the whole topology (what the app does next with the shape),
            // and re-serialize with OCCT's writer: the app parses only that output.
            let s = safe::brep_read(data).map_err(err)?;
            safe::topology(&s).map_err(err)?;
            let brep = safe::brep_write(&s).map_err(err)?;
            Ok(vec![Body { name: String::new(), brep }])
        }
    }
}

#[cfg(not(gusset_occt))]
fn run(format: Format, _: &[u8], file: Option<&Path>, granted: Option<&Path>) -> Result<Vec<Body>, String> {
    if format == Format::Iges {
        iges_input(file, granted)?;
    }
    Err("this build of Gusset does not include the OCCT kernel".into())
}
