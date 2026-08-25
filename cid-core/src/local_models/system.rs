//! What this machine can actually run.
//!
//! The Local Models UI previously listed runtimes and nothing else, so a user
//! had no way to know whether a given model would run here, run badly, or fail
//! outright. Picking a 70B model on a 16 GB laptop is not a preference, it is a
//! mistake the product should be able to prevent.
//!
//! Everything here is measured or explicitly unknown — no guessing. GPU
//! detection in particular shells out to the vendor tool and reports `None`
//! when it is absent, rather than inventing a number that would change a model
//! recommendation.

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuInfo {
    pub name: String,
    /// Dedicated video memory. `None` means "present but not reported" — which
    /// is different from zero, and is why this is not defaulted.
    pub vram_mb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCapability {
    pub os: String,
    pub arch: String,
    pub cpu_cores: usize,
    pub total_ram_mb: u64,
    /// Free right now. The number that decides whether a model loads *today*,
    /// as opposed to whether the machine could ever host it.
    pub available_ram_mb: u64,
    pub gpus: Vec<GpuInfo>,
    /// Total dedicated VRAM across detected GPUs, when any reported it.
    pub total_vram_mb: Option<u64>,
}

/// Detection shells out to the GPU vendor tool, which on Windows means a
/// PowerShell/CIM query costing seconds. The hardware does not change between
/// calls, so the answer is cached and the UI does not sit on a spinner every
/// time the panel opens. `detect_fresh` exists for the explicit Rescan.
static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<SystemCapability>>> =
    std::sync::OnceLock::new();

impl SystemCapability {
    /// Cached after the first call.
    pub fn detect_cached() -> Self {
        let cell = CACHE.get_or_init(|| std::sync::Mutex::new(None));
        if let Ok(guard) = cell.lock() {
            if let Some(cached) = guard.as_ref() {
                return cached.clone();
            }
        }
        let fresh = Self::detect();
        if let Ok(mut guard) = cell.lock() {
            *guard = Some(fresh.clone());
        }
        fresh
    }

    /// Re-probe and replace the cache. Memory figures move constantly, and a
    /// GPU can genuinely appear (eGPU, driver install), so Rescan is real work
    /// rather than a no-op.
    pub fn detect_fresh() -> Self {
        let fresh = Self::detect();
        if let Some(cell) = CACHE.get() {
            if let Ok(mut guard) = cell.lock() {
                *guard = Some(fresh.clone());
            }
        }
        fresh
    }

    pub fn detect() -> Self {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        sys.refresh_cpu();

        let gpus = detect_gpus();
        let total_vram_mb = {
            let reported: Vec<u64> = gpus.iter().filter_map(|g| g.vram_mb).collect();
            if reported.is_empty() {
                None
            } else {
                Some(reported.iter().sum())
            }
        };

        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_cores: sys.cpus().len(),
            total_ram_mb: sys.total_memory() / 1024 / 1024,
            available_ram_mb: sys.available_memory() / 1024 / 1024,
            gpus,
            total_vram_mb,
        }
    }

    /// The memory a model actually gets to use. GPU-resident inference is bound
    /// by VRAM; CPU inference by system RAM. Taking the larger of the two is
    /// what decides whether a given quantisation fits.
    pub fn usable_model_memory_mb(&self) -> u64 {
        self.total_vram_mb
            .unwrap_or(0)
            .max(self.total_ram_mb.saturating_sub(RESERVED_SYSTEM_RAM_MB))
    }
}

/// Headroom left for the OS and everything else the user has open. Without it a
/// model that technically "fits" total RAM swaps the machine to a standstill.
const RESERVED_SYSTEM_RAM_MB: u64 = 4 * 1024;

fn detect_gpus() -> Vec<GpuInfo> {
    if let Some(gpus) = nvidia_gpus() {
        return gpus;
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(gpus) = windows_gpus() {
            return gpus;
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(gpus) = macos_gpus() {
            return gpus;
        }
    }
    Vec::new()
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn nvidia_gpus() -> Option<Vec<GpuInfo>> {
    let out = run(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ],
    )?;
    let gpus: Vec<GpuInfo> = out
        .lines()
        .filter_map(|line| {
            let (name, mem) = line.split_once(',')?;
            Some(GpuInfo {
                name: name.trim().to_string(),
                vram_mb: mem.trim().parse::<u64>().ok(),
            })
        })
        .collect();
    (!gpus.is_empty()).then_some(gpus)
}

#[cfg(target_os = "windows")]
fn windows_gpus() -> Option<Vec<GpuInfo>> {
    // `AdapterRAM` is a 32-bit field and saturates at 4 GB, so it is read only
    // as a name source here; an over-4GB card would otherwise be reported as
    // exactly 4096 MB and quietly cap the recommendations.
    let out = run(
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
        ],
    )?;
    let gpus: Vec<GpuInfo> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|name| GpuInfo {
            name: name.to_string(),
            vram_mb: None,
        })
        .collect();
    (!gpus.is_empty()).then_some(gpus)
}

#[cfg(target_os = "macos")]
fn macos_gpus() -> Option<Vec<GpuInfo>> {
    // Apple Silicon shares one pool between CPU and GPU, so there is no
    // separate VRAM figure to report; `usable_model_memory_mb` falls back to
    // system RAM, which is the correct bound there.
    let out = run("system_profiler", &["SPDisplaysDataType"])?;
    let gpus: Vec<GpuInfo> = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Chipset Model:"))
        .map(|name| GpuInfo {
            name: name.trim().to_string(),
            vram_mb: None,
        })
        .collect();
    (!gpus.is_empty()).then_some(gpus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_reports_real_numbers_for_this_machine() {
        let cap = SystemCapability::detect();
        assert!(cap.cpu_cores > 0, "a machine running tests has CPUs");
        assert!(
            cap.total_ram_mb > 256,
            "total RAM should be a plausible figure, got {}MB",
            cap.total_ram_mb
        );
        assert!(cap.available_ram_mb <= cap.total_ram_mb);
        assert!(!cap.os.is_empty() && !cap.arch.is_empty());
    }

    #[test]
    fn usable_memory_reserves_headroom_for_the_os() {
        let cap = SystemCapability {
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu_cores: 8,
            total_ram_mb: 16 * 1024,
            available_ram_mb: 8 * 1024,
            gpus: vec![],
            total_vram_mb: None,
        };
        // 16 GB machine must not be treated as 16 GB of model budget.
        assert_eq!(cap.usable_model_memory_mb(), 12 * 1024);
    }

    #[test]
    fn a_reported_gpu_raises_the_budget_above_system_ram() {
        let cap = SystemCapability {
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu_cores: 8,
            total_ram_mb: 8 * 1024,
            available_ram_mb: 4 * 1024,
            gpus: vec![GpuInfo {
                name: "RTX 4090".into(),
                vram_mb: Some(24 * 1024),
            }],
            total_vram_mb: Some(24 * 1024),
        };
        assert_eq!(cap.usable_model_memory_mb(), 24 * 1024);
    }

    /// A machine with less RAM than the reserve must report zero budget, not
    /// wrap around to an enormous number.
    #[test]
    fn a_tiny_machine_reports_no_budget_rather_than_underflowing() {
        let cap = SystemCapability {
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu_cores: 2,
            total_ram_mb: 1024,
            available_ram_mb: 512,
            gpus: vec![],
            total_vram_mb: None,
        };
        assert_eq!(cap.usable_model_memory_mb(), 0);
    }
}
