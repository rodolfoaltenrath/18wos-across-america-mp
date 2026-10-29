//! Bounded work at the return from the profiled PeekMessageA import.
//! The naked Rust shim preserves stdcall's five arguments and captures the REAL
//! caller return address before a compiler prologue. Engine APIs run only from
//! explicit phase tasks, outside nested PeekMessage and window callbacks.
//! Installing this observer pins the DLL until process exit: removing an IAT
//! slot cannot prove another thread has not already loaded its old value.
use crate::{api::EngineControl, inspection::process::Process, phase_queue::Queue, profile};
use std::{
    ffi::c_void,
    path::Path,
    ptr,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND, SetLastError},
    System::{
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW, GetModuleHandleW,
        },
        Memory::{PAGE_READWRITE, VirtualProtect},
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::WindowsAndMessaging::{MSG, PeekMessageA, PostMessageW, WM_NULL, WM_QUIT},
};
type Peek = unsafe extern "system" fn(*mut MSG, HWND, u32, u32, u32) -> i32;
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static SLOT: AtomicUsize = AtomicUsize::new(0);
static BASE: AtomicU32 = AtomicU32::new(0);
static THREAD: AtomicU32 = AtomicU32::new(0);
static FIXTURE: AtomicU32 = AtomicU32::new(0);
static STATE: AtomicU32 = AtomicU32::new(0); // 0 unused, 1 installed, 2 restored/resident
static LAST_THREAD: AtomicU32 = AtomicU32::new(0);
static LAST_SITE: AtomicU32 = AtomicU32::new(0);
static HITS: AtomicU32 = AtomicU32::new(0);
static RESTORE: AtomicU32 = AtomicU32::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
static TASKS: Queue = Queue::new();
struct DepthGuard(bool);
pub(crate) fn release_context() -> crate::release_policy::Context {
    crate::release_policy::Context {
        current_thread: unsafe { GetCurrentThreadId() },
        owner_thread: THREAD.load(Ordering::Acquire),
        pump_installed: STATE.load(Ordering::Acquire) == 1,
        pump_depth: DEPTH.load(Ordering::Acquire),
        window_depth: crate::engine_bridge::window_depth(),
        draw_depth: crate::engine_draw::callback_depth(),
        render_depth: crate::engine_render::callback_depth(),
    }
}
impl Drop for DepthGuard {
    fn drop(&mut self) {
        if self.0 {
            DEPTH.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

// Entry: [esp] return, [esp+4..24] five stdcall arguments. Preserve EBP,
// realign the Rust call's stack, append the caller as argument six, and ret 20.
// The implementation returns EAX and preserves EBX/ESI/EDI/EBP by its ABI.
#[unsafe(naked)]
unsafe extern "system" fn peek_shim(_: *mut MSG, _: HWND, _: u32, _: u32, _: u32) -> i32 {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16", "sub esp, 8",
        "push dword ptr [ebp+4]", "push dword ptr [ebp+24]",
        "push dword ptr [ebp+20]", "push dword ptr [ebp+16]",
        "push dword ptr [ebp+12]", "push dword ptr [ebp+8]",
        "call {implementation}", "mov esp, ebp", "pop ebp", "ret 20",
        implementation = sym peek_impl,
    );
}
unsafe extern "system" fn peek_impl(
    msg: *mut MSG,
    hwnd: HWND,
    min: u32,
    max: u32,
    remove: u32,
    caller: u32,
) -> i32 {
    let thread = unsafe { GetCurrentThreadId() };
    let owner = thread == THREAD.load(Ordering::Acquire);
    if owner {
        DEPTH.fetch_add(1, Ordering::AcqRel);
    }
    let _depth = DepthGuard(owner);
    // ORIGINAL stays valid even after removal; this module is process-pinned.
    let original: Peek = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Acquire)) };
    let result = unsafe { original(msg, hwnd, min, max, remove) };
    let error = unsafe { GetLastError() };
    let site = caller.wrapping_sub(BASE.load(Ordering::Acquire));
    if owner && (FIXTURE.load(Ordering::Acquire) != 0 || profile::PUMP_RETURN_SITES.contains(&site))
    {
        LAST_THREAD.store(thread, Ordering::Release);
        LAST_SITE.store(site, Ordering::Release);
        HITS.fetch_add(1, Ordering::Relaxed);
        // A quit message has priority over queued lifecycle operations.
        let quitting = result != 0 && !msg.is_null() && unsafe { (*msg).message } == WM_QUIT;
        if !quitting
            && STATE.load(Ordering::Acquire) == 1
            && DEPTH.load(Ordering::Acquire) == 1
            && crate::engine_bridge::window_depth() == 0
        {
            TASKS.run_one(|block| {
                crate::engine_lifecycle::run(block, FIXTURE.load(Ordering::Acquire) != 0)
            });
            crate::engine_load_gate::maintain(FIXTURE.load(Ordering::Acquire) != 0);
            crate::engine_visual::maintain(FIXTURE.load(Ordering::Acquire) != 0);
        }
    }
    unsafe { SetLastError(error) };
    result
}
fn replace(slot: usize, expected: usize, desired: usize) -> Result<(), String> {
    let mut old = 0;
    if unsafe { VirtualProtect(slot as *mut c_void, 4, PAGE_READWRITE, &mut old) } == 0 {
        return Err(format!(
            "cannot unlock IAT slot: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Store protection before mutation so a failed restoration can be retried.
    if RESTORE.load(Ordering::Acquire) == 0 {
        RESTORE.store(old, Ordering::Release);
    }
    let word = unsafe { AtomicUsize::from_ptr(slot as *mut usize) };
    let exchanged = word.compare_exchange(expected, desired, Ordering::AcqRel, Ordering::Acquire);
    let mut ignored = 0;
    let restored = unsafe {
        VirtualProtect(
            slot as *mut c_void,
            4,
            RESTORE.load(Ordering::Acquire),
            &mut ignored,
        )
    };
    if restored == 0 {
        return Err("IAT page protection restoration failed; module pinned, retry removal".into());
    }
    RESTORE.store(0, Ordering::Release);
    exchanged
        .map(|_| ())
        .map_err(|_| "IAT owner changed; refusing overwrite".into())
}
pub fn install(thread: u32, fixture_slot: Option<u32>) -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 0 {
        return Err("pump observer already used; restart process for another installation".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let modules = process.modules().map_err(|e| e.to_string())?;
    let base = unsafe { GetModuleHandleW(ptr::null()) } as usize as u64;
    let executable = modules
        .iter()
        .find(|m| m.base == base)
        .ok_or("executable module unavailable")?;
    let slot = if let Some(slot) = fixture_slot {
        if !executable.name.eq_ignore_ascii_case("engine_fixture.exe") {
            return Err("fixture mode restricted to our engine_fixture.exe".into());
        }
        slot as usize
    } else {
        profile::verify_executable(Path::new(&executable.path))?;
        crate::inspection::entities::registered(&process).map_err(|e| e.to_string())?;
        (executable.base + u64::from(profile::PEEK_MESSAGE_IAT)) as usize
    };
    if slot == 0 || !slot.is_multiple_of(4) {
        return Err("invalid IAT slot alignment".into());
    }
    let mut bytes = [0; 4];
    process
        .read(slot as u64, &mut bytes)
        .map_err(|e| e.to_string())?;
    let original = u32::from_le_bytes(bytes) as usize;
    if original != PeekMessageA as *const () as usize {
        return Err("PeekMessageA import already modified or incompatible".into());
    }
    let mut module = ptr::null_mut();
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            peek_shim as *const u16,
            &mut module,
        )
    } == 0
    {
        return Err("cannot pin observer module".into());
    }
    ORIGINAL.store(original, Ordering::Release);
    SLOT.store(slot, Ordering::Release);
    BASE.store(executable.base as u32, Ordering::Release);
    THREAD.store(thread, Ordering::Release);
    FIXTURE.store(u32::from(fixture_slot.is_some()), Ordering::Release);
    STATE.store(1, Ordering::Release);
    replace(slot, original, peek_shim as *const () as usize)
}
pub fn remove() -> Result<(), String> {
    if crate::engine_visual::allocated() {
        return Err("release the test visual before removing the pump".into());
    }
    TASKS.cancel_pending()?;
    if TASKS.stats().1 != 0 {
        return Err("phase operation still running; dispatcher and module retained".into());
    }
    crate::engine_load_gate::remove()?;
    crate::engine_transition::remove()?;
    crate::engine_draw::remove()?;
    crate::engine_render::remove()?;
    if STATE.load(Ordering::Acquire) != 1 {
        return Ok(());
    }
    let slot = SLOT.load(Ordering::Acquire);
    let original = ORIGINAL.load(Ordering::Acquire);
    let word = unsafe { AtomicUsize::from_ptr(slot as *mut usize) };
    if word.load(Ordering::Acquire) == original {
        // A prior attempt may have restored the value but failed the protection.
        replace(slot, original, original)?;
    } else {
        replace(slot, peek_shim as *const () as usize, original)?;
    }
    STATE.store(2, Ordering::Release);
    Ok(())
}
pub fn fill(block: &mut EngineControl) {
    block.phase = STATE.load(Ordering::Acquire);
    block.phase_thread = LAST_THREAD.load(Ordering::Acquire);
    block.phase_site = LAST_SITE.load(Ordering::Acquire);
    block.phase_hits = HITS.load(Ordering::Acquire);
    (
        block.phase_pending,
        block.phase_running,
        block.phase_completed,
    ) = TASKS.stats();
}
pub fn dispatch(block: EngineControl, window: HWND) -> Result<EngineControl, String> {
    if STATE.load(Ordering::Acquire) != 1 {
        return Err("attach the pump before submitting phase work".into());
    }
    let ticket = TASKS.enqueue(block)?;
    unsafe {
        PostMessageW(window, WM_NULL, 0, 0);
    }
    ticket.wait(std::time::Duration::from_secs(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "system" fn fake(
        msg: *mut MSG,
        hwnd: HWND,
        min: u32,
        max: u32,
        remove: u32,
    ) -> i32 {
        unsafe {
            (*msg).message = (hwnd as usize as u32) ^ min ^ max ^ remove;
            SetLastError(0x18aa);
        }
        0x76543210
    }
    #[test]
    fn naked_stdcall_preserves_five_arguments_return_stack_and_last_error() {
        ORIGINAL.store(fake as *const () as usize, Ordering::Release);
        let mut msg = MSG::default();
        for n in 0..1000 {
            let result = unsafe { peek_shim(&mut msg, 0x1234usize as HWND, n, 0x7654, 0x55aa) };
            assert_eq!(unsafe { GetLastError() }, 0x18aa);
            assert_eq!(result, 0x76543210);
            assert_eq!(msg.message, 0x1234 ^ n ^ 0x7654 ^ 0x55aa);
        }
    }
}
