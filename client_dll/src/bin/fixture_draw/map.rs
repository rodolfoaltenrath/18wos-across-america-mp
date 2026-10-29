//! Own map descriptor, actual no-argument cdecl callback and synthetic reload.
use super::*;
use std::sync::{Mutex, atomic::AtomicBool};
static ARGC: AtomicU32 = AtomicU32::new(2);
static ARGV: AtomicU32 = AtomicU32::new(0);
static ARGUMENTS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static CALLS: AtomicU32 = AtomicU32::new(0);
static MODE: AtomicU32 = AtomicU32::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
static HOLD: AtomicBool = AtomicBool::new(false);
static ENTERED: AtomicBool = AtomicBool::new(false);
static THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
pub(super) static CONFLICT: AtomicUsize = AtomicUsize::new(0);
const RESULT: u32 = 0xfedcba98;
pub unsafe fn setup(page: *mut c_void) {
    for (i, value) in [c"map".as_ptr() as u32, original as *const () as u32, 318, 0]
        .into_iter()
        .enumerate()
    {
        unsafe { page.cast::<u32>().add(0x90 / 4 + i).write(value) };
    }
    ARGUMENTS[0].store(c"map".as_ptr() as u32, Ordering::Release);
    // Deliberately unreadable second argument: the observer must never read it.
    ARGUMENTS[1].store(1, Ordering::Release);
    ARGV.store(ARGUMENTS.as_ptr() as u32, Ordering::Release);
    CONFLICT.store(foreign as *const () as usize, Ordering::Release);
}
pub fn descriptors(page: *mut c_void) -> [u32; 6] {
    [
        page as u32 + 0x90,
        original as *const () as u32,
        &ARGC as *const _ as u32,
        &ARGV as *const _ as u32,
        &PROC as *const _ as u32,
        0,
    ]
}
unsafe extern "C" fn original() -> u32 {
    if MODE.load(Ordering::Acquire) == 6 {
        return gate_load();
    }
    let incoming = unsafe { GetLastError() };
    assert_eq!(incoming, 0x1860);
    CALLS.fetch_add(1, Ordering::AcqRel);
    let depth = DEPTH.fetch_add(1, Ordering::AcqRel);
    let mode = MODE.load(Ordering::Acquire);
    if mode == 5 {
        ENTERED.store(true, Ordering::Release);
        while HOLD.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        ENTERED.store(false, Ordering::Release);
    }
    if mode < 4 {
        if mode == 1 && depth == 0 {
            invoke();
        }
        let saved = super::ARGV.swap(c"map".as_ptr() as u32, Ordering::AcqRel);
        dispatch_command(true);
        super::ARGV.store(saved, Ordering::Release);
        if depth == 0 && mode != 3 {
            let car = super::super::CAR.load(Ordering::Acquire);
            let old = super::super::CARS
                .iter()
                .position(|c| c.load(Ordering::Acquire) == car)
                .unwrap();
            if mode == 2 {
                FAIL.store(1, Ordering::Release);
                block_call(0, ITEMS[1 - old].load(Ordering::Acquire), false);
                FAIL.store(0, Ordering::Release);
            } else {
                block_call(0, ITEMS[1 - old].load(Ordering::Acquire), true);
                for phase in [-10i32, 0, 10] {
                    call(1, car, phase as u32);
                }
                super::super::CAR.store(
                    super::super::CARS[1 - old].load(Ordering::Acquire),
                    Ordering::Release,
                );
            }
        }
        if mode == 3 {
            ARGC.store(0, Ordering::Release);
            ARGV.store(0, Ordering::Release);
        }
    }
    DEPTH.fetch_sub(1, Ordering::AcqRel);
    unsafe { SetLastError(0x1861) };
    RESULT
}
unsafe extern "C" fn foreign() -> u32 {
    0
}
fn invoke() {
    let at = PAGE.load(Ordering::Acquire) + 0x94;
    let callback: unsafe extern "C" fn() -> u32 = unsafe {
        std::mem::transmute(AtomicUsize::from_ptr(at as *mut usize).load(Ordering::Acquire))
    };
    let before: u32;
    unsafe {
        core::arch::asm!("mov {0:e},esp",out(reg) before,options(nomem,nostack,preserves_flags));
    }
    unsafe { SetLastError(0x1860) };
    let result = unsafe { callback() };
    let error = unsafe { GetLastError() };
    let after: u32;
    unsafe {
        core::arch::asm!("mov {0:e},esp",out(reg) after,options(nomem,nostack,preserves_flags));
    }
    assert_eq!((result, error, before), (RESULT, 0x1861, after));
}
pub fn dispatch(mode: u32) {
    MODE.store(mode, Ordering::Release);
    invoke();
    assert_eq!(ARGC.load(Ordering::Acquire), if mode == 3 { 0 } else { 2 });
    assert_eq!(
        ARGV.load(Ordering::Acquire),
        if mode == 3 {
            0
        } else {
            ARGUMENTS.as_ptr() as u32
        }
    );
    ARGC.store(2, Ordering::Release);
    ARGV.store(ARGUMENTS.as_ptr() as u32, Ordering::Release);
}
pub fn command(line: &str) -> bool {
    match line {
        "transition-map-reload" => REQUEST.store(11, Ordering::Release),
        "transition-map-nested" => REQUEST.store(12, Ordering::Release),
        "transition-map-rollback" => REQUEST.store(13, Ordering::Release),
        "transition-map-arguments-change" => REQUEST.store(14, Ordering::Release),
        "transition-map-foreign" => dispatch(4),
        "gate-map" => REQUEST.store(15, Ordering::Release),
        "gate-map-twice" => REQUEST.store(16, Ordering::Release),
        "gate-map-bad" => REQUEST.store(17, Ordering::Release),
        "gate-map-short" => REQUEST.store(18, Ordering::Release),
        "gate-map-nested" => {
            GATE_NESTED.store(true, Ordering::Release);
            REQUEST.store(15, Ordering::Release);
        }
        "gate-stats" => {
            println!("{}", gate_stats());
            return true;
        }
        "transition-map-hold" => {
            HOLD.store(true, Ordering::Release);
            let mut thread = THREAD.lock().unwrap();
            assert!(thread.is_none());
            *thread = Some(std::thread::spawn(|| dispatch(5)));
            let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while !ENTERED.load(Ordering::Acquire) {
                assert!(
                    std::time::Instant::now() < until,
                    "foreign callback did not enter"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        "transition-map-release" => {
            HOLD.store(false, Ordering::Release);
            THREAD.lock().unwrap().take().unwrap().join().unwrap();
        }
        "transition-map-stats" => {
            println!("{}", CALLS.load(Ordering::Acquire));
            return true;
        }
        _ => return false,
    }
    println!("armed");
    true
}
pub fn check_restored(page: *mut c_void) {
    for (i, expected) in [c"map".as_ptr() as u32, original as *const () as u32, 318, 0]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            word(page as u32 + 0x90 + i as u32 * 4).load(Ordering::Acquire),
            expected
        );
    }
}
// Active load-gate mode (MODE 6): the descriptor is invoked like cmd_execute
// (globals published, callback read from descriptor+4), then the source argv
// is destroyed. A load reached while a PSM instance or a queued draw is alive
// is recorded as unsafe; the new world swaps the player entity.
const GATE_NAME: &std::ffi::CStr = c"fixture.sav";
static GATE_ARGS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static GATE_IN_INVOKE: AtomicBool = AtomicBool::new(false);
static GATE_NESTED: AtomicBool = AtomicBool::new(false);
/// invokes, loads, synchronous loads, unsafe loads, bad calls, swallowed,
/// no-argument calls, nested calls, wrong LastError/stack.
static GATE: [AtomicU32; 9] = [const { AtomicU32::new(0) }; 9];
fn gate_bump(i: usize) {
    GATE[i].fetch_add(1, Ordering::AcqRel);
}
fn gate_stats() -> serde_json::Value {
    let g = |i: usize| GATE[i].load(Ordering::Acquire);
    serde_json::json!({"invokes":g(0),"loads":g(1),"synchronous_loads":g(2),"unsafe_loads":g(3),"bad_calls":g(4),"swallowed":g(5),"no_arguments":g(6),"nested":g(7),"abi_errors":g(8),"live_instances":crate::fixture_actor::live_instances()})
}
fn cstr(at: u32) -> Option<&'static [u8]> {
    // Own fixture memory: the bound argv frame or the source arguments.
    (at > 0x10000)
        .then(|| unsafe { std::ffi::CStr::from_ptr(at as *const std::ffi::c_char) }.to_bytes())
}
fn gate_load() -> u32 {
    let depth = DEPTH.fetch_add(1, Ordering::AcqRel);
    CALLS.fetch_add(1, Ordering::AcqRel);
    let argc = ARGC.load(Ordering::Acquire);
    let argv = ARGV.load(Ordering::Acquire) as *const u32;
    let owner = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
        == super::super::OWNER.load(Ordering::Acquire);
    if depth > 0 {
        gate_bump(7);
    } else if argc < 2 {
        gate_bump(6);
    } else {
        let args = unsafe { (*argv, *argv.add(1)) };
        if !owner
            || argc != 2
            || cstr(args.0) != Some(b"map")
            || cstr(args.1) != Some(GATE_NAME.to_bytes())
        {
            gate_bump(4);
        }
        gate_bump(1);
        if GATE_IN_INVOKE.load(Ordering::Acquire) {
            gate_bump(2);
        }
        let pending = !super::super::DRAWN.lock().unwrap().pending.is_empty();
        if crate::fixture_actor::live_instances() != 0 || pending {
            gate_bump(3);
        }
        if GATE_NESTED.swap(false, Ordering::AcqRel) {
            // A command executed by the load itself, through the same descriptor.
            let callback: unsafe extern "C" fn() -> u32 = unsafe {
                std::mem::transmute(
                    AtomicUsize::from_ptr((PAGE.load(Ordering::Acquire) + 0x94) as *mut usize)
                        .load(Ordering::Acquire),
                )
            };
            unsafe { callback() };
        }
        let car = super::super::CAR.load(Ordering::Acquire);
        let old = super::super::CARS
            .iter()
            .position(|c| c.load(Ordering::Acquire) == car)
            .unwrap();
        super::super::CAR.store(
            super::super::CARS[1 - old].load(Ordering::Acquire),
            Ordering::Release,
        );
    }
    DEPTH.fetch_sub(1, Ordering::AcqRel);
    unsafe { SetLastError(0x1861) };
    RESULT
}
pub fn gate(kind: u32) {
    MODE.store(6, Ordering::Release);
    GATE_ARGS[0].store(c"map".as_ptr() as u32, Ordering::Release);
    // kind 17: unreadable second argument; the gate must drop the request.
    GATE_ARGS[1].store(
        if kind == 17 {
            1
        } else {
            GATE_NAME.as_ptr() as u32
        },
        Ordering::Release,
    );
    ARGC.store(if kind == 18 { 1 } else { 2 }, Ordering::Release);
    ARGV.store(GATE_ARGS.as_ptr() as u32, Ordering::Release);
    let at = PAGE.load(Ordering::Acquire) + 0x94;
    for _ in 0..if kind == 16 { 2 } else { 1 } {
        let callback: unsafe extern "C" fn() -> u32 = unsafe {
            std::mem::transmute(AtomicUsize::from_ptr(at as *mut usize).load(Ordering::Acquire))
        };
        gate_bump(0);
        let before: u32;
        unsafe {
            core::arch::asm!("mov {0:e},esp",out(reg) before,options(nomem,nostack,preserves_flags));
        }
        GATE_IN_INVOKE.store(true, Ordering::Release);
        unsafe { SetLastError(0x1870) };
        let result = unsafe { callback() };
        let error = unsafe { GetLastError() };
        GATE_IN_INVOKE.store(false, Ordering::Release);
        let after: u32;
        unsafe {
            core::arch::asm!("mov {0:e},esp",out(reg) after,options(nomem,nostack,preserves_flags));
        }
        let consistent = match result {
            RESULT => error == 0x1861,
            0 => error == 0x1870,
            _ => false,
        };
        if before != after || !consistent {
            gate_bump(8);
        }
        if result == 0 {
            gate_bump(5);
        }
    }
    // cmd_execute returns: its argv storage is gone and the globals are stale.
    GATE_ARGS[0].store(c"bad".as_ptr() as u32, Ordering::Release);
    GATE_ARGS[1].store(c"WRONG.sav".as_ptr() as u32, Ordering::Release);
    ARGC.store(2, Ordering::Release);
    ARGV.store(ARGUMENTS.as_ptr() as u32, Ordering::Release);
}
