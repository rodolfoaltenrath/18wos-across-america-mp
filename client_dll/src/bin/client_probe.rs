//! A Rust host that loads the actual DLL in its OWN process for LAN tests.
//! It does not attach to or modify the game.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::PathBuf, time::Duration};
    use windows_sys::Win32::{
        Foundation::FreeLibrary,
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    };
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or(std::env::current_exe()?.with_file_name("client_dll.dll"));
    let seconds: u64 = args
        .next()
        .map(|s| s.to_str().unwrap_or("").parse())
        .transpose()?
        .unwrap_or(10);
    let path = path.canonicalize()?;
    let wide: Vec<u16> = OsStr::new(&path).encode_wide().chain(Some(0)).collect();
    // SAFETY: path is a valid NUL-terminated Windows string. The user selects
    // this project's DLL, and all symbols below use its documented ABI.
    let module = unsafe { LoadLibraryW(wide.as_ptr()) };
    if module.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    macro_rules! symbol {
        ($name:literal, $signature:ty) => {{
            let address = unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) }
                .ok_or(concat!("missing DLL export: ", $name))?;
            unsafe {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, $signature>(address)
            }
        }};
    }
    let start = symbol!("client_start", extern "system" fn() -> i32);
    let stop = symbol!("client_stop", extern "system" fn() -> i32);
    let status = symbol!("client_status", extern "system" fn() -> u32);
    let sent = symbol!("client_packets_sent", extern "system" fn() -> u64);
    let received = symbol!("client_packets_received", extern "system" fn() -> u64);
    let remote = symbol!(
        "client_get_remote",
        unsafe extern "system" fn(u32, *mut client_dll::RemoteState) -> i32
    );
    if start() != 0 {
        unsafe {
            FreeLibrary(module);
        }
        return Err("client_start failed; check AA_MP_* configuration".into());
    }
    println!(
        "Loaded {} for {seconds}s; mode={}",
        path.display(),
        std::env::var("AA_MP_MODE").unwrap_or_else(|_| "simulated".into())
    );
    for _ in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        println!(
            "status={} sent={} received={}",
            status(),
            sent(),
            received()
        );
        let mut state = client_dll::RemoteState::default();
        for index in 0..64 {
            // SAFETY: state is aligned writable storage of the documented type.
            if unsafe { remote(index, &mut state) } == 0 {
                break;
            }
            println!(
                "  player={} seq={} xyz=({:.2}, {:.2}, {:.2})",
                state.player_id, state.sequence, state.x, state.y, state.z
            );
        }
        if status() == 3 {
            break;
        }
    }
    let stopped = stop(); // Join the worker BEFORE unloading any of its code.
    unsafe {
        FreeLibrary(module);
    }
    if stopped != 0 {
        return Err("DLL worker stopped with an error".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("client_probe requires Windows");
}
