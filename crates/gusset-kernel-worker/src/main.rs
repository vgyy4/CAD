//! `gusset-kernel-worker`: the geometry kernel in its own sandboxed process (ADR 0022).
//! Serves kernel requests from stdin to stdout until stdin closes. Started by the app;
//! never by users.

use std::process::ExitCode;

/// The only accepted arguments: `--memory-mb <n>` (the memory budget, set by the app).
fn memory_budget() -> Result<u64, ()> {
    let mut args = std::env::args().skip(1);
    let mb = match (args.next().as_deref(), args.next()) {
        (Some(gusset_kernel_worker::MEMORY_FLAG), Some(v)) => v.parse::<u64>().map_err(|_| ())?,
        (None, None) => gusset_kernel_worker::DEFAULT_MEMORY_MB,
        _ => return Err(()),
    };
    if args.next().is_some() || mb == 0 {
        return Err(());
    }
    Ok(mb.saturating_mul(1 << 20))
}

fn main() -> ExitCode {
    let Ok(memory_bytes) = memory_budget() else { return ExitCode::from(2) };
    let codeload = gusset_hw::codeload::harden();
    // No file access, no network, dies with the app, bounded memory. CPU time is not
    // capped: the process lives as long as the document is open; a stuck call is stopped
    // by the app (ADR 0022 §3).
    let report = gusset_sandbox::lockdown(&gusset_sandbox::Limits {
        memory_bytes,
        cpu_seconds: gusset_kernel_worker::NO_CPU_LIMIT,
        read_file: None,
    });
    if let Some(e) = codeload {
        eprintln!("gusset-kernel-worker: DLL search-path hardening failed: {e}");
    }
    if !report.missing.is_empty() {
        eprintln!("gusset-kernel-worker: sandbox incomplete: {}", report.missing.join("; "));
    }
    serve()
}

#[cfg(gusset_occt)]
fn serve() -> ExitCode {
    // The app vets B-rep blobs before sending them (`OcctGate`); this process is the sandbox.
    let kernel = match gusset_kernel_occt::OcctKernel::with_vetted_inputs() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("gusset-kernel-worker: {e}");
            return ExitCode::from(3);
        }
    };
    let server = gusset_kernel_remote::Server::new(kernel);
    let (stdin, stdout) = (std::io::stdin(), std::io::stdout());
    match server.serve(&mut stdin.lock(), &mut stdout.lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gusset-kernel-worker: {e}");
            ExitCode::from(4)
        }
    }
}

#[cfg(not(gusset_occt))]
fn serve() -> ExitCode {
    eprintln!("gusset-kernel-worker: this build does not include the OCCT kernel");
    ExitCode::from(3)
}
