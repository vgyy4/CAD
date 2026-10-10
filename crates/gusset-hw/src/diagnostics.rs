//! Hardware diagnostics shown in the About panel and exposed as an MCP resource.
//!
//! Plain data (no serde) with a deterministic text form: the same machine and the same
//! GPU fields always produce byte-identical output, with keys in a fixed order.

use std::fmt::Write as _;

use crate::budgets::Budgets;
use crate::cpu::CpuInfo;
use crate::dispatch::{self, ActiveDispatch};
use crate::registry::GpuClass;
use crate::tier::QualityTier;

/// GPU facts, filled in by the renderer once an adapter is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuDiagnostics {
    /// Adapter name as reported by the driver (wgpu `AdapterInfo::name`).
    pub adapter_name: String,
    /// Backend in use ("vulkan", "dx12", "metal", "gl").
    pub backend: String,
    /// Adapter class.
    pub class: GpuClass,
    /// Driver name (wgpu `AdapterInfo::driver`), may be empty.
    pub driver: String,
    /// Driver version/info string (wgpu `AdapterInfo::driver_info`), may be empty.
    pub driver_info: String,
    /// Tier in use.
    pub tier: QualityTier,
    /// Whether the user overrode the automatic tier.
    pub tier_overridden: bool,
    /// Backends tried and failed before this one, in order (fallback chain).
    pub failed_backends: Vec<String>,
}

/// Everything the diagnostics panel shows about the hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostics {
    /// CPU, cores and memory.
    pub cpu: CpuInfo,
    /// Dispatch level in use.
    pub dispatch: ActiveDispatch,
    /// Resource budgets in use.
    pub budgets: Budgets,
    /// GPU, once the renderer has chosen an adapter.
    pub gpu: Option<GpuDiagnostics>,
}

impl Diagnostics {
    /// Collect CPU-side diagnostics for this process (dispatch from
    /// [`dispatch::active`]). GPU fields stay empty until [`Diagnostics::set_gpu`].
    #[must_use]
    pub fn collect() -> Self {
        let cpu = CpuInfo::detect();
        let budgets = Budgets::for_cpu(&cpu);
        Self { cpu, dispatch: dispatch::active(), budgets, gpu: None }
    }

    /// Record the renderer's adapter and tier.
    pub fn set_gpu(&mut self, gpu: GpuDiagnostics) {
        self.gpu = Some(gpu);
    }

    /// Deterministic `key: value` report, one item per line.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut s = String::from("# Gusset hardware diagnostics\n");
        s.push_str(&self.cpu.to_text());
        let forced = if self.dispatch.forced { " (forced)" } else { "" };
        let _ = writeln!(s, "dispatch.level: {}{forced}", self.dispatch.level);
        let supported: Vec<&str> = dispatch::supported_levels(&self.cpu.features).iter().map(|d| d.name()).collect();
        let _ = writeln!(s, "dispatch.supported: {}", supported.join(" "));
        s.push_str(&self.budgets.to_text());
        match &self.gpu {
            None => s.push_str("gpu: not initialized\n"),
            Some(g) => {
                let _ = writeln!(s, "gpu.adapter: {}", one_line(&g.adapter_name));
                let _ = writeln!(s, "gpu.backend: {}", one_line(&g.backend));
                let _ = writeln!(s, "gpu.class: {}", g.class.label());
                let _ = writeln!(s, "gpu.driver: {}", one_line(&g.driver));
                let _ = writeln!(s, "gpu.driver_info: {}", one_line(&g.driver_info));
                let o = if g.tier_overridden { " (user override)" } else { " (automatic)" };
                let _ = writeln!(s, "gpu.tier: {}{o}", g.tier.name());
                let spec = g.tier.spec();
                let _ = writeln!(
                    s,
                    "gpu.tier_effects: msaa={} ssao={} shadows={} lod={:.2}",
                    spec.msaa_samples, spec.ssao, spec.shadows, spec.lod_detail_factor
                );
                let failed = if g.failed_backends.is_empty() {
                    "none".to_owned()
                } else {
                    g.failed_backends.iter().map(|b| one_line(b)).collect::<Vec<_>>().join(" ")
                };
                let _ = writeln!(s, "gpu.failed_backends: {failed}");
            }
        }
        s
    }
}

/// Driver strings are untrusted: keep them on one line so they cannot forge report keys.
fn one_line(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let t = t.trim();
    if t.is_empty() { "unknown".to_owned() } else { t.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::{CoreTopology, CpuFeatures};
    use crate::dispatch::Dispatch;

    fn fixed() -> Diagnostics {
        let cpu = CpuInfo {
            arch: "x86_64",
            os: "linux",
            brand: Some("Intel(R) Core(TM) i5-6200U CPU @ 2.30GHz".into()),
            machine_model: None,
            features: CpuFeatures::from_names(&["sse4.2", "popcnt", "avx", "avx2", "fma"]),
            logical_cores: 4,
            physical_cores: Some(2),
            topology: CoreTopology::Homogeneous,
            total_ram_bytes: Some(8 * 1024 * 1024 * 1024),
        };
        let budgets = Budgets::for_cpu(&cpu);
        Diagnostics { cpu, dispatch: ActiveDispatch { level: Dispatch::Scalar, forced: true }, budgets, gpu: None }
    }

    #[test]
    fn text_is_deterministic_and_complete() {
        let mut d = fixed();
        let a = d.to_text();
        assert_eq!(a, fixed().to_text());
        assert!(a.contains("cpu.brand: Intel(R) Core(TM) i5-6200U CPU @ 2.30GHz\n"));
        assert!(a.contains("cpu.features: sse4.2 popcnt avx avx2 fma\n"));
        assert!(a.contains("dispatch.level: scalar (forced)\n"));
        assert!(a.contains("memory.total: 8192 MiB\n"));
        assert!(a.contains("gpu: not initialized\n"));
        d.set_gpu(GpuDiagnostics {
            adapter_name: "Intel(R) HD Graphics 520\nbackend: evil".into(),
            backend: "vulkan".into(),
            class: GpuClass::Integrated,
            driver: "Intel open-source Mesa driver".into(),
            driver_info: String::new(),
            tier: QualityTier::Low,
            tier_overridden: false,
            failed_backends: vec![],
        });
        let b = d.to_text();
        assert!(b.contains("gpu.adapter: Intel(R) HD Graphics 520 backend: evil\n"));
        assert!(b.contains("gpu.tier: low (automatic)\n"));
        assert!(b.contains("gpu.driver_info: unknown\n"));
        assert!(b.contains("gpu.tier_effects: msaa=1 ssao=false shadows=false lod=1.00\n"));
        // Every line is `key: value` or the header.
        for line in b.lines().skip(1) {
            assert!(line.contains(": "), "{line}");
        }
    }

    #[test]
    fn collect_works_on_this_machine() {
        let d = Diagnostics::collect();
        assert!(d.to_text().contains("dispatch.level: "));
    }
}
