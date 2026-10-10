//! CPU detection: ISA features, core counts and topology, RAM, brand string.
//!
//! Sources by platform (best effort; anything not listed is reported as unknown):
//!
//! | Item | Linux | macOS | Windows |
//! |---|---|---|---|
//! | ISA features | `std::arch` runtime detection | same | same |
//! | Logical cores | `available_parallelism` (respects affinity/cgroups) | same | same |
//! | Physical cores | `/sys/devices/system/cpu/cpu*/topology` | `hw.physicalcpu` | unknown |
//! | Hybrid P/E | `/sys/devices/cpu_core`/`cpu_atom`, else `cpu_capacity` | `hw.perflevel{0,1}.logicalcpu` | unknown |
//! | RAM | `/proc/meminfo` | `hw.memsize` | `GlobalMemoryStatusEx` |
//! | Brand | x86 `cpuid`; ARM: MIDR implementer/part from `/proc/cpuinfo` | `machdep.cpu.brand_string` | x86 `cpuid` |

use std::fmt::Write as _;

use crate::registry::{Arch, CPU_FEATURES};

/// The set of registered CPU features (see [`crate::registry::CPU_FEATURES`]) present
/// on a CPU, in registry order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CpuFeatures {
    present: Vec<&'static str>,
}

impl CpuFeatures {
    /// Detect the running CPU's features.
    #[must_use]
    pub fn detect() -> Self {
        let present = CPU_FEATURES
            .iter()
            .filter(|f| Some(f.arch) == Arch::current() && runtime_detect(f.name) == Some(true))
            .map(|f| f.name)
            .collect();
        Self { present }
    }

    /// Build a feature set from names (unregistered names are ignored). For tests and
    /// for describing other machines.
    #[must_use]
    pub fn from_names(names: &[&str]) -> Self {
        let present = CPU_FEATURES.iter().filter(|f| names.contains(&f.name)).map(|f| f.name).collect();
        Self { present }
    }

    /// Whether `name` is present.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.present.contains(&name)
    }

    /// Present feature names, in registry order.
    pub fn iter(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.present.iter().copied()
    }
}

/// Runtime detection of one registered feature. `None` if the name is not a feature of
/// the running architecture (or the architecture is not covered).
#[must_use]
pub fn runtime_detect(name: &str) -> Option<bool> {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::is_x86_feature_detected as d;
        let r = match name {
            "sse4.2" => d!("sse4.2"),
            "popcnt" => d!("popcnt"),
            "avx" => d!("avx"),
            "avx2" => d!("avx2"),
            "fma" => d!("fma"),
            "avx512f" => d!("avx512f"),
            "avx512bw" => d!("avx512bw"),
            "avx512vl" => d!("avx512vl"),
            "avxvnni" => d!("avxvnni"),
            _ => return None,
        };
        Some(r)
    }
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::is_aarch64_feature_detected as d;
        let r = match name {
            "neon" => d!("neon"),
            "dotprod" => d!("dotprod"),
            "i8mm" => d!("i8mm"),
            "sve" => d!("sve"),
            "sve2" => d!("sve2"),
            _ => return None,
        };
        Some(r)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = name;
        None
    }
}

/// Core topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoreTopology {
    /// All cores of one kind.
    Homogeneous,
    /// Performance + efficiency cores. Counts are logical CPUs (hardware threads).
    Hybrid {
        /// Logical CPUs on performance cores.
        performance: usize,
        /// Logical CPUs on efficiency cores (including low-power E-cores).
        efficiency: usize,
    },
    /// Not detectable on this platform.
    #[default]
    Unknown,
}

/// Everything detected about the CPU and memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuInfo {
    /// `std::env::consts::ARCH`.
    pub arch: &'static str,
    /// `std::env::consts::OS`.
    pub os: &'static str,
    /// CPU brand/model string, if available.
    pub brand: Option<String>,
    /// Board/machine model (Linux device tree, e.g. "Raspberry Pi 5 Model B Rev 1.0").
    pub machine_model: Option<String>,
    /// Registered ISA features present.
    pub features: CpuFeatures,
    /// Logical CPUs this process may use (`available_parallelism`; at least 1).
    pub logical_cores: usize,
    /// Physical cores in the machine, if known.
    pub physical_cores: Option<usize>,
    /// Hybrid topology.
    pub topology: CoreTopology,
    /// Installed RAM in bytes, if known.
    pub total_ram_bytes: Option<u64>,
}

