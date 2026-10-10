//! `gusset-worker`: sandboxed importer process. Reads one request on stdin, writes one
//! response frame on stdout, exits. Never started by users directly.

use gusset_kernel::Kernel;
use gusset_worker::{Body, Format, MAX_INPUT, decode_request, encode_response};
use std::io::{Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    // Sandbox first, before any untrusted byte is read.
    let report = gusset_sandbox::lockdown(&gusset_sandbox::Limits::default()).to_text();
    let mut input = Vec::new();
    if std::io::stdin().take(MAX_INPUT + 64).read_to_end(&mut input).is_err() {
        return ExitCode::from(3);
    }
    let result = match decode_request(&input) {
        Ok((format, data)) => import(format, data),
        Err(e) => Err(e),
    };
    let frame = encode_response(&result, &report);
    let mut out = std::io::stdout().lock();
    if out.write_all(&frame).and_then(|()| out.flush()).is_err() {
        return ExitCode::from(4);
    }
    ExitCode::SUCCESS
}

fn import(format: Format, data: &[u8]) -> Result<Vec<Body>, String> {
    let k = gusset_kernel_occt::OcctKernel::new().map_err(|e| e.to_string())?;
    let shapes = match format {
        Format::Step => k.import_step(data),
        Format::Iges => k.import_iges(data),
    }
    .map_err(|e| e.to_string())?;
    if shapes.is_empty() {
        return Err("the file contains no shapes".into());
    }
    shapes
        .into_iter()
        .map(|(s, name)| k.write_native(&s).map(|brep| Body { name, brep }).map_err(|e| e.to_string()))
        .collect()
}
