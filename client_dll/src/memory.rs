use crate::SampleError;
use crate::addresses::Address;
use multiplayer_server::protocol::Position;

#[cfg(windows)]
fn read_bytes(address: usize, bytes: &mut [u8]) -> Result<(), String> {
    use std::ffi::c_void;
    use windows_sys::Win32::System::{
        Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess,
    };
    let mut count = 0;
    // SAFETY: the destination is valid; Windows validates the source address.
    let success = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            &mut count,
        )
    };
    if success == 0 || count != bytes.len() {
        return Err(format!(
            "cannot read at 0x{address:x}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn resolve(address: &Address) -> Result<usize, String> {
    address
        .resolve(
            usize::MAX as u64,
            |module| {
                let wide: Vec<u16> = module.encode_utf16().chain(Some(0)).collect();
                // SAFETY: wide is a NUL-terminated module filename. No module is loaded.
                let base = unsafe {
                    windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr())
                };
                if base.is_null() {
                    return Err(format!("module {module} is not loaded"));
                }
                Ok(base as usize as u64)
            },
            |at| {
                let at = usize::try_from(at).map_err(|_| "address does not fit this DLL")?;
                let mut bytes = [0; std::mem::size_of::<usize>()];
                read_bytes(at, &mut bytes)?;
                Ok(usize::from_le_bytes(bytes) as u64)
            },
        )
        .and_then(|value| {
            usize::try_from(value).map_err(|_| "address does not fit this DLL".into())
        })
}

#[cfg(windows)]
fn read_f32(address: usize) -> Result<f32, String> {
    let mut bytes = [0; 4];
    read_bytes(address, &mut bytes)?;
    let value = f32::from_le_bytes(bytes);
    if !value.is_finite() {
        return Err(format!("invalid coordinate at 0x{address:x}"));
    }
    Ok(value)
}

#[cfg(windows)]
pub fn position(addresses: &[Address; 3]) -> Result<Position, String> {
    Ok(Position {
        x: read_f32(resolve(&addresses[0])?)?,
        y: read_f32(resolve(&addresses[1])?)?,
        z: read_f32(resolve(&addresses[2])?)?,
    })
}

#[cfg(not(windows))]
pub fn position(_addresses: &[Address; 3]) -> Result<Position, String> {
    Err("game memory reading is available only on Windows".into())
}

#[cfg(windows)]
fn game_base(verified_base: &mut usize) -> Result<usize, SampleError> {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
    let name: Vec<u16> = "game.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: NUL-terminated filename; this only queries an already loaded module.
    let module = unsafe { GetModuleHandleW(name.as_ptr()) };
    if module.is_null() {
        *verified_base = 0;
        return Err(SampleError::Unavailable("waiting for game.dll".into()));
    }
    if module as usize != *verified_base {
        let mut filename = vec![0u16; 32768];
        let len =
            unsafe { GetModuleFileNameW(module, filename.as_mut_ptr(), filename.len() as u32) }
                as usize;
        if len == 0 || len >= filename.len() {
            return Err(SampleError::Fatal(
                "cannot identify loaded game.dll path".into(),
            ));
        }
        use std::os::windows::ffi::OsStringExt;
        let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&filename[..len]));
        crate::profile::verify_module(&path).map_err(SampleError::Fatal)?;
        *verified_base = module as usize;
    }
    Ok(module as usize)
}

#[cfg(windows)]
pub fn game_position(verified_base: &mut usize) -> Result<Position, SampleError> {
    game_base(verified_base)?;
    let address: Address = crate::profile::POSITION
        .parse()
        .map_err(SampleError::Fatal)?;
    let at = resolve(&address).map_err(SampleError::Unavailable)?;
    let mut bytes = [0; 12];
    read_bytes(at, &mut bytes).map_err(SampleError::Unavailable)?;
    // Re-resolving catches pointer-chain changes around capture, not concurrent physics writes.
    if resolve(&address).map_err(SampleError::Unavailable)? != at {
        return Err(SampleError::Unavailable(
            "vehicle changed during capture".into(),
        ));
    }
    let values: [f32; 3] =
        std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    let position = Position {
        x: values[0],
        y: values[1],
        z: values[2],
    };
    if !position.is_valid() {
        return Err(SampleError::Unavailable(
            "vehicle coordinates unavailable".into(),
        ));
    }
    Ok(position)
}

#[cfg(not(windows))]
pub fn game_position(_verified_base: &mut usize) -> Result<Position, SampleError> {
    Err(SampleError::Fatal(
        "game profile requires Windows x86".into(),
    ))
}

#[cfg(windows)]
pub fn game_pose(verified_base: &mut usize) -> Result<crate::pose_capture::Pose, SampleError> {
    let base = u32::try_from(game_base(verified_base)?)
        .map_err(|_| SampleError::Fatal("pose requires x86 address space".into()))?;
    let root = base
        .checked_add(0x160df0)
        .ok_or_else(|| SampleError::Fatal("pose root overflow".into()))?;
    let vtable = base
        .checked_add(crate::profile::CAR_VTABLE)
        .ok_or_else(|| SampleError::Fatal("pose vtable overflow".into()))?;
    crate::pose_capture::capture(
        crate::pose_capture::Layout {
            root,
            steps: &[0x14c, 0x4610, 8, 8],
            vtable,
        },
        |at, b| read_bytes(at as usize, b).map_err(|_| ()),
    )
    .map_err(SampleError::PoseUnavailable)
}
pub fn require_pose_fixture() -> Result<(), String> {
    if !cfg!(all(windows, target_arch = "x86"))
        || !std::env::current_exe()
            .ok()
            .and_then(|p| {
                p.file_name()
                    .map(|s| s.eq_ignore_ascii_case("engine_fixture.exe"))
            })
            .unwrap_or(false)
    {
        return Err("pose fixture requires our x86 engine_fixture.exe".into());
    }
    Ok(())
}
#[cfg(windows)]
pub fn fixture_pose(root: u32, vtable: u32) -> Result<crate::pose_capture::Pose, SampleError> {
    crate::pose_capture::capture(
        crate::pose_capture::Layout {
            root,
            steps: &[],
            vtable,
        },
        |at, b| read_bytes(at as usize, b).map_err(|_| ()),
    )
    .map_err(SampleError::PoseUnavailable)
}
#[cfg(not(windows))]
pub fn game_pose(_: &mut usize) -> Result<crate::pose_capture::Pose, SampleError> {
    Err(SampleError::Fatal("game pose requires Windows x86".into()))
}
#[cfg(not(windows))]
pub fn fixture_pose(_: u32, _: u32) -> Result<crate::pose_capture::Pose, SampleError> {
    Err(SampleError::Fatal(
        "fixture pose requires Windows x86".into(),
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn reads_valid_local_values_and_rejects_unmapped_address() {
        let values = [1.5f32, -2.0, 7.0];
        let addresses = std::array::from_fn(|i| {
            format!("{:x}", (&values[i] as *const f32) as usize)
                .parse()
                .unwrap()
        });
        let result = position(&addresses).unwrap();
        assert_eq!(
            result,
            Position {
                x: 1.5,
                y: -2.0,
                z: 7.0
            }
        );
        assert!(read_f32(1).is_err());
        let pointer = values.as_ptr() as usize;
        let chained: Address = format!("{:x}->4", (&pointer as *const usize) as usize)
            .parse()
            .unwrap();
        assert_eq!(read_f32(resolve(&chained).unwrap()).unwrap(), -2.0);
        let absent: Address = "nonexistent-aa-test-module.dll+1".parse().unwrap();
        assert!(resolve(&absent).is_err());
    }
}