impl CpuInfo {
    /// Detect the running machine. Never fails; unknown items are `None`/`Unknown`.
    #[must_use]
    pub fn detect() -> Self {
        let logical_cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let plat = platform::detect();
        Self {
            arch: std::env::consts::ARCH,
            os: std::env::consts::OS,
            brand: x86_brand().or(plat.brand),
            machine_model: plat.machine_model,
            features: CpuFeatures::detect(),
            logical_cores,
            physical_cores: plat.physical_cores,
            topology: plat.topology,
            total_ram_bytes: plat.total_ram_bytes,
        }
    }

    /// Deterministic multi-line report (part of [`crate::Diagnostics::to_text`]).
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "cpu.arch: {}", self.arch);
        let _ = writeln!(s, "cpu.os: {}", self.os);
        let _ = writeln!(s, "cpu.brand: {}", self.brand.as_deref().unwrap_or("unknown"));
        if let Some(m) = &self.machine_model {
            let _ = writeln!(s, "cpu.machine_model: {m}");
        }
        let feats: Vec<&str> = self.features.iter().collect();
        let feats = if feats.is_empty() { "none".to_owned() } else { feats.join(" ") };
        let _ = writeln!(s, "cpu.features: {feats}");
        let _ = writeln!(s, "cpu.logical_cores: {}", self.logical_cores);
        let _ = writeln!(s, "cpu.physical_cores: {}", opt(self.physical_cores));
        let topo = match self.topology {
            CoreTopology::Homogeneous => "homogeneous".to_owned(),
            CoreTopology::Hybrid { performance, efficiency } => {
                format!("hybrid ({performance} performance + {efficiency} efficiency logical CPUs)")
            }
            CoreTopology::Unknown => "unknown".to_owned(),
        };
        let _ = writeln!(s, "cpu.topology: {topo}");
        let ram = self.total_ram_bytes.map_or_else(|| "unknown".to_owned(), |b| format!("{} MiB", b / MIB));
        let _ = writeln!(s, "memory.total: {ram}");
        s
    }
}

const MIB: u64 = 1024 * 1024;

fn opt(v: Option<usize>) -> String {
    v.map_or_else(|| "unknown".to_owned(), |n| n.to_string())
}

/// x86 brand string from CPUID leaves 0x8000_0002..=0x8000_0004.
#[cfg(target_arch = "x86_64")]
fn x86_brand() -> Option<String> {
    use crate::sys::x86::cpuid;
    if cpuid(0x8000_0000).eax < 0x8000_0004 {
        return None;
    }
    let mut bytes = Vec::with_capacity(48);
    for leaf in 0x8000_0002_u32..=0x8000_0004 {
        let r = cpuid(leaf);
        for reg in [r.eax, r.ebx, r.ecx, r.edx] {
            bytes.extend_from_slice(&reg.to_le_bytes());
        }
    }
    clean_string(&bytes)
}

#[cfg(not(target_arch = "x86_64"))]
fn x86_brand() -> Option<String> {
    None
}

/// Trim NULs and whitespace; collapse internal runs of spaces. `None` if empty.
#[cfg(any(target_arch = "x86_64", target_vendor = "apple", target_os = "linux", test))]
pub(crate) fn clean_string(bytes: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(bytes);
    let s: Vec<&str> = s.trim_matches(char::from(0)).split_whitespace().collect();
    let s = s.join(" ").trim_matches(char::from(0)).to_owned();
    if s.is_empty() { None } else { Some(s) }
}

/// What the platform-specific layer found.
#[derive(Debug, Default)]
pub(crate) struct PlatformInfo {
    pub(crate) brand: Option<String>,
    pub(crate) machine_model: Option<String>,
    pub(crate) physical_cores: Option<usize>,
    pub(crate) topology: CoreTopology,
    pub(crate) total_ram_bytes: Option<u64>,
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{CoreTopology, PlatformInfo, clean_string, linux_parse};
    use std::fs;
    use std::path::Path;

    pub(crate) fn detect() -> PlatformInfo {
        let sys = Path::new("/sys/devices/system/cpu");
        let cpus = fs::read_to_string(sys.join("online")).ok().and_then(|s| linux_parse::cpulist(&s));
        let cpus = cpus.unwrap_or_default();
        let mut cores = Vec::new();
        let mut capacities = Vec::new();
        for cpu in &cpus {
            let dir = sys.join(format!("cpu{cpu}"));
            let pkg = read_u64(&dir.join("topology/physical_package_id"));
            let core = read_u64(&dir.join("topology/core_id"));
            if let (Some(p), Some(c)) = (pkg, core) {
                cores.push((p, c));
            }
            if let Some(cap) = read_u64(&dir.join("cpu_capacity")) {
                capacities.push(cap);
            }
        }
        let physical_cores = if !cpus.is_empty() && cores.len() == cpus.len() {
            Some(linux_parse::distinct_pairs(&cores))
        } else {
            None
        };
        let topology = intel_hybrid()
            .or_else(|| (capacities.len() == cpus.len()).then(|| linux_parse::topology_from_capacities(&capacities)))
            .unwrap_or(CoreTopology::Unknown);
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        PlatformInfo {
            brand: linux_parse::cpuinfo_brand(&cpuinfo),
            machine_model: fs::read("/proc/device-tree/model").ok().and_then(|b| clean_string(&b)),
            physical_cores,
            topology,
            total_ram_bytes: fs::read_to_string("/proc/meminfo").ok().and_then(|s| linux_parse::meminfo_total(&s)),
        }
    }

