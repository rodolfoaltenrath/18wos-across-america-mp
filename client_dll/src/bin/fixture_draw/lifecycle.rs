//! Own thiscall(phase) virtuals and simulated teardown, never commercial code.
use std::{
    ffi::c_void,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    System::Memory::{PAGE_READWRITE, VirtualProtect},
};
static PAGE: AtomicUsize = AtomicUsize::new(0);
static REQUEST: AtomicU32 = AtomicU32::new(0);
static CALLS: AtomicU32 = AtomicU32::new(0);
type Phase = unsafe extern "thiscall" fn(*mut c_void, u32) -> u32;
const OFFSETS: [usize; 2] = [0x5c / 4, 0x60 / 4];
pub unsafe fn setup(page: *mut c_void) {
    PAGE.store(page as usize, Ordering::Release);
    CONFLICT.store(init as *const () as usize, Ordering::Release);
    for (offset, function) in OFFSETS
        .into_iter()
        .zip([init as *const () as usize, done as *const () as usize])
    {
        unsafe { page.cast::<usize>().add(offset).write(function) };
    }
}
pub fn descriptors(page: *mut c_void) -> [u32; 6] {
    [
        page as u32 + 0x5c,
        init as *const () as u32,
        page as u32 + 0x60,
        done as *const () as u32,
        0,
        0,
    ]
}
fn call(which: usize, car: u32, phase: u32) {
    let slot = (PAGE.load(Ordering::Acquire) as *mut usize).wrapping_add(OFFSETS[which]);
    let function: Phase =
        unsafe { std::mem::transmute(AtomicUsize::from_ptr(slot).load(Ordering::Acquire)) };
    unsafe {
        SetLastError(0x1830);
        let result = function(car as *mut c_void, phase);
        assert_eq!(
            result,
            0x1800u32.wrapping_add(phase).wrapping_add(which as u32)
        );
        assert_eq!(GetLastError(), 0x1831);
    }
}
unsafe extern "thiscall" fn init(this: *mut c_void, phase: u32) -> u32 {
    assert_eq!(unsafe { GetLastError() }, 0x1830);
    CALLS.fetch_add(1, Ordering::AcqRel);
    if phase == 0 {
        unsafe { AtomicU32::from_ptr(this.cast::<u32>().add(0x200 / 4)) }
            .store(0x11110000, Ordering::Release);
    }
    unsafe { SetLastError(0x1831) };
    0x1800u32.wrapping_add(phase)
}
unsafe extern "thiscall" fn done(this: *mut c_void, phase: u32) -> u32 {
    assert_eq!(unsafe { GetLastError() }, 0x1830);
    CALLS.fetch_add(1, Ordering::AcqRel);
    if phase == 0 {
        unsafe { AtomicU32::from_ptr(this.cast::<u32>().add(0x200 / 4)) }
            .store(0, Ordering::Release);
    }
    if phase == 7 {
        call(1, this as u32, 8);
    }
    unsafe { SetLastError(0x1831) };
    0x1801u32.wrapping_add(phase)
}
pub fn frame(car: u32) {
    match REQUEST.swap(0, Ordering::AcqRel) {
        1 => {
            call(1, car, 1);
            call(1, car, 0);
            call(0, car, 0);
            call(0, car, 1);
        }
        2 => call(1, car, 7),
        3 => {
            for _ in 0..80 {
                call(1, car, 1);
            }
        }
        4 => {
            // Order observed in the real reload: new init BEFORE old destroy.
            let next = super::CARS
                .iter()
                .map(|c| c.load(Ordering::Acquire))
                .find(|&other| other != car)
                .unwrap();
            for phase in [-10i32, -1, 0, 10] {
                call(0, next, phase as u32);
            }
            for phase in [-10i32, 0, 10] {
                call(1, car, phase as u32);
            }
            super::CAR.store(next, Ordering::Release);
        }
        _ => {}
    }
}
pub fn command(line: &str, car: u32) -> bool {
    match line {
        "transition-cycle" => REQUEST.store(1, Ordering::Release),
        "transition-nested" => REQUEST.store(2, Ordering::Release),
        "transition-overflow" => REQUEST.store(3, Ordering::Release),
        "transition-reload" => REQUEST.store(4, Ordering::Release),
        "transition-foreign" => call(1, car, 1),
        "transition-stats" => {
            println!("{}", CALLS.load(Ordering::Acquire));
            return true;
        }
        "transition-conflict" | "transition-unconflict" => {
            let slot = (PAGE.load(Ordering::Acquire) as *mut usize).wrapping_add(OFFSETS[1]);
            let mut old = 0;
            assert_ne!(
                unsafe { VirtualProtect(slot.cast(), 4, PAGE_READWRITE, &mut old) },
                0
            );
            let current = unsafe { AtomicUsize::from_ptr(slot) }
                .swap(CONFLICT.swap(0, Ordering::AcqRel), Ordering::AcqRel);
            CONFLICT.store(current, Ordering::Release);
            let mut ignored = 0;
            assert_ne!(
                unsafe { VirtualProtect(slot.cast(), 4, old, &mut ignored) },
                0
            );
        }
        _ => return false,
    }
    println!("armed");
    true
}
// A foreign slot owner is executable but must never be overwritten by removal.
static CONFLICT: AtomicUsize = AtomicUsize::new(0);
pub fn check_restored(page: *mut c_void) {
    for (slot, original) in [0x5c / 4, 0x60 / 4]
        .into_iter()
        .zip([init as *const () as usize, done as *const () as usize])
    {
        assert_eq!(
            unsafe { AtomicUsize::from_ptr(page.cast::<usize>().add(slot)) }
                .load(Ordering::Acquire),
            original
        );
    }
}
