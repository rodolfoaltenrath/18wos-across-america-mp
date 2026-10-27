use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    path::PathBuf,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_BAD_LENGTH, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, FILETIME,
        HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE,
    },
    System::{
        Diagnostics::{Debug::ReadProcessMemory, ToolHelp::*},
        Memory::*,
        Threading::{
            GetExitCodeProcess, GetProcessTimes, IsWow64Process2, OpenProcess,
            PROCESS_QUERY_INFORMATION, PROCESS_VM_READ, QueryFullProcessImageNameW,
        },
    },
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a valid process/snapshot handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    pub pid: u32,
    pub created: u64,
    pub executable: String,
    pub pointer_bytes: u8,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub base: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MemoryScope {
    All,
    Private,
    Image,
    Mapped,
}

impl MemoryScope {
    pub fn accepts(self, kind: u32) -> bool {
        match self {
            Self::All => true,
            Self::Private => kind == MEM_PRIVATE,
            Self::Image => kind == MEM_IMAGE,
            Self::Mapped => kind == MEM_MAPPED,
        }
    }
}

pub struct Region {
    pub start: u64,
    pub end: u64,
    pub kind: u32,
    pub protection: u32,
}

impl Region {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            MEM_PRIVATE => "PRIVATE",
            MEM_IMAGE => "IMAGE",
            MEM_MAPPED => "MAPPED",
            _ => "OTHER",
        }
    }
}

pub struct Process {
    handle: Handle,
    pub identity: Identity,
}

fn wide(text: &[u16]) -> String {
    String::from_utf16_lossy(&text[..text.iter().position(|v| *v == 0).unwrap_or(text.len())])
}

fn snapshot(flags: u32, pid: u32) -> Result<Handle> {
    for _ in 0..16 {
        // SAFETY: numeric arguments; the returned handle is owned below.
        let handle = unsafe { CreateToolhelp32Snapshot(flags, pid) };
        if handle != INVALID_HANDLE_VALUE {
            return Ok(Handle(handle));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_BAD_LENGTH as i32) {
            return Err(error.into());
        }
    }
    bail!("module list kept changing; retry once the game has finished loading")
}

pub fn processes(all: bool) -> Result<Vec<(u32, String)>> {
    let snapshot = snapshot(TH32CS_SNAPPROCESS, 0)?;
    // SAFETY: PROCESSENTRY32W is plain Win32 data, initialized with its size.
    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut output = Vec::new();
    let mut ok = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while ok != 0 {
        let name = wide(&entry.szExeFile);
        if all || matches!(name.to_ascii_lowercase().as_str(), "aa.exe" | "prism3d.exe") {
            output.push((entry.th32ProcessID, name));
        }
        ok = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    let error = std::io::Error::last_os_error();
    ensure!(
        error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32),
        "process enumeration failed: {error}"
    );
    output.sort_unstable();
    Ok(output)
}

impl Process {
    pub fn open(pid: u32) -> Result<Self> {
        // SAFETY: only query/read rights are requested; no writes or injection.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
        ensure!(
            !raw.is_null(),
            "cannot open PID {pid}: {}",
            std::io::Error::last_os_error()
        );
        let handle = Handle(raw);
        let (mut created, mut exited, mut kernel, mut user): (
            FILETIME,
            FILETIME,
            FILETIME,
            FILETIME,
        ) = unsafe { zeroed() };
        ensure!(
            unsafe { GetProcessTimes(raw, &mut created, &mut exited, &mut kernel, &mut user) } != 0,
            "cannot get process creation time: {}",
            std::io::Error::last_os_error()
        );
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        ensure!(
            unsafe { QueryFullProcessImageNameW(raw, 0, path.as_mut_ptr(), &mut length) } != 0,
            "cannot get executable path: {}",
            std::io::Error::last_os_error()
        );
        path.truncate(length as usize);
        let (mut machine, mut native) = (0u16, 0u16);
        ensure!(
            unsafe { IsWow64Process2(raw, &mut machine, &mut native) } != 0,
            "cannot determine target architecture: {}",
            std::io::Error::last_os_error()
        );
        let machine = if machine == 0 { native } else { machine };
        let pointer_bytes = match machine {
            0x014c => 4,
            0x8664 | 0xaa64 => 8,
            _ => bail!("unsupported target machine: 0x{machine:04x}"),
        };
        ensure!(
            size_of::<usize>() >= pointer_bytes as usize,
            "use the 64-bit memory_tool to inspect this process"
        );
        let identity = Identity {
            pid,
            created: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
            executable: String::from_utf16_lossy(&path),
            pointer_bytes,
        };
        let process = Self { handle, identity };
        process.ensure_running()?;
        Ok(process)
    }

