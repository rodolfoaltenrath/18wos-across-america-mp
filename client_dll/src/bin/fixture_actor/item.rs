//! Persistent own anonymous-item slot and separate placed storage allocator.
//! It intentionally rejects direct/free-twice and wrong-thread ownership paths.
use super::*;
use std::{
    mem::MaybeUninit,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64},
    },
};
static POOL: OnceLock<Mutex<Box<[u32; 26]>>> = OnceLock::new();
static MANAGER: OnceLock<Box<[AtomicU64; 16]>> = OnceLock::new();
static OPS: [u32; 9] = [0; 9]; // Identity only; own release explicitly exercises the operations.
static DESCRIPTORS: OnceLock<[Box<[u32; 12]>; 2]> = OnceLock::new();
static ALLOCATED: AtomicU32 = AtomicU32::new(0);
static RELEASED: AtomicU32 = AtomicU32::new(0);
static FAIL_FIX: AtomicBool = AtomicBool::new(false);
pub(super) fn fail_fix() {
    FAIL_FIX.store(true, Ordering::Release);
}
fn owner() {
    assert_eq!(
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        OWNER.load(Ordering::Acquire)
    );
}
fn pool() -> &'static Mutex<Box<[u32; 26]>> {
    POOL.get_or_init(|| Mutex::new(Box::new([0; 26])))
}
fn manager() -> &'static [AtomicU64; 16] {
    MANAGER.get_or_init(|| Box::new(std::array::from_fn(|_| AtomicU64::new(0))))
}
pub(super) fn descriptors(factory: [u32; 6]) -> [u32; 2] {
    let tables = DESCRIPTORS.get_or_init(|| {
        let mut good = Box::new([0; 12]);
        good[..6].copy_from_slice(&factory);
        good[6..].copy_from_slice(&[
            allocate as *const () as u32,
            fix as *const () as u32,
            free as *const () as u32,
            release as *const () as u32,
            OPS.as_ptr() as u32,
            &ROOT as *const _ as u32,
        ]);
        let mut bad = good.clone();
        bad[5] = 1;
        [good, bad]
    });
    [tables[0].as_ptr() as u32, tables[1].as_ptr() as u32]
}
pub(super) fn stats() -> serde_json::Value {
    serde_json::json!({"allocated": ALLOCATED.load(Ordering::Acquire), "released":RELEASED.load(Ordering::Acquire), "bytes_allocated":manager()[12].load(Ordering::Acquire),"bytes_freed":manager()[13].load(Ordering::Acquire)})
}
pub(super) fn assert_clean() {
    assert_eq!(
        ALLOCATED.load(Ordering::Acquire),
        RELEASED.load(Ordering::Acquire)
    );
    assert_eq!(
        manager()[12].load(Ordering::Acquire),
        manager()[13].load(Ordering::Acquire)
    );
}
unsafe extern "thiscall" fn allocate(context: *mut c_void) -> *mut c_void {
    owner();
    assert_eq!(context as u32, &ROOT as *const _ as u32);
    let mut item = pool().lock().unwrap();
    assert_eq!(item[19], 0);
    item.fill(0);
    item[0] = 0xd2;
    item[10] = manager().as_ptr() as u32;
    item[12] = OPS.as_ptr() as u32;
    item[19] = 1;
    ALLOCATED.fetch_add(1, Ordering::AcqRel);
    item.as_mut_ptr().cast()
}
unsafe extern "C" fn fix(pointer: *mut c_void, size: u32) -> *mut c_void {
    owner();
    let mut item = pool().lock().unwrap();
    assert_eq!(pointer, item.as_mut_ptr().cast());
    assert_eq!(size as usize, std::mem::size_of::<Object>());
    assert_eq!((item[2], item[19], item[22]), (0, 1, 0));
    if FAIL_FIX.swap(false, Ordering::AcqRel) {
        return ptr::null_mut();
    }
    let storage = Box::into_raw(Box::new(MaybeUninit::<Object>::uninit()));
    item[2] = storage as u32;
    item[18] = size;
    item[22] = 1;
    item[0] |= 8;
    manager()[12].fetch_add(size.into(), Ordering::AcqRel);
    storage.cast()
}
unsafe extern "C" fn free(pointer: *mut c_void) {
    owner();
    let mut item = pool().lock().unwrap();
    assert_eq!(pointer, item.as_mut_ptr().cast());
    assert_ne!(item[2], 0);
    assert_eq!(item[22], 0);
    unsafe {
        drop(Box::from_raw(item[2] as *mut MaybeUninit<Object>));
    }
    manager()[13].fetch_add(item[18].into(), Ordering::AcqRel);
    item[2] = 0;
    item[18] = 0;
    item[0] &= !8;
}
unsafe extern "C" fn release(pointer: *mut c_void) -> u32 {
    owner();
    let storage = {
        let mut item = pool().lock().unwrap();
        assert_eq!(pointer, item.as_mut_ptr().cast());
        assert_eq!((item[19], item[22], item[23]), (1, 0, 0));
        item[19] = 0;
        item[2] as *mut c_void
    };
    if !storage.is_null() {
        let fields = storage.cast::<u32>();
        assert_eq!(unsafe { *fields.add(0x60 / 4) }, pointer as u32);
        assert_eq!(unsafe { *fields.add(0x84 / 4) }, &ROOT as *const _ as u32);
        unsafe {
            destroy(storage, storage);
            free(pointer);
        }
    }
    let mut item = pool().lock().unwrap();
    item.fill(0);
    item[0] = 0x80;
    RELEASED.fetch_add(1, Ordering::AcqRel);
    0
}
