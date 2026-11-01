//! Opt-in launcher/DLL loader check in a NEW helper process, never in the game.
//! Only metadata queries are allowed; no start/stop, worker, network or hooks.
use crate::runtime_info::{Info, Source};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoaderStage {
    Load,
    Release,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoaderFailure {
    pub schema: u8,
    pub stage: LoaderStage,
    pub windows_code: u32,
}
impl std::fmt::Display for LoaderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "own DLL loader {:?} failed (Windows code {})",
            self.stage, self.windows_code
        )
    }
}
impl std::error::Error for LoaderFailure {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: u8,
    pub dll_sha256: String,
    pub runtime: Info,
    pub module_released: bool,
    pub worker_started: bool,
    pub game_tested: bool,
    pub lan_tested: bool,
}
impl Report {
    pub fn validate(&self) -> Result<()> {
        self.runtime.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            self.schema == 1
                && self.dll_sha256.len() == 64
                && self
                    .dll_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && self.runtime.software_version == env!("CARGO_PKG_VERSION")
                && self.runtime.source == Source::None
                && self.module_released
                && !self.worker_started
                && !self.game_tested
                && !self.lan_tested,
            "invalid isolated loader evidence"
        );
        Ok(())
    }
}

#[cfg(any(windows, test))]
mod supervision {
    use super::*;
    use std::{
        io::Read,
        process::{Child, ExitStatus},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    const MAX_OUTPUT: usize = 8192;
    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                // This handle belongs exclusively to our newly spawned helper.
                // Never an attached game/controller PID or remote DLL thread.
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    pub(super) fn collect(child: Child, timeout: Duration) -> Result<(ExitStatus, Vec<u8>)> {
        let mut child = OwnedChild(child);
        let deadline = Instant::now() + timeout;
        let stdout = child
            .0
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("helper stdout missing"))?;
        let (tx, rx) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("runtime-check-output".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let result = stdout
                    .take((MAX_OUTPUT + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map(|_| bytes);
                let _ = tx.send(result);
            })?;
        let mut output = None;
        loop {
            if output.is_none()
                && let Ok(bytes) = rx.try_recv()
            {
                let bytes = bytes?;
                ensure!(
                    bytes.len() <= MAX_OUTPUT,
                    "isolated helper output exceeded 8192 bytes"
                );
                output = Some(bytes);
            }
            if let Some(status) = child.0.try_wait()? {
                let bytes = match output {
                    Some(bytes) => bytes,
                    None => rx
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .map_err(|_| anyhow::anyhow!("isolated helper output timeout"))??,
                };
                ensure!(
                    bytes.len() <= MAX_OUTPUT,
                    "isolated helper output exceeded 8192 bytes"
                );
                return Ok((status, bytes));
            }
            ensure!(
                Instant::now() < deadline,
                "isolated helper timed out; only its own process was stopped"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::process::{Command, Stdio};
        const MODE: &str = "AA_OWN_RUNTIME_CHECK_FIXTURE";
        fn child(mode: &str) -> Child {
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "runtime_check::supervision::tests::owned_helper",
                    "--nocapture",
                ])
                .env(MODE, mode)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        }
        #[test]
        fn owned_helper() {
            match std::env::var(MODE).as_deref() {
                Ok("hang") => thread::sleep(Duration::from_secs(30)),
                Ok("flood") => {
                    use std::io::Write;
                    let _ = std::io::stdout().write_all(&[b'x'; MAX_OUTPUT * 4]);
                    thread::sleep(Duration::from_secs(30));
                }
                _ => {}
            }
        }
        #[test]
        fn own_helper_timeout_and_output_flood_are_bounded_and_reaped() {
            for (mode, limit) in [
                ("hang", Duration::from_millis(100)),
                ("flood", Duration::from_secs(3)),
            ] {
                let started = Instant::now();
                assert!(collect(child(mode), limit).is_err());
                assert!(started.elapsed() < Duration::from_secs(8));
            }
            let (status, bytes) = collect(child("ok"), Duration::from_secs(5)).unwrap();
            assert!(status.success() && bytes.len() < MAX_OUTPUT);
        }
    }
}

#[cfg(windows)]
fn dll_hash(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::{fs::File, io::Read};
    let file = File::open(path)?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.len() <= 64 * 1024 * 1024,
        "invalid client DLL size/type"
    );
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == meta.len(),
        "client DLL changed or exceeds size limit"
    );
    let binary = crate::doctor::binary::inspect(&bytes, true).map_err(anyhow::Error::msg)?;
    ensure!(
        binary.format == "pe32"
            && binary.machine == "x86"
            && binary.exports.contains("client_control"),
        "expected own x86 client DLL with control export"
    );
    let nt = u32::from_le_bytes(bytes[60..64].try_into()?) as usize;
    ensure!(
        u16::from_le_bytes(bytes[nt + 22..nt + 24].try_into()?) & 0x2000 != 0,
        "client file must be a DLL, not an executable"
    );
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Run only inside the short-lived `mp_launcher runtime-check` process.
#[cfg(windows)]
pub fn local(dll: &std::path::Path) -> Result<Report> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{FreeLibrary, GetLastError},
        System::{
            Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode},
            LibraryLoader::{
                GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
            },
        },
    };
    ensure!(
        cfg!(target_arch = "x86"),
        "runtime check requires Windows x86 helper"
    );
    let dll = dll.canonicalize()?;
    ensure!(
        dll.file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case("client_dll.dll")),
        "select client_dll.dll"
    );
    let hash = dll_hash(&dll)?;
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
    ensure!(
        unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null(),
        "runtime check requires a fresh helper without this DLL loaded"
    );
    // SAFETY: this NEW helper process alone changes its error UI policy. The
    // absolute file passed to the loader is checked PE32 with our export name.
    // DllMain is inert; the two calls below only query our documented Control ABI.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    }
    let module = unsafe {
        LoadLibraryExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    if module.is_null() {
        return Err(LoaderFailure {
            schema: 1,
            stage: LoaderStage::Load,
            windows_code: unsafe { GetLastError() },
        }
        .into());
    }
    // On any failure keep the module until this helper exits. Never unload
    // without lifecycle evidence, and never start/stop a worker to obtain it.
    let function = unsafe { GetProcAddress(module, c"client_control".as_ptr().cast()) }
        .ok_or_else(|| anyhow::anyhow!("control export unavailable"))?;
    let control: unsafe extern "system" fn(*mut crate::api::Control) -> u32 =
        unsafe { std::mem::transmute(function) };
    let query = |command| -> Result<crate::api::Control> {
        let mut block = crate::api::Control::new(command);
        ensure!(
            unsafe { control(&mut block) } == 0
                && block.result == 0
                && block.version == crate::api::ABI_VERSION,
            "client DLL refused metadata ABI"
        );
        Ok(block)
    };
    let info = query(crate::api::RUNTIME_INFO)?;
    let runtime: Info =
        serde_json::from_str(crate::api::text(&info.message).map_err(anyhow::Error::msg)?)?;
    let state = query(crate::api::LIFECYCLE_STATUS)?;
    let lifecycle: crate::lifecycle::State =
        serde_json::from_str(crate::api::text(&state.message).map_err(anyhow::Error::msg)?)?;
    lifecycle.require_unload().map_err(anyhow::Error::msg)?;
    let report = Report {
        schema: 1,
        dll_sha256: hash,
        runtime,
        module_released: true,
        worker_started: false,
        game_tested: false,
        lan_tested: false,
    };
    report.validate()?;
    if unsafe { FreeLibrary(module) } == 0 {
        return Err(LoaderFailure {
            schema: 1,
            stage: LoaderStage::Release,
            windows_code: unsafe { GetLastError() },
        }
        .into());
    }
    ensure!(
        unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null(),
        "client DLL remained loaded; retaining it until this helper exits"
    );
    Ok(report)
}

