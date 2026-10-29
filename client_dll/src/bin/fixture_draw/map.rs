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
