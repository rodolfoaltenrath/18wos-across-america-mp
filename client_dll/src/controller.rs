//! Explicit x86 process control. Allocates data only; resolves code in target images.
mod call_lease;
use crate::{
    api::{self, Control},
    inspection::process::{Module, Process},
    pe_exports::{self, Image},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::c_void,
    mem::size_of,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
    System::{
        Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory},
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
        },
        Threading::{
            CreateMutexW, CreateRemoteThread, GetExitCodeThread, OpenProcess,
            PROCESS_CREATE_THREAD, PROCESS_DUP_HANDLE, PROCESS_QUERY_INFORMATION,
            PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE, ReleaseMutex,
            WaitForSingleObject,
        },
    },
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub struct Controller {
    process: Process,
    handle: Handle,
    lock: Handle,
    call_guard: Vec<u16>,
}
impl Drop for Controller {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.lock.0);
        }
    }
}
fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .replace('/', "\\")
        .to_lowercase()
}
impl Controller {
    pub fn open(pid: u32) -> Result<Self> {
        ensure!(
            cfg!(target_pointer_width = "32"),
            "use the Windows x86 launcher"
        );
        let name: Vec<u16> = format!("Local\\AcrossAmericaMP-{pid}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let lock = Handle(unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) });
        ensure!(
            !lock.0.is_null(),
            "cannot create control mutex: {}",
            std::io::Error::last_os_error()
        );
        ensure!(
            matches!(
                unsafe { WaitForSingleObject(lock.0, 5000) },
                WAIT_OBJECT_0 | WAIT_ABANDONED
            ),
            "another controller is busy"
        );
        // Construct after both handles succeed; release mutex on any early failure.
        let result = (|| {
            let process = Process::open(pid)?;
            ensure!(process.identity.pointer_bytes == 4, "target must be x86");
            let handle = Handle(unsafe {
                OpenProcess(
                    PROCESS_CREATE_THREAD
                        | PROCESS_DUP_HANDLE
                        | PROCESS_QUERY_INFORMATION
                        | PROCESS_VM_OPERATION
                        | PROCESS_VM_WRITE
                        | PROCESS_VM_READ,
                    0,
                    pid,
                )
            });
            ensure!(
                !handle.0.is_null(),
                "cannot control PID {pid}: {}",
                std::io::Error::last_os_error()
            );
            // Recheck PID creation identity after obtaining the writable handle.
            let mut created = unsafe { std::mem::zeroed() };
            let mut exit = created;
            let mut kernel = created;
            let mut user = created;
            ensure!(
                unsafe {
                    windows_sys::Win32::System::Threading::GetProcessTimes(
                        handle.0,
                        &mut created,
                        &mut exit,
                        &mut kernel,
                        &mut user,
                    )
                } != 0,
                "cannot verify target session"
            );
            let timestamp =
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
            ensure!(
                timestamp == process.identity.created,
                "PID reused while opening target"
            );
            let call_guard = call_lease::name(pid, process.identity.created);
            call_lease::ensure_clear(&call_guard)?;
            Ok((process, handle, call_guard))
        })();
        match result {
            Ok((process, handle, call_guard)) => Ok(Self {
                process,
                handle,
                lock,
                call_guard,
            }),
            Err(error) => {
                unsafe {
                    ReleaseMutex(lock.0);
                }
                Err(error)
            }
        }
    }
    pub fn executable(&self) -> PathBuf {
        PathBuf::from(&self.process.identity.executable)
    }
    pub fn identity(&self) -> &crate::inspection::process::Identity {
        &self.process.identity
    }
    pub fn runtime_info(&self, dll: &Path) -> Result<crate::runtime_info::Info> {
        let mut control = Control::new(api::RUNTIME_INFO);
        self.control(dll, &mut control)?;
        let info: crate::runtime_info::Info =
            serde_json::from_str(api::text(&control.message).map_err(anyhow::Error::msg)?)?;
        info.validate().map_err(anyhow::Error::msg)?;
        Ok(info)
    }
    fn module(&self, dll: &Path) -> Result<Option<Module>> {
        let name = dll
            .file_name()
            .context("DLL needs filename")?
            .to_string_lossy();
        for module in self
            .process
            .modules()
            .context("enumerating target modules")?
        {
            if module.name.eq_ignore_ascii_case(&name) {
                ensure!(
                    path_key(Path::new(&module.path)) == path_key(dll),
                    "different DLL with same filename is already loaded: {}",
                    module.path
                );
                return Ok(Some(module));
            }
        }
        Ok(None)
    }
    fn export(&self, module: &str, symbol: &str) -> Result<usize> {
        let images: Vec<_> = self
            .process
            .modules()?
            .into_iter()
            .map(|m| Image {
                name: m.name,
                base: m.base,
                size: m.size as u32,
            })
            .collect();
        let value = pe_exports::resolve(&images, module, symbol, |at, out| {
            self.process.read(at, out).map_err(|e| e.to_string())
        })
        .map_err(anyhow::Error::msg)?;
        usize::try_from(value).context("export does not fit x86")
    }
    /// On timeout the remote allocation is intentionally retained: the target
    /// may still access it. No thread termination or automatic unload follows.
    fn call(&self, function: usize, payload: &mut [u8], direct: Option<usize>) -> Result<u32> {
        self.call_wait(function, payload, direct, 30000)
    }
    fn call_wait(
        &self,
        function: usize,
        payload: &mut [u8],
        direct: Option<usize>,
        wait_ms: u32,
    ) -> Result<u32> {
        self.process.ensure_running()?;
        let mut lease = call_lease::Lease::acquire(self.handle.0, &self.call_guard)?;
        let allocation = if direct.is_none() {
            let pointer = unsafe {
                VirtualAllocEx(
                    self.handle.0,
                    ptr::null(),
                    payload.len(),
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            };
            ensure!(
                !pointer.is_null(),
                "remote allocation failed: {}",
                std::io::Error::last_os_error()
            );
            Some(pointer)
        } else {
            None
        };
        let mut memory = Allocation {
            process: self.handle.0,
            pointer: allocation,
            release: true,
        };
        if let Some(pointer) = allocation {
            let mut count = 0;
            ensure!(
                unsafe {
                    WriteProcessMemory(
                        self.handle.0,
                        pointer,
                        payload.as_ptr().cast(),
                        payload.len(),
                        &mut count,
                    )
                } != 0
                    && count == payload.len(),
                "remote payload write failed"
            );
        }
        let argument = allocation.unwrap_or(direct.unwrap_or_default() as *mut c_void);
        // SAFETY: function is resolved in target PE32; all supported callbacks
        // have the one-argument stdcall ABI of LPTHREAD_START_ROUTINE.
        let callback = unsafe {
            std::mem::transmute::<usize, unsafe extern "system" fn(*mut c_void) -> u32>(function)
        };
        let thread = Handle(unsafe {
            CreateRemoteThread(
                self.handle.0,
                ptr::null(),
                0,
                Some(callback),
                argument,
                0,
                ptr::null_mut(),
            )
        });
        ensure!(
            !thread.0.is_null(),
            "remote thread failed: {}",
            std::io::Error::last_os_error()
        );
        lease.started();
        if unsafe { WaitForSingleObject(thread.0, wait_ms) } != WAIT_OBJECT_0 {
            memory.release = false;
            bail!(
                "remote call did not finish; payload retained; restart target before more control/unload operations"
            );
        }
        lease.completed();
        let mut code = 0;
        ensure!(
            unsafe { GetExitCodeThread(thread.0, &mut code) } != 0,
            "cannot read thread result"
        );
        if let Some(pointer) = allocation {
            let mut count = 0;
            ensure!(
                unsafe {
                    ReadProcessMemory(
                        self.handle.0,
                        pointer,
                        payload.as_mut_ptr().cast(),
                        payload.len(),
                        &mut count,
                    )
                } != 0
                    && count == payload.len(),
                "cannot read control result"
            );
        }
        Ok(code)
    }
    pub fn load(&self, dll: &Path) -> Result<()> {
        call_lease::ensure_clear(&self.call_guard)?;
        if self.module(dll)?.is_some() {
            return Ok(());
        }
        ensure!(dll.is_absolute(), "DLL path must be absolute");
        // Inspect only the PE architecture; no DLL code is loaded in this controller.
        let bytes = std::fs::read(dll)
            .with_context(|| format!("reading selected DLL {}", dll.display()))?;
        ensure!(bytes.len() >= 64 && &bytes[..2] == b"MZ", "DLL missing MZ");
        let pe = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
        let header = bytes
            .get(pe..pe.checked_add(6).context("PE overflow")?)
            .context("truncated DLL")?;
        ensure!(
            &header[..4] == b"PE\0\0" && header[4..6] == 0x14cu16.to_le_bytes(),
            "DLL must be PE x86"
        );
        let mut wide: Vec<u8> = dll
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let function = self
            .export("kernel32.dll", "LoadLibraryW")
            .context("resolving target LoadLibraryW")?;
        let result = self.call(function, &mut wide, None)?;
        ensure!(
            result != 0 && self.module(dll)?.is_some(),
            "LoadLibraryW failed in target; check DLL dependencies"
        );
        Ok(())
    }
    pub fn control(&self, dll: &Path, block: &mut Control) -> Result<()> {
        call_lease::ensure_clear(&self.call_guard)?;
        ensure!(self.module(dll)?.is_some(), "DLL not loaded");
        let name = dll.file_name().unwrap().to_string_lossy();
        let function = self.export(&name, "client_control")?;
        // Control has no pointers, references or invalid bit patterns. Its explicit
        // offsets/size are tested on both hosts; caller owns the full byte buffer.
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(
                (block as *mut Control).cast::<u8>(),
                size_of::<Control>(),
            )
        };
        let code = self.call(function, bytes, None)?;
        ensure!(
            code == 0 && block.result == 0,
            "DLL command failed: {}",
            api::text(&block.message).unwrap_or("invalid diagnostic")
        );
        Ok(())
    }
    pub fn unload(&self, dll: &Path) -> Result<()> {
        call_lease::ensure_clear(&self.call_guard)?;
        let Some(module) = self.module(dll)? else {
            return Ok(());
        };
        let mut stop = Control::new(api::STOP);
        let stopped = self.control(dll, &mut stop);
        // A worker failure may report an error despite a successful join.
        ensure!(
            stop.status == 2,
            "stop did not confirm worker joined: {stopped:?}"
        );
        self.lifecycle(dll)?
            .require_unload()
            .map_err(anyhow::Error::msg)?;
        let function = self.export("kernel32.dll", "FreeLibrary")?;
        ensure!(
            self.call(function, &mut [], Some(module.base as usize))? != 0,
            "remote FreeLibrary failed"
        );
        ensure!(
            self.module(dll)?.is_none(),
            "DLL still has other owners; refusing repeated FreeLibrary"
        );
        Ok(())
    }
    pub fn lifecycle(&self, dll: &Path) -> Result<crate::lifecycle::State> {
        let mut block = Control::new(api::LIFECYCLE_STATUS);
        self.control(dll, &mut block)
            .context("lifecycle evidence unavailable; DLL retained")?;
        serde_json::from_str(api::text(&block.message).map_err(anyhow::Error::msg)?)
            .context("invalid lifecycle evidence; DLL retained")
    }
    pub fn engine_control(&self, dll: &Path, block: &mut api::EngineControl) -> Result<()> {
        call_lease::ensure_clear(&self.call_guard)?;
        ensure!(self.module(dll)?.is_some(), "DLL not loaded");
        let name = dll.file_name().unwrap().to_string_lossy();
        let function = self.export(&name, "client_engine_control")?;
        // SAFETY: EngineControl is pointer-free and has no implicit padding.
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(
                (block as *mut api::EngineControl).cast::<u8>(),
                size_of::<api::EngineControl>(),
            )
        };
        let code = self.call(function, bytes, None)?;
        ensure!(
            code == 0 && block.result == 0,
            "engine command failed: {}",
            api::text(&block.message).unwrap_or("invalid diagnostic")
        );
        Ok(())
    }
}
struct Allocation {
    process: HANDLE,
    pointer: Option<*mut c_void>,
    release: bool,
}
impl Drop for Allocation {
    fn drop(&mut self) {
        if self.release
            && let Some(pointer) = self.pointer
        {
            unsafe {
                VirtualFreeEx(self.process, pointer, 0, MEM_RELEASE);
            }
        }
    }
}

