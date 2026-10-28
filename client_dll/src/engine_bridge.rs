//! Serial, bounded dispatch to ONE window thread. No game constructors called here.
//! A module lease protects the subclass, including timeouts. Detach restores
//! the WndProc on that thread and waits for SendMessageTimeout to confirm return.
use crate::api::{self, EngineControl};
use std::{
    collections::BTreeMap,
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HWND, LPARAM, LRESULT, RECT, WPARAM},
    System::{
        Diagnostics::Debug::RtlCaptureStackBackTrace,
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExW, GetModuleHandleW,
        },
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::WindowsAndMessaging::*,
};
static CONTROL: Mutex<()> = Mutex::new(());
static WINDOW: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static LEASE: AtomicUsize = AtomicUsize::new(0);
static THREAD: AtomicU32 = AtomicU32::new(0);
static MESSAGE: AtomicU32 = AtomicU32::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
static COMPLETED: AtomicU32 = AtomicU32::new(0);
static LAST_THREAD: AtomicU32 = AtomicU32::new(0);
static NEXT: AtomicU32 = AtomicU32::new(1);
static JOBS: Mutex<BTreeMap<u32, Arc<Job>>> = Mutex::new(BTreeMap::new());
const ACK: isize = 0x18aa;
struct Job {
    request: EngineControl,
    canceled: AtomicBool,
    result: Mutex<Option<Result<EngineControl, String>>>,
}
fn error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}
fn fill(block: &mut EngineControl) {
    #[cfg(target_arch = "x86")]
    crate::engine_phase::fill(block);
    block.window = WINDOW.load(Ordering::Acquire) as u32;
    block.window_thread = THREAD.load(Ordering::Acquire);
    block.active = u32::from(block.window != 0);
    block.completed = COMPLETED.load(Ordering::Relaxed);
    block.executed_thread = LAST_THREAD.load(Ordering::Acquire);
    block.inflight = DEPTH.load(Ordering::Acquire);
}
unsafe extern "system" fn select_window(window: HWND, data: LPARAM) -> i32 {
    // SAFETY: EnumWindows invokes synchronously with this owned stack pair.
    let best = unsafe { &mut *(data as *mut (usize, i64)) };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, &mut pid);
    }
    if pid != unsafe { GetCurrentProcessId() }
        || unsafe { IsWindowVisible(window) } == 0
        || !unsafe { GetWindow(window, GW_OWNER) }.is_null()
    {
        return 1;
    }
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetClientRect(window, &mut rect) } != 0 {
        let area = i64::from(rect.right - rect.left) * i64::from(rect.bottom - rect.top);
        if area > best.1 {
            *best = (window as usize, area);
        }
    }
    1
}
fn attach(window: u32) -> Result<(), String> {
    if WINDOW.load(Ordering::Acquire) != 0 {
        return Err("window dispatcher already attached".into());
    }
    if LEASE.load(Ordering::Acquire) != 0 {
        return Err("dispatcher lease retained after incomplete detach; reopen target".into());
    }
    let window = if window == 0 {
        let mut best = (0usize, 0i64);
        unsafe {
            EnumWindows(
                Some(select_window),
                (&mut best as *mut (usize, i64)) as isize,
            );
        }
        best.0 as HWND
    } else {
        window as usize as HWND
    };
    if window.is_null() {
        return Err("no primary window found; supply explicit window handle".into());
    }
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid != unsafe { GetCurrentProcessId() } || thread == 0 {
        return Err("window does not belong to this process".into());
    }
    let old = unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) };
    if old == 0 {
        return Err(error("cannot query original window procedure"));
    }
    let text: Vec<u16> = "AcrossAmericaMP.Dispatch.v1"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let message = unsafe { RegisterWindowMessageW(text.as_ptr()) };
    if message == 0 {
        return Err(error("cannot reserve dispatcher message"));
    }
    let mut module = ptr::null_mut();
    // SAFETY: address is in this DLL; this increments its loader reference count.
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            window_proc as *const u16,
            &mut module,
        )
    } == 0
    {
        return Err(error("cannot retain dispatcher module"));
    }
    LEASE.store(module as usize, Ordering::Release);
    ORIGINAL.store(old as usize, Ordering::Release);
    THREAD.store(thread, Ordering::Release);
    MESSAGE.store(message, Ordering::Release);
    WINDOW.store(window as usize, Ordering::Release);
    let previous =
        unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, window_proc as *const () as _) };
    if previous == 0 {
        WINDOW.store(0, Ordering::Release);
        THREAD.store(0, Ordering::Release);
        ORIGINAL.store(0, Ordering::Release);
        LEASE.store(0, Ordering::Release);
        unsafe {
            FreeLibrary(module);
        }
        return Err(error("cannot subclass window"));
    }
    // Use the actual previous procedure if another component changed it concurrently.
    ORIGINAL.store(previous as usize, Ordering::Release);
    Ok(())
}
fn execute(mut block: EngineControl) -> Result<EngineControl, String> {
    let tid = unsafe { GetCurrentThreadId() };
    if tid != THREAD.load(Ordering::Acquire) {
        return Err("dispatcher ran on the wrong thread".into());
    }
    if DEPTH.load(Ordering::Acquire) != 1 {
        return Err("reentrant window callback; retry at outer message loop".into());
    }
    LAST_THREAD.store(tid, Ordering::Release);
    match block.command {
        #[cfg(target_arch = "x86")]
        api::UI_PUMP_ATTACH | api::UI_PUMP_FIXTURE => {
            crate::engine_phase::install(
                tid,
                (block.command == api::UI_PUMP_FIXTURE).then_some(block.entity),
            )?;
        }
        #[cfg(target_arch = "x86")]
        api::UI_PUMP_REMOVE => crate::engine_phase::remove()?,
        #[cfg(target_arch = "x86")]
        api::UI_RENDER_ATTACH | api::UI_RENDER_FIXTURE => {
            crate::engine_render::install(tid, &block)?
        }
        #[cfg(target_arch = "x86")]
        api::UI_RENDER_REMOVE => crate::engine_render::remove()?,
        #[cfg(target_arch = "x86")]
        api::UI_DRAW_ATTACH | api::UI_DRAW_FIXTURE => crate::engine_draw::install(tid, &block)?,
        #[cfg(target_arch = "x86")]
        api::UI_DRAW_REMOVE => crate::engine_draw::remove()?,
        api::UI_PROBE => {}
        api::UI_TRACE => {
            let mut frames = [ptr::null_mut(); 32];
            block.frame_count = u32::from(unsafe {
                RtlCaptureStackBackTrace(
                    0,
                    frames.len() as u32,
                    frames.as_mut_ptr(),
                    ptr::null_mut(),
                )
            });
            for (out, frame) in block.frames.iter_mut().zip(frames) {
                *out = frame as usize as u32;
            }
            let process =
                crate::inspection::process::Process::open(unsafe { GetCurrentProcessId() })
                    .map_err(|e| e.to_string())?;
            block.executable_base = unsafe { GetModuleHandleW(ptr::null()) } as usize as u32;
            if let Some(executable) = process
                .modules()
                .map_err(|e| e.to_string())?
                .iter()
                .find(|m| m.base == u64::from(block.executable_base))
            {
                block.executable_frame = block
                    .frames
                    .iter()
                    .copied()
                    .find(|&a| {
                        u64::from(a) >= executable.base
                            && u64::from(a) - executable.base < executable.size
                    })
                    .map_or(0, |a| a - block.executable_base);
            }
        }
        api::UI_REGISTRY | api::UI_INSPECT_TYPE => {
            let process =
                crate::inspection::process::Process::open(unsafe { GetCurrentProcessId() })
                    .map_err(|e| e.to_string())?;
            let records =
                crate::inspection::entities::registered(&process).map_err(|e| e.to_string())?;
            block.registry_count = records.len() as u32;
            if block.command == api::UI_INSPECT_TYPE {
                let name = api::text(&block.name)?;
                let record = records
                    .iter()
                    .find(|r| r.name == name)
                    .ok_or("entity type not registered")?;
                block.object_size = record.false_size;
                block.constructor = record.false_constructor;
                block.destructor = record.false_destructor;
            }
        }
        api::UI_DETACH => {
            #[cfg(target_arch = "x86")]
            crate::engine_phase::remove()?;
            let window = WINDOW.load(Ordering::Acquire) as HWND;
            if unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) } as usize
                != window_proc as *const () as usize
            {
                return Err("another subclass owns the window; DLL retained".into());
            }
            let old = ORIGINAL.load(Ordering::Acquire);
            if unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, old as _) } == 0 {
                return Err(error("cannot restore window procedure"));
            }
        }
        _ => return Err("unknown UI task".into()),
    }
    COMPLETED.fetch_add(1, Ordering::Relaxed);
    fill(&mut block);
    Ok(block)
}
struct CallbackGuard;
impl Drop for CallbackGuard {
    fn drop(&mut self) {
        DEPTH.fetch_sub(1, Ordering::AcqRel);
    }
}
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DEPTH.fetch_add(1, Ordering::AcqRel);
    let _guard = CallbackGuard;
    if message == MESSAGE.load(Ordering::Acquire) && lparam == 0 {
        let job = JOBS
            .lock()
            .ok()
            .and_then(|mut jobs| jobs.remove(&(wparam as u32)));
        if let Some(job) = job
            && !job.canceled.load(Ordering::Acquire)
        {
            let result = panic::catch_unwind(AssertUnwindSafe(|| execute(job.request)))
                .unwrap_or_else(|_| Err("dispatcher task panicked".into()));
            if let Ok(mut slot) = job.result.lock() {
                *slot = Some(result);
            }
        }
        return ACK;
    }
    let old = ORIGINAL.load(Ordering::Acquire);
    // SAFETY: original is the WndProc token returned by paired Win32 subclass APIs.
    // CallWindowProc handles both function pointers and system procedure tokens.
    unsafe {
        CallWindowProcW(
            Some(std::mem::transmute::<
                usize,
                unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
            >(old)),
            window,
            message,
            wparam,
            lparam,
        )
    }
}
fn dispatch(block: EngineControl) -> Result<EngineControl, String> {
    let window = WINDOW.load(Ordering::Acquire) as HWND;
    if window.is_null() || unsafe { IsWindow(window) } == 0 {
        return Err(
            "dispatcher window unavailable; module retained until safe detach or process exit"
                .into(),
        );
    }
    let token = NEXT
        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
        .map_err(|_| "dispatcher sequence exhausted")?;
    let job = Arc::new(Job {
        request: block,
        canceled: AtomicBool::new(false),
        result: Mutex::new(None),
    });
    {
        let mut jobs = JOBS.lock().map_err(|_| "dispatcher queue poisoned")?;
        if jobs.len() >= 16 {
            return Err("dispatcher queue is full".into());
        }
        jobs.insert(token, job.clone());
    }
    let mut acknowledgement = 0;
    // No caller-owned pointers are sent through Windows messages. The Arc owns
    // the request even if this call times out while the target task is running.
    let delivered = unsafe {
        SendMessageTimeoutW(
            window,
            MESSAGE.load(Ordering::Acquire),
            token as usize,
            0,
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            1000,
            &mut acknowledgement,
        )
    };
    if delivered == 0 || acknowledgement != ACK as usize {
        job.canceled.store(true, Ordering::Release);
        if let Ok(mut jobs) = JOBS.lock() {
            jobs.remove(&token);
        }
        return Err(
            "UI dispatch timed out or window exited; task canceled if not started; module retained"
                .into(),
        );
    }
    job.result
        .lock()
        .map_err(|_| "dispatcher result poisoned")?
        .take()
        .ok_or("dispatcher result unavailable")?
}
fn detach_locked() -> Result<(), String> {
    if LEASE.load(Ordering::Acquire) == 0 {
        return Ok(());
    }
    dispatch(EngineControl::new(api::UI_DETACH))?;
    // A successful synchronous SendMessage return confirms our subclass callback
    // has returned into User32; only now can we clear its original-procedure token.
    if DEPTH.load(Ordering::Acquire) != 0 {
        return Err("callback still active; DLL retained".into());
    }
    WINDOW.store(0, Ordering::Release);
    THREAD.store(0, Ordering::Release);
    ORIGINAL.store(0, Ordering::Release);
    JOBS.lock()
        .map_err(|_| "dispatcher queue poisoned")?
        .clear();
    let module = LEASE.swap(0, Ordering::AcqRel);
    unsafe {
        FreeLibrary(module as *mut c_void);
    }
    Ok(())
}
pub fn detach() -> Result<(), String> {
    let _control = CONTROL.lock().map_err(|_| "dispatcher control poisoned")?;
    detach_locked()
}
#[cfg(target_arch = "x86")]
pub(super) fn window_depth() -> u32 {
    DEPTH.load(Ordering::Acquire)
}
pub fn control(block: &mut EngineControl) -> Result<(), String> {
    let _control = CONTROL.lock().map_err(|_| "dispatcher control poisoned")?;
    let result = match block.command {
        #[cfg(target_arch = "x86")]
        api::PHASE_PROBE
        | api::PRIVATE_CYCLE
        | api::PRIVATE_FIXTURE
        | api::PRIVATE_ITEM
        | api::PRIVATE_ITEM_FIXTURE
        | api::PRIVATE_MODEL
        | api::PRIVATE_MODEL_FIXTURE
        | api::PRIVATE_INFO
        | api::PRIVATE_INFO_FIXTURE
        | api::PRIVATE_INSTANCE
        | api::PRIVATE_INSTANCE_FIXTURE
        | api::VISUAL_SHOW
        | api::VISUAL_SHOW_FIXTURE
        | api::VISUAL_RELEASE => {
            crate::engine_phase::dispatch(*block, WINDOW.load(Ordering::Acquire) as HWND).map(
                |response| {
                    *block = response;
                },
            )
        }
        api::UI_ATTACH => attach(block.window),
        api::UI_STATUS | api::UI_PUMP_STATUS => Ok(()),
        #[cfg(target_arch = "x86")]
        api::RENDER_ARM | api::RENDER_STATUS | api::RENDER_READ => {
            crate::engine_render::control(block)
        }
        #[cfg(target_arch = "x86")]
        api::DRAW_ARM | api::DRAW_STATUS | api::DRAW_READ => crate::engine_draw::control(block),
        #[cfg(target_arch = "x86")]
        api::VISUAL_HIDE | api::VISUAL_STATUS => crate::engine_visual::control(block),
        api::UI_DETACH => detach_locked(),
        api::UI_PROBE
        | api::UI_REGISTRY
        | api::UI_INSPECT_TYPE
        | api::UI_TRACE
        | api::UI_PUMP_ATTACH
        | api::UI_PUMP_FIXTURE
        | api::UI_PUMP_REMOVE
        | api::UI_RENDER_ATTACH
        | api::UI_RENDER_FIXTURE
        | api::UI_RENDER_REMOVE
        | api::UI_DRAW_ATTACH
        | api::UI_DRAW_FIXTURE
        | api::UI_DRAW_REMOVE => dispatch(*block).map(|response| {
            *block = response;
        }),
        _ => Err("unknown engine control command".into()),
    };
    fill(block);
    result
}