    /// Intel hybrid CPUs expose two PMUs with their CPU lists.
    fn intel_hybrid() -> Option<CoreTopology> {
        let p = fs::read_to_string("/sys/devices/cpu_core/cpus").ok().and_then(|s| linux_parse::cpulist(&s))?;
        let e = fs::read_to_string("/sys/devices/cpu_atom/cpus").ok().and_then(|s| linux_parse::cpulist(&s))?;
        Some(CoreTopology::Hybrid { performance: p.len(), efficiency: e.len() })
    }

    fn read_u64(p: &Path) -> Option<u64> {
        fs::read_to_string(p).ok()?.trim().parse().ok()
    }
}

#[cfg(target_vendor = "apple")]
mod platform {
    use super::{CoreTopology, PlatformInfo};
    use crate::sys::apple::{sysctl_string, sysctl_u64};

    pub(crate) fn detect() -> PlatformInfo {
        let nlevels = sysctl_u64(c"hw.nperflevels").unwrap_or(0);
        let topology = if nlevels >= 2 {
            match (sysctl_u64(c"hw.perflevel0.logicalcpu"), sysctl_u64(c"hw.perflevel1.logicalcpu")) {
                (Some(p), Some(e)) => CoreTopology::Hybrid { performance: to_usize(p), efficiency: to_usize(e) },
                _ => CoreTopology::Unknown,
            }
        } else if nlevels == 1 {
            CoreTopology::Homogeneous
        } else {
            CoreTopology::Unknown
        };
        PlatformInfo {
            brand: sysctl_string(c"machdep.cpu.brand_string"),
            machine_model: sysctl_string(c"hw.model"),
            physical_cores: sysctl_u64(c"hw.physicalcpu").map(to_usize),
            topology,
            total_ram_bytes: sysctl_u64(c"hw.memsize"),
        }
    }

    fn to_usize(v: u64) -> usize {
        usize::try_from(v).unwrap_or(usize::MAX)
    }
}

#[cfg(windows)]
mod platform {
    use super::PlatformInfo;

    pub(crate) fn detect() -> PlatformInfo {
        // Physical cores and P/E topology would need GetLogicalProcessorInformationEx and
        // variable-length record parsing; reported as unknown (see docs/HARDWARE.md).
        PlatformInfo { total_ram_bytes: crate::sys::windows::total_ram_bytes(), ..PlatformInfo::default() }
    }
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
mod platform {
    use super::PlatformInfo;

    pub(crate) fn detect() -> PlatformInfo {
        PlatformInfo::default()
    }
}

/// Pure parsers for Linux `/proc` and `/sys` formats (platform-independent code, so the
/// unit tests run on every platform).
#[cfg(any(target_os = "linux", test))]
pub(crate) mod linux_parse {
    use super::CoreTopology;

    /// Parse a kernel CPU list such as `0-3,8,10-11`. `None` on malformed input.
    pub(crate) fn cpulist(s: &str) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        for part in s.trim().split(',').filter(|p| !p.is_empty()) {
            match part.split_once('-') {
                Some((a, b)) => {
                    let (a, b): (u32, u32) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
                    if b < a || b - a > 65_536 {
                        return None;
                    }
                    out.extend(a..=b);
                }
                None => out.push(part.trim().parse().ok()?),
            }
        }
        Some(out)
    }

    /// `MemTotal` from `/proc/meminfo`, in bytes.
    pub(crate) fn meminfo_total(s: &str) -> Option<u64> {
        let line = s.lines().find(|l| l.starts_with("MemTotal:"))?;
        let mut it = line.split_whitespace().skip(1);
        let n: u64 = it.next()?.parse().ok()?;
        match it.next() {
            Some("kB") | None => n.checked_mul(1024),
            Some(_) => None,
        }
    }

    /// Count distinct (package, core) pairs.
    pub(crate) fn distinct_pairs(pairs: &[(u64, u64)]) -> usize {
        let mut v = pairs.to_vec();
        v.sort_unstable();
        v.dedup();
        v.len()
    }

