//! A private message-pump host for dispatcher lifecycle tests. No game assets.
#[cfg(all(windows, target_arch = "x86"))]
mod fixture_actor;
#[cfg(all(windows, target_arch = "x86"))]
mod fixture_draw;
#[cfg(all(windows, target_arch = "x86"))]
mod fixture_render;
#[cfg(windows)]
static PEEK_SLOT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(windows)]
static PEEK_ADDRESS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
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
            Memory::{
                MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_READONLY,
                PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect, VirtualQuery,
            },
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
    let peek = std::env::args().any(|a| a == "--peek");
    #[cfg(target_arch = "x86")]
    let render_enabled = std::env::args().any(|a| a == "--render");
    #[cfg(target_arch = "x86")]
    let mut renderer = render_enabled.then(fixture_render::Fixture::new);
    #[cfg(target_arch = "x86")]
    let render_descriptors = renderer
        .as_ref()
        .map_or([0; 6], fixture_render::Fixture::descriptors);
    #[cfg(not(target_arch = "x86"))]
    let render_descriptors = [0u32; 6];
    #[cfg(target_arch = "x86")]
    let draw_enabled = std::env::args().any(|a| a == "--draw");
    #[cfg(target_arch = "x86")]
    let mut drawer = draw_enabled.then(fixture_draw::Fixture::new);
    #[cfg(target_arch = "x86")]
    let (draw_descriptors, draw_car) = drawer
        .as_ref()
        .map_or(([0; 6], 0), |d| (d.descriptors(), d.car()));
    #[cfg(not(target_arch = "x86"))]
    let (draw_descriptors, draw_car) = ([0u32; 6], 0u32);
    #[cfg(target_arch = "x86")]
    let transition_descriptors = drawer
        .as_ref()
        .map_or([0; 6], fixture_draw::Fixture::transition_descriptors);
    #[cfg(not(target_arch = "x86"))]
    let transition_descriptors = [0u32; 6];
    #[cfg(target_arch = "x86")]
    let block_descriptors = drawer
        .as_ref()
        .map_or([0; 6], fixture_draw::Fixture::block_descriptors);
    #[cfg(not(target_arch = "x86"))]
    let block_descriptors = [0u32; 6];
    #[cfg(target_arch="x86")]
    let command_descriptors=drawer.as_ref().map_or([0;6],fixture_draw::Fixture::command_descriptors);
    #[cfg(not(target_arch="x86"))]
    let command_descriptors=[0u32;6];
    #[cfg(target_arch = "x86")]
    let visual_descriptors = fixture_draw::Fixture::visual_descriptors();
    #[cfg(not(target_arch = "x86"))]
    let visual_descriptors = [0u32; 6];
    #[cfg(target_arch = "x86")]
    let actor = fixture_actor::Fixture::new();
    #[cfg(target_arch = "x86")]
    let bindings = actor.bindings();
    #[cfg(target_arch = "x86")]
    let item_descriptors = actor.item_descriptors();
    #[cfg(not(target_arch = "x86"))]
    let bindings = [0u32; 6];
    #[cfg(not(target_arch = "x86"))]
    let item_descriptors = [0u32; 2];
    #[cfg(target_arch = "x86")]
    let (model_definitions, model_setter) = (actor.model_definitions(), actor.model_setter());
    #[cfg(not(target_arch = "x86"))]
    let (model_definitions, model_setter) = ([0u32; 9], 0u32);
    #[cfg(target_arch = "x86")]
    let info_descriptors = actor.info_descriptors();
    #[cfg(not(target_arch = "x86"))]
    let info_descriptors = [0u32; 3];
    #[cfg(target_arch = "x86")]
    let instance_descriptors = actor.instance_descriptors();
    #[cfg(not(target_arch = "x86"))]
    let instance_descriptors = [0u32; 6];
    PEEK_SLOT.store(
        PeekMessageA as *const () as usize,
        std::sync::atomic::Ordering::Release,
    );
    let peek_slot = if peek {
        let page =
            unsafe { VirtualAlloc(ptr::null(), 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        ensure!(!page.is_null(), "cannot allocate fixture import page");
        unsafe {
            page.cast::<usize>()
                .write(PeekMessageA as *const () as usize);
        }
        let mut old = 0;
        ensure!(
            unsafe { VirtualProtect(page, 4096, PAGE_READONLY, &mut old) } != 0,
            "cannot protect fixture import page"
        );
        page as usize
    } else {
        &PEEK_SLOT as *const _ as usize
    };
    PEEK_ADDRESS.store(peek_slot, std::sync::atomic::Ordering::Release);
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
        serde_json::json!({"pid":unsafe{GetCurrentProcessId()},"window":window as usize,"thread":unsafe{GetCurrentThreadId()}, "peek_slot": peek_slot, "factory": bindings, "item_descriptors": item_descriptors, "model_definitions": model_definitions, "model_setter": model_setter,"info_descriptors":info_descriptors,"instance_descriptors":instance_descriptors,"render_descriptors":render_descriptors,"draw_descriptors":draw_descriptors,"draw_car":draw_car,"visual_descriptors":visual_descriptors,"transition_descriptors":transition_descriptors,"block_descriptors":block_descriptors,"command_descriptors":command_descriptors})
    );
    std::io::stdout().flush()?;
    let address = window as usize;
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            #[cfg(target_arch = "x86")]
            if draw_enabled && fixture_draw::command(&line) {
                let _ = std::io::stdout().flush();
                continue;
            }
            #[cfg(target_arch = "x86")]
            if render_enabled && fixture_render::command(&line) {
                let _ = std::io::stdout().flush();
                continue;
            }
            #[cfg(target_arch = "x86")]
            if matches!(
                line.as_str(),
                "item-fail-fix"
                    | "item-fail-create"
                    | "model-fail-set"
                    | "model-no-write"
                    | "model-side-effect"
                    | "info-cold"
                    | "info-null"
                    | "info-repeat-growth"
                    | "info-normal"
                    | "instance-null"
                    | "instance-bad-header"
                    | "instance-cold"
                    | "instance-repeat-growth"
                    | "instance-normal"
            ) {
                fixture_actor::fail_item_step(&line);
                println!("armed");
                continue;
            }
            #[cfg(target_arch = "x86")]
            if line == "actor-stats" {
                println!("{}", fixture_actor::stats());
                let _ = std::io::stdout().flush();
                continue;
            }
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
                "nested-pump" => WM_APP + 102,
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
        #[cfg(target_arch = "x86")]
        if let Some(drawer) = drawer.as_mut() {
            drawer.frame();
        }
        #[cfg(target_arch = "x86")]
        if let Some(renderer) = renderer.as_mut() {
            renderer.frame();
        }
        #[cfg(target_arch = "x86")]
        if let Some(drawer) = drawer.as_mut() {
            drawer.consume();
        }
        if peek {
            let function: unsafe extern "system" fn(
                *mut MSG,
                windows_sys::Win32::Foundation::HWND,
                u32,
                u32,
                u32,
            ) -> i32 = unsafe {
                let slot = std::sync::atomic::AtomicUsize::from_ptr(peek_slot as *mut usize);
                std::mem::transmute(slot.load(std::sync::atomic::Ordering::Acquire))
            };
            if unsafe { function(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } == 0 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
            if message.message == WM_QUIT {
                break;
            }
        } else {
            let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
            ensure!(result != -1, "fixture message loop failed");
            if result == 0 {
                break;
            }
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
    if peek {
        let mut region: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        ensure!(
            unsafe {
                VirtualQuery(
                    peek_slot as *const _,
                    &mut region,
                    std::mem::size_of_val(&region),
                )
            } != 0
                && region.Protect == PAGE_READONLY,
            "observer did not restore the import page protection"
        );
        unsafe {
            VirtualFree(peek_slot as *mut _, 0, MEM_RELEASE);
        }
    }
    #[cfg(target_arch = "x86")]
    actor.assert_clean();
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
        m if m == WM_APP + 102 => {
            println!("nested");
            let _ = std::io::stdout().flush();
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_millis(2200) {
                let mut msg = MSG::default();
                let function: unsafe extern "system" fn(
                    *mut MSG,
                    windows_sys::Win32::Foundation::HWND,
                    u32,
                    u32,
                    u32,
                ) -> i32 = unsafe {
                    let slot = std::sync::atomic::AtomicUsize::from_ptr(
                        PEEK_ADDRESS.load(std::sync::atomic::Ordering::Acquire) as *mut usize,
                    );
                    std::mem::transmute(slot.load(std::sync::atomic::Ordering::Acquire))
                };
                unsafe {
                    function(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
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