#[cfg(all(test, target_arch = "x86"))]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Child, Command, Stdio},
        time::Duration,
    };
    struct Host(Child);
    impl Drop for Host {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[test]
    fn unresolved_call_stays_blocked_across_controllers_and_launchers_until_target_exit() {
        let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
            .canonicalize()
            .unwrap();
        let mut host = Host(
            Command::new(dll.with_file_name("engine_fixture.exe"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut output = BufReader::new(host.0.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let info: serde_json::Value = serde_json::from_str(&line).unwrap();
        let c = Controller::open(host.0.id()).unwrap();
        c.load(&dll).unwrap();
        let mut attach = api::EngineControl::new(api::UI_ATTACH);
        attach.window = info["window"].as_u64().unwrap() as u32;
        c.engine_control(&dll, &mut attach).unwrap();
        // Confirm normal calls leave no guard behind before inducing a delay.
        call_lease::ensure_clear(&c.call_guard).unwrap();
        writeln!(host.0.stdin.as_mut().unwrap(), "block").unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "blocked");
        let function = c.export("client_dll.dll", "client_engine_control").unwrap();
        let mut probe = api::EngineControl::new(api::UI_PROBE);
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(
                (&mut probe as *mut api::EngineControl).cast::<u8>(),
                size_of_val(&probe),
            )
        };
        let error = c
            .call_wait(function, bytes, None, 10)
            .unwrap_err()
            .to_string();
        assert!(error.contains("payload retained"), "{error}");
        assert!(c.control(&dll, &mut Control::new(api::STATUS)).is_err());
        assert!(c.unload(&dll).is_err());
        let guard = c.call_guard.clone();
        drop(c);
        assert!(Controller::open(host.0.id()).is_err());
        let launcher = Command::new(dll.with_file_name("mp_launcher.exe"))
            .args(["--pid", &host.0.id().to_string(), "--dll"])
            .arg(&dll)
            .args(["--fixture", "status"])
            .output()
            .unwrap();
        assert!(!launcher.status.success());
        assert!(String::from_utf8_lossy(&launcher.stderr).contains("remote call unresolved"));
        // Completion after timeout does not retroactively prove a control result.
        std::thread::sleep(Duration::from_millis(2400));
        assert!(Controller::open(host.0.id()).is_err());
        writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
        assert!(host.0.wait().unwrap().success());
        call_lease::ensure_clear(&guard).unwrap();
    }
}