    pub fn ensure_running(&self) -> Result<()> {
        let mut code = 0;
        // SAFETY: handle is live and code is valid writable storage.
        ensure!(
            unsafe { GetExitCodeProcess(self.handle.0, &mut code) } != 0,
            "cannot query process state: {}",
            std::io::Error::last_os_error()
        );
        ensure!(
            code == STILL_ACTIVE as u32,
            "target process has exited; start a fresh session"
        );
        Ok(())
    }

    pub fn same_session(&self, expected: &Identity) -> Result<()> {
        ensure!(
            &self.identity == expected,
            "snapshot belongs to a different process/session; start a new scan after restarting the game"
        );
        Ok(())
    }

    pub fn max_address(&self) -> u64 {
        if self.identity.pointer_bytes == 4 {
            u32::MAX.into()
        } else {
            usize::MAX as u64
        }
    }

    pub fn modules(&self) -> Result<Vec<Module>> {
        let snapshot = snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, self.identity.pid)?;
        let mut entry: MODULEENTRY32W = unsafe { zeroed() };
        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        let mut output = Vec::new();
        let mut ok = unsafe { Module32FirstW(snapshot.0, &mut entry) };
        while ok != 0 {
            let base = entry.modBaseAddr as usize as u64;
            if base <= self.max_address() {
                output.push(Module {
                    name: wide(&entry.szModule),
                    base,
                    size: entry.modBaseSize.into(),
                });
            }
            ok = unsafe { Module32NextW(snapshot.0, &mut entry) };
        }
        let error = std::io::Error::last_os_error();
        ensure!(
            error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32),
            "module enumeration failed: {error}"
        );
        output.sort_by_key(|module| module.base);
        output.dedup_by_key(|module| module.base);
        Ok(output)
    }

    pub fn read(&self, address: u64, bytes: &mut [u8]) -> Result<()> {
        let address =
            usize::try_from(address).context("address does not fit scanner architecture")?;
        ensure!(
            address != 0 && (address as u64) <= self.max_address(),
            "address outside target process"
        );
        let mut count = 0;
        // SAFETY: bytes is writable for its full length. Windows checks the
        // remote address, which is never dereferenced as a Rust reference.
        let ok = unsafe {
            ReadProcessMemory(
                self.handle.0,
                address as *const c_void,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                &mut count,
            )
        };
        ensure!(
            ok != 0 && count == bytes.len(),
            "read at 0x{address:x} failed: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }

    pub fn pointer(&self, address: u64) -> Result<u64> {
        let mut bytes = [0u8; 8];
        self.read(address, &mut bytes[..self.identity.pointer_bytes as usize])?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Enumerate regions without copying their contents. An end of None scans
    /// until VirtualQueryEx reports the end of the target address space.
    pub fn regions(
        &self,
        start: u64,
        end: Option<u64>,
        all_readable: bool,
    ) -> Result<Vec<(u64, u64)>> {
        Ok(self
            .region_details(start, end, all_readable)?
            .into_iter()
            .map(|region| (region.start, region.end))
            .collect())
    }

    pub fn region_details(
        &self,
        start: u64,
        end: Option<u64>,
        all_readable: bool,
    ) -> Result<Vec<Region>> {
        let mut address = start;
        let mut output = Vec::new();
        while address <= self.max_address() && end.is_none_or(|end| address < end) {
            let mut info: MEMORY_BASIC_INFORMATION = unsafe { zeroed() };
            let length = unsafe {
                VirtualQueryEx(
                    self.handle.0,
                    address as usize as *const c_void,
                    &mut info,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if length == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                    break;
                }
                return Err(error.into());
            }
            let base = info.BaseAddress as usize as u64;
            let next = base
                .checked_add(info.RegionSize as u64)
                .context("region address overflow")?;
            ensure!(next > address, "VirtualQueryEx did not advance");
            let protection = info.Protect & 0xff;
            let readable = matches!(
                protection,
                PAGE_READONLY
                    | PAGE_READWRITE
                    | PAGE_WRITECOPY
                    | PAGE_EXECUTE_READ
                    | PAGE_EXECUTE_READWRITE
                    | PAGE_EXECUTE_WRITECOPY
            );
            let writable = matches!(
                protection,
                PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
            );
            if info.State == MEM_COMMIT
                && info.Protect & PAGE_GUARD == 0
                && readable
                && (all_readable || writable)
            {
                let from = base.max(start);
                let to = next.min(end.unwrap_or(u64::MAX));
                if from < to {
                    output.push(Region {
                        start: from,
                        end: to,
                        kind: info.Type,
                        protection: info.Protect,
                    });
                }
            }
            address = next;
        }
        Ok(output)
    }
}

pub fn executable_name(identity: &Identity) -> String {
    PathBuf::from(&identity.executable)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
