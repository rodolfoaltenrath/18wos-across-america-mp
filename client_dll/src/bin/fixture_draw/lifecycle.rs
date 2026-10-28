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
static PROC: AtomicU32 = AtomicU32::new(0);
static ITEMS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static BLOCK_CALLS: AtomicU32 = AtomicU32::new(0);
static FAIL: AtomicU32 = AtomicU32::new(0);
type Phase = unsafe extern "thiscall" fn(*mut c_void, u32) -> u32;
const OFFSETS: [usize; 2] = [0x5c / 4, 0x60 / 4];
pub unsafe fn setup(page: *mut c_void) {
    PAGE.store(page as usize, Ordering::Release);
    CONFLICT.store(init as *const () as usize, Ordering::Release);
    BLOCK_CONFLICT.store(block_init as *const () as usize, Ordering::Release);
    for (offset, function) in OFFSETS
        .into_iter()
        .zip([init as *const () as usize, done as *const () as usize])
    {
        unsafe { page.cast::<usize>().add(offset).write(function) };
    }
    for (offset, function) in [0x80 / 4, 0x84 / 4].into_iter().zip([
        block_init as *const () as usize,
        block_request as *const () as usize,
    ]) {
        unsafe { page.cast::<usize>().add(offset).write(function) };
    }
}
pub struct Blocks {
    _processor: super::Words<256>,
    _items: [super::Words<24>; 2],
}
impl Blocks {
    pub fn new(cars: &[super::Words<256>; 2]) -> Self {
        let processor = super::words::<256>();
        let items = [super::words::<24>(), super::words::<24>()];
        let proc = super::address(&processor);
        PROC.store(proc, Ordering::Release);
        for i in 0..2 {
            items[i][8 / 4].store(super::address(&cars[i]), Ordering::Release);
            items[i][0x28 / 4].store(proc, Ordering::Release);
            items[i][0x4c / 4].store(1, Ordering::Release);
            items[i][0x50 / 4].store(1, Ordering::Release);
            ITEMS[i].store(super::address(&items[i]), Ordering::Release);
        }
        Self {
            _processor: processor,
            _items: items,
        }
    }
}
pub fn block_descriptors(page: *mut c_void) -> [u32; 6] {
    [
        page as u32 + 0x80,
        block_init as *const () as u32,
        page as u32 + 0x84,
        block_request as *const () as u32,
        0,
        0,
    ]
}
fn word(at: u32) -> &'static AtomicU32 {
    unsafe { &*(at as *const AtomicU32) }
}
fn block_call(which: usize, item: u32, expected: bool) {
    let slot = (PAGE.load(Ordering::Acquire) as *mut usize).wrapping_add(0x80 / 4 + which);
    let function: unsafe extern "thiscall" fn(*mut c_void, *mut c_void) -> bool =
        unsafe { std::mem::transmute(AtomicUsize::from_ptr(slot).load(Ordering::Acquire)) };
    unsafe {
        SetLastError(0x1840);
        assert_eq!(
            function(
                PROC.load(Ordering::Acquire) as *mut c_void,
                item as *mut c_void
            ),
            expected
        );
        assert_eq!(GetLastError(), 0x1841);
    }
}
unsafe extern "thiscall" fn block_init(this: *mut c_void, item: *mut c_void) -> bool {
    assert_eq!(this as u32, PROC.load(Ordering::Acquire));
    assert_eq!(unsafe { GetLastError() }, 0x1840);
    BLOCK_CALLS.fetch_add(1, Ordering::AcqRel);
    let car = word(item as u32 + 8).load(Ordering::Acquire);
    for phase in [-10i32, -1, 0, 10] {
        call(0, car, phase as u32);
    }
    let fail = FAIL.load(Ordering::Acquire) != 0;
    if fail {
        call(1, car, 0);
    }
    unsafe { SetLastError(0x1841) };
    !fail
}
#[unsafe(naked)]
unsafe extern "thiscall" fn block_request(_: *mut c_void, _: *mut c_void) -> bool {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","sub esp,8","push dword ptr [ebp+8]","push ecx","call {implementation}","or eax,0x75a30000","mov esp,ebp","pop ebp","ret 4",implementation=sym request_impl);
}
unsafe extern "C" fn request_impl(this: *mut c_void, item: *mut c_void) -> bool {
    assert_eq!(this as u32, PROC.load(Ordering::Acquire));
    assert_eq!(unsafe { GetLastError() }, 0x1840);
    BLOCK_CALLS.fetch_add(1, Ordering::AcqRel);
    word(this as u32 + 0x3ac).store(item as u32, Ordering::Release);
    word(this as u32 + 0x3b0).store(1, Ordering::Release);
    unsafe { SetLastError(0x1841) };
    true
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
            for _ in 0..client_dll::transition_observation::CAPACITY / 2 + 8 {
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
        kind @ (5 | 6) => {
            let old = super::CARS
                .iter()
                .position(|c| c.load(Ordering::Acquire) == car)
                .unwrap();
            if kind == 6 {
                FAIL.store(1, Ordering::Release);
                block_call(0, ITEMS[1 - old].load(Ordering::Acquire), false);
                FAIL.store(0, Ordering::Release);
            } else {
                block_call(1, ITEMS[old].load(Ordering::Acquire), true);
                block_call(0, ITEMS[1 - old].load(Ordering::Acquire), true);
                for phase in [-10i32, 0, 10] {
                    call(1, car, phase as u32);
                }
                word(PROC.load(Ordering::Acquire) + 0x3ac).store(0, Ordering::Release);
                word(PROC.load(Ordering::Acquire) + 0x3b0).store(0, Ordering::Release);
                super::CAR.store(
                    super::CARS[1 - old].load(Ordering::Acquire),
                    Ordering::Release,
                );
            }
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
        "transition-block-reload" => REQUEST.store(5, Ordering::Release),
        "transition-block-rollback" => REQUEST.store(6, Ordering::Release),
        "transition-block-foreign" => block_call(1, ITEMS[0].load(Ordering::Acquire), true),
        "transition-block-stats" => {
            println!("{}", BLOCK_CALLS.load(Ordering::Acquire));
            return true;
        }
        "transition-foreign" => call(1, car, 1),
        "transition-stats" => {
            println!("{}", CALLS.load(Ordering::Acquire));
            return true;
        }
        "transition-conflict"
        | "transition-unconflict"
        | "transition-block-conflict"
        | "transition-block-unconflict" => {
            let (offset, saved) = if line.contains("block") {
                (0x84 / 4, &BLOCK_CONFLICT)
            } else {
                (OFFSETS[1], &CONFLICT)
            };
            let slot = (PAGE.load(Ordering::Acquire) as *mut usize).wrapping_add(offset);
            let mut old = 0;
            assert_ne!(
                unsafe { VirtualProtect(slot.cast(), 4, PAGE_READWRITE, &mut old) },
                0
            );
            let current = unsafe { AtomicUsize::from_ptr(slot) }
                .swap(saved.swap(0, Ordering::AcqRel), Ordering::AcqRel);
            saved.store(current, Ordering::Release);
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
static BLOCK_CONFLICT: AtomicUsize = AtomicUsize::new(0);
pub fn check_restored(page: *mut c_void) {
    for (slot, original) in [0x5c / 4, 0x60 / 4, 0x80 / 4, 0x84 / 4].into_iter().zip([
        init as *const () as usize,
        done as *const () as usize,
        block_init as *const () as usize,
        block_request as *const () as usize,
    ]) {
        assert_eq!(
            unsafe { AtomicUsize::from_ptr(page.cast::<usize>().add(slot)) }
                .load(Ordering::Acquire),
            original
        );
    }
}
