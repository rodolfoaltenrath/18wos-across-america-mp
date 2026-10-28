//! Explicit x86 process control. Allocates data only; resolves code in target images.
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
            PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
            PROCESS_VM_READ, PROCESS_VM_WRITE, ReleaseMutex, WaitForSingleObject,
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
            Ok((process, handle))
        })();
        match result {
            Ok((process, handle)) => Ok(Self {
                process,
                handle,
                lock,
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
        self.process.ensure_running()?;
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
        if unsafe { WaitForSingleObject(thread.0, 30000) } != WAIT_OBJECT_0 {
            memory.release = false;
            bail!(
                "remote call did not finish; payload retained; restart target before more control/unload operations"
            );
        }
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
        let mut phase = api::EngineControl::new(api::UI_PUMP_STATUS);
        if self.engine_control(dll, &mut phase).is_ok() {
            ensure!(
                phase.phase == 0,
                "stop completed and pump import restored; observer pins DLL until process exit"
            );
        }
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
    pub fn engine_control(&self, dll: &Path, block: &mut api::EngineControl) -> Result<()> {
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
