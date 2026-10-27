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