    /// Heterogeneous `cpu_capacity` values (Arm big.LITTLE / DynamIQ): the CPUs with
    /// the highest capacity are "performance", the rest "efficiency".
    pub(crate) fn topology_from_capacities(caps: &[u64]) -> CoreTopology {
        let Some(&max) = caps.iter().max() else { return CoreTopology::Unknown };
        let performance = caps.iter().filter(|&&c| c == max).count();
        let efficiency = caps.len() - performance;
        if efficiency == 0 { CoreTopology::Homogeneous } else { CoreTopology::Hybrid { performance, efficiency } }
    }

    /// Brand from `/proc/cpuinfo`: x86 `model name`, otherwise the Arm MIDR implementer
    /// and part numbers of the first CPU (e.g. `arm implementer 0x41 part 0xd0b`).
    pub(crate) fn cpuinfo_brand(s: &str) -> Option<String> {
        let field = |key: &str| {
            s.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                (k.trim() == key).then(|| v.trim().to_owned())
            })
        };
        if let Some(m) = field("model name").filter(|m| !m.is_empty()) {
            return Some(m);
        }
        match (field("CPU implementer"), field("CPU part")) {
            (Some(i), Some(p)) => Some(format!("arm implementer {i} part {p}")),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::linux_parse::*;
    use super::*;

    #[test]
    fn cpulist_parsing() {
        assert_eq!(cpulist("0-3,8,10-11\n"), Some(vec![0, 1, 2, 3, 8, 10, 11]));
        assert_eq!(cpulist("0"), Some(vec![0]));
        assert_eq!(cpulist(""), Some(vec![]));
        assert_eq!(cpulist("3-1"), None);
        assert_eq!(cpulist("a-b"), None);
        assert_eq!(cpulist("0-4294967295"), None);
    }

    #[test]
    fn meminfo_parsing() {
        let s = "MemTotal:       16318412 kB\nMemFree:  1 kB\n";
        assert_eq!(meminfo_total(s), Some(16_318_412 * 1024));
        assert_eq!(meminfo_total("MemFree: 1 kB"), None);
        assert_eq!(meminfo_total("MemTotal: x kB"), None);
        assert_eq!(meminfo_total("MemTotal: 1 MB"), None);
    }

    #[test]
    fn topology_parsing() {
        assert_eq!(distinct_pairs(&[(0, 0), (0, 0), (0, 1), (1, 0)]), 3);
        assert_eq!(topology_from_capacities(&[1024; 4]), CoreTopology::Homogeneous);
        assert_eq!(
            topology_from_capacities(&[1024, 1024, 446, 446, 446, 446]),
            CoreTopology::Hybrid { performance: 2, efficiency: 4 }
        );
        assert_eq!(topology_from_capacities(&[]), CoreTopology::Unknown);
    }

    #[test]
    fn cpuinfo_brand_parsing() {
        let x86 = "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i7-6700 CPU @ 3.40GHz\n";
        assert_eq!(cpuinfo_brand(x86).as_deref(), Some("Intel(R) Core(TM) i7-6700 CPU @ 3.40GHz"));
        let arm = "processor\t: 0\nBogoMIPS\t: 108.00\nCPU implementer\t: 0x41\nCPU part\t: 0xd0b\n";
        assert_eq!(cpuinfo_brand(arm).as_deref(), Some("arm implementer 0x41 part 0xd0b"));
        assert_eq!(cpuinfo_brand(""), None);
    }

    #[test]
    fn clean_string_trims() {
        assert_eq!(clean_string(b"  Intel(R)  Core\0\0").as_deref(), Some("Intel(R) Core"));
        assert_eq!(clean_string(b"\0\0"), None);
    }

    #[test]
    fn every_registered_feature_has_a_detector() {
        for f in CPU_FEATURES.iter().filter(|f| Some(f.arch) == Arch::current()) {
            assert!(runtime_detect(f.name).is_some(), "{} has no detector", f.name);
        }
        assert_eq!(runtime_detect("not-a-feature"), None);
    }

    #[test]
    fn detect_is_sane() {
        let info = CpuInfo::detect();
        assert!(info.logical_cores >= 1);
        if let Some(p) = info.physical_cores {
            assert!(p >= 1);
        }
        let text = info.to_text();
        assert!(text.contains("cpu.logical_cores: "));
        // Baseline features are present on any CPU the app supports.
        #[cfg(target_arch = "aarch64")]
        assert!(info.features.has("neon"));
        #[cfg(target_os = "linux")]
        assert!(info.total_ram_bytes.is_some());
    }

    #[test]
    fn from_names_keeps_registry_order_and_drops_unknown() {
        let f = CpuFeatures::from_names(&["fma", "bogus", "avx2"]);
        assert_eq!(f.iter().collect::<Vec<_>>(), vec!["avx2", "fma"]);
    }
}
