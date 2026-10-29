//! Own-process observations only. CPU includes the driver and every client,
//! Tokio worker and embedded server; it is not server-only cost or game FPS.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Sample {
    pub cpu_ms: Option<u64>,
    pub resident_bytes: Option<u64>,
    pub peak_resident_bytes: Option<u64>,
}

#[cfg(windows)]
pub fn sample() -> Sample {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::{
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{GetCurrentProcess, GetProcessTimes},
        },
    };
    // SAFETY: current-process pseudo handle, correctly sized stack outputs,
    // no retained pointers and no access to another process or game memory.
    unsafe {
        let process = GetCurrentProcess();
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let ticks = |v: FILETIME| (u64::from(v.dwHighDateTime) << 32) | u64::from(v.dwLowDateTime);
        let cpu_ms = (GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)
            != 0)
            .then(|| ticks(kernel).saturating_add(ticks(user)) / 10000);
        let mut memory: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        memory.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let measured = K32GetProcessMemoryInfo(process, &mut memory, memory.cb) != 0;
        Sample {
            cpu_ms,
            resident_bytes: measured.then_some(memory.WorkingSetSize as u64),
            peak_resident_bytes: measured.then_some(memory.PeakWorkingSetSize as u64),
        }
    }
}

#[cfg(target_os = "linux")]
pub fn sample() -> Sample {
    let status = std::fs::read_to_string("/proc/self/status").ok();
    let stat = std::fs::read_to_string("/proc/self/stat").ok();
    // SAFETY: sysconf takes no pointers; nonpositive results remain unavailable.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    Sample {
        cpu_ms: stat.and_then(|s| linux_cpu(&s, i128::from(hz))),
        resident_bytes: status.as_deref().and_then(|s| linux_memory(s, "VmRSS:")),
        peak_resident_bytes: status.as_deref().and_then(|s| linux_memory(s, "VmHWM:")),
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn sample() -> Sample {
    Sample::default()
}

#[cfg(any(target_os = "linux", test))]
fn linux_cpu(stat: &str, hz: i128) -> Option<u64> {
    let hz = u64::try_from(hz).ok().filter(|n| *n > 0)?;
    let fields: Vec<_> = stat
        .get(stat.rfind(')')? + 1..)?
        .split_whitespace()
        .collect();
    let user = fields.get(11)?.parse::<u64>().ok()?;
    let kernel = fields.get(12)?.parse::<u64>().ok()?;
    Some(user.checked_add(kernel)?.checked_mul(1000)? / hz)
}

#[cfg(any(target_os = "linux", test))]
fn linux_memory(status: &str, name: &str) -> Option<u64> {
    let line = status.lines().find(|s| s.starts_with(name))?;
    let mut fields = line.strip_prefix(name)?.split_whitespace();
    let kb = fields.next()?.parse::<u64>().ok()?;
    (fields.next()? == "kB" && fields.next().is_none())
        .then(|| kb.checked_mul(1024))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proc_fixtures_handle_names_units_and_invalid_ticks() {
        let stat = "42 (worker ) name) S 0 0 0 0 0 0 0 0 0 0 123 77";
        assert_eq!(linux_cpu(stat, 100), Some(2000));
        assert_eq!(linux_cpu(stat, 0), None);
        assert_eq!(linux_cpu("bad", 100), None);
        assert_eq!(
            linux_memory("VmRSS:\t123 kB\nVmHWM: 456 kB", "VmRSS:"),
            Some(125952)
        );
        assert_eq!(linux_memory("VmRSS: 123 MB", "VmRSS:"), None);
    }
    #[test]
    fn current_process_measurements_have_no_identifiers_and_missing_is_not_zero() {
        let value = sample();
        #[cfg(any(windows, target_os = "linux"))]
        assert!(value.cpu_ms.is_some() && value.resident_bytes.is_some_and(|n| n > 0));
        assert!(
            serde_json::to_string(&Sample::default())
                .unwrap()
                .contains("null")
        );
    }
}
