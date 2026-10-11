//! Persistent-naming golden scenarios (ADR 0003, review H7) with the real OCCT kernel;
//! the same scenarios run against the mock kernel in gusset-commands.

#[cfg(gusset_occt)]
#[test]
fn naming_golden_scenarios_with_occt() {
    use std::sync::Arc;
    let kernel: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")));
    if let Err(e) = gusset_commands::golden::all(&kernel) {
        panic!("{e}");
    }
}
