//! CPU and memory usage straight from /proc. No crates needed.
//!
//! Linux-only: the `stats` command is compiled out elsewhere.

use std::fs;
use std::io;

/// Aggregate jiffies from the `cpu` line of /proc/stat.
#[derive(Copy, Clone, Debug)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
}

pub fn read_cpu_times() -> io::Result<CpuTimes> {
    let stat = fs::read_to_string("/proc/stat")?;
    let line = stat
        .lines()
        .next()
        .filter(|l| l.starts_with("cpu "))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no cpu line in /proc/stat"))?;

    // user nice system idle iowait irq softirq steal guest guest_nice
    let vals: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();

    if vals.len() < 5 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected /proc/stat format",
        ));
    }

    // idle + iowait both count as not-busy.
    let idle = vals[3] + vals[4];
    Ok(CpuTimes {
        total: vals.iter().sum(),
        idle,
    })
}

/// Busy percentage between two samples. Returns 0 if the clock didn't advance.
pub fn cpu_percent(prev: CpuTimes, now: CpuTimes) -> u8 {
    let total_delta = now.total.saturating_sub(prev.total);
    let idle_delta = now.idle.saturating_sub(prev.idle);
    if total_delta == 0 {
        return 0;
    }
    let busy = total_delta.saturating_sub(idle_delta);
    ((busy * 100) / total_delta).min(100) as u8
}

/// Used memory as a percentage, using MemAvailable (what the kernel thinks is
/// actually reclaimable) rather than MemFree.
pub fn ram_percent() -> io::Result<u8> {
    let meminfo = fs::read_to_string("/proc/meminfo")?;
    let mut total = 0u64;
    let mut available = 0u64;

    for line in meminfo.lines() {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("MemTotal:"), Some(v)) => total = v.parse().unwrap_or(0),
            (Some("MemAvailable:"), Some(v)) => available = v.parse().unwrap_or(0),
            _ => {}
        }
        if total != 0 && available != 0 {
            break;
        }
    }

    if total == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "could not read MemTotal",
        ));
    }

    let used = total.saturating_sub(available);
    Ok(((used * 100) / total).min(100) as u8)
}

/// Best-effort GPU busy percentage.
///
/// Intel (i915/xe) exposes no simple percentage without perf counters, and
/// amdgpu does via `gpu_busy_percent`. Returns None when we can't tell, and the
/// caller just skips the update rather than showing a fake number.
pub fn gpu_percent() -> Option<u8> {
    for entry in fs::read_dir("/sys/class/drm").ok()? {
        let path = entry.ok()?.path().join("device/gpu_busy_percent");
        if let Ok(s) = fs::read_to_string(&path) {
            if let Ok(v) = s.trim().parse::<u8>() {
                return Some(v.min(100));
            }
        }
    }
    None
}
