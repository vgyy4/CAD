//! `gusset-worker`: sandboxed importer process. Reads one request on stdin, writes one
//! response frame on stdout, exits. Never started by users directly.

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
        Ok((format, data)) => run(format, data),
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
fn run(format: Format, data: &[u8]) -> Result<Vec<Body>, String> {
    use gusset_occt_sys::safe;
    let err = |e: safe::ShimError| format!("geometry kernel error: {e}");
    match format {
        Format::Step | Format::Iges => {
            let shapes = safe::read_shapes(data, format == Format::Iges).map_err(err)?;
            if shapes.is_empty() {
                return Err("the file contains no shapes".into());
            }
            shapes
                .iter()
                .enumerate()
                .map(|(i, s)| Ok(Body { name: format!("Body {}", i + 1), brep: safe::brep_write(s).map_err(err)? }))
                .collect()
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
fn run(_: Format, _: &[u8]) -> Result<Vec<Body>, String> {
    Err("this build of Gusset does not include the OCCT kernel".into())
}
