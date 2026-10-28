//! A private message-pump host for dispatcher lifecycle tests. No game assets.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use std::{
        io::{BufRead, Write},
        ptr,
    };
    use windows_sys::Win32::{
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{GetCurrentProcessId, GetCurrentThreadId},
        },
        UI::WindowsAndMessaging::*,
    };
    let name: Vec<u16> = "AcrossAmericaMP.OwnFixture"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let ansi = std::env::args().any(|a| a == "--ansi");
    let class = WNDCLASSW {
        lpfnWndProc: Some(procedure_w),
        hInstance: instance,
        lpszClassName: name.as_ptr(),
        ..Default::default()
    };
    let class_a = WNDCLASSA {
        lpfnWndProc: Some(procedure_a),
        hInstance: instance,
        lpszClassName: c"AcrossAmericaMP.OwnFixture".as_ptr().cast(),
        ..Default::default()
    };
    let registered = unsafe {
        if ansi {
            RegisterClassA(&class_a)
        } else {
            RegisterClassW(&class)
        }
    };
    ensure!(registered != 0, "cannot register fixture class");
    let window = unsafe {
        if ansi {
            CreateWindowExA(
                0,
                class_a.lpszClassName,
                class_a.lpszClassName,
                0,
                0,
                0,
                32,
                32,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            )
        } else {
            CreateWindowExW(
                0,
                name.as_ptr(),
                name.as_ptr(),
                0,
                0,
                0,
                32,
                32,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            )
        }
    };
    ensure!(!window.is_null(), "cannot create fixture window");
    println!(
        "{}",
        serde_json::json!({"pid":unsafe{GetCurrentProcessId()},"window":window as usize,"thread":unsafe{GetCurrentThreadId()}})
    );
    std::io::stdout().flush()?;
    let address = window as usize;
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if line == "title" {
                let title: Vec<u16> = "Caminhao MP".encode_utf16().chain(Some(0)).collect();
                let mut received = [0u16; 32];
                unsafe {
                    SetWindowTextW(address as _, title.as_ptr());
                    GetWindowTextW(address as _, received.as_mut_ptr(), received.len() as i32);
                }
                let end = received
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(received.len());
                println!("title={}", String::from_utf16_lossy(&received[..end]));
                let _ = std::io::stdout().flush();
                continue;
            }
            let message = match line.as_str() {
                "quit" => WM_CLOSE,
                "ping" => WM_APP + 100,
                "block" => WM_APP + 101,
                _ => continue,
            };
            unsafe {
                PostMessageW(address as _, message, 0, 0);
            }
            if line == "quit" {
                break;
            }
        }
    });
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        ensure!(result != -1, "fixture message loop failed");
        if result == 0 {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            if ansi {
                DispatchMessageA(&message);
            } else {
                DispatchMessageW(&message);
            }
        }
    }
    Ok(())
}
#[cfg(windows)]
unsafe extern "system" fn procedure_w(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    unsafe { procedure(window, message, wparam, lparam, false) }
}
#[cfg(windows)]
unsafe extern "system" fn procedure_a(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    unsafe { procedure(window, message, wparam, lparam, true) }
}
#[cfg(windows)]
unsafe fn procedure(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    ansi: bool,
) -> isize {
    use std::io::Write;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    match message {
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        m if m == WM_APP + 100 => {
            println!("pong");
            let _ = std::io::stdout().flush();
            0
        }
        m if m == WM_APP + 101 => {
            println!("blocked");
            let _ = std::io::stdout().flush();
            std::thread::sleep(std::time::Duration::from_millis(2200));
            0
        }
        _ => unsafe {
            if ansi {
                DefWindowProcA(window, message, wparam, lparam)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        },
    }
}
#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("engine_fixture requires Windows");
    std::process::ExitCode::FAILURE
}