/// Execute the bundled launcher, not a game PID. A failure kills/reaps only
/// this owned helper. Output is bounded/typed and excludes stderr/path text.
#[cfg(windows)]
pub fn run(kit: &std::path::Path) -> Result<Report> {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    let kit = kit.canonicalize()?;
    let dll = kit.join("client_dll.dll");
    let expected = dll_hash(&dll)?;
    let child = Command::new(kit.join("mp_launcher.exe"))
        .args(["--pid", "0", "--dll"])
        .arg(&dll)
        .arg("runtime-check")
        .current_dir(&kit)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let (status, bytes) = supervision::collect(child, Duration::from_secs(10))?;
    if !status.success() {
        if let Ok(failure) = serde_json::from_slice::<LoaderFailure>(&bytes)
            && failure.schema == 1
            && failure.windows_code != 0
        {
            return Err(failure.into());
        }
        anyhow::bail!(
            "launcher/DLL runtime check failed (exit {:?}); components may be incompatible",
            status.code()
        );
    }
    let report: Report = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid isolated loader report"))?;
    report.validate()?;
    ensure!(
        report.dll_sha256 == expected && dll_hash(&dll)? == expected,
        "client DLL changed during runtime check"
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_is_bounded_typed_and_cannot_claim_game_network_or_started_worker() {
        let clean = Report {
            schema: 1,
            dll_sha256: "a".repeat(64),
            runtime: Info::selected(0, 0),
            module_released: true,
            worker_started: false,
            game_tested: false,
            lan_tested: false,
        };
        clean.validate().unwrap();
        let bytes = serde_json::to_vec(&clean).unwrap();
        assert!(bytes.len() < 1024);
        for key in ["worker_started", "game_tested", "lan_tested"] {
            let mut bad = serde_json::to_value(&clean).unwrap();
            bad[key] = true.into();
            assert!(
                serde_json::from_value::<Report>(bad)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut bad = clean.clone();
        bad.module_released = false;
        assert!(bad.validate().is_err());
        bad = clean.clone();
        bad.runtime.source = Source::Simulated;
        assert!(bad.validate().is_err());
        bad = clean.clone();
        bad.dll_sha256 = "private/path".into();
        assert!(bad.validate().is_err());
        bad = clean;
        bad.schema = 2;
        assert!(bad.validate().is_err());
        let extra = String::from_utf8(bytes)
            .unwrap()
            .replace("\"schema\":1", "\"schema\":1,\"path\":\"private\"");
        assert!(serde_json::from_str::<Report>(&extra).is_err());
    }
}
