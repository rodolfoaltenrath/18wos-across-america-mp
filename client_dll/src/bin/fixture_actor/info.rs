//! Own cdecl info provider, reference-balanced with explicit simulated cache.
use super::*;
use std::ffi::{CStr, c_char};
static CONTEXT: [AtomicU32; 4] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(7),
    AtomicU32::new(0),
];
static MODEL: [AtomicU32; 26] = [const { AtomicU32::new(0) }; 26];
static HEAD: OnceLock<Box<u32>> = OnceLock::new();
static CREATED: AtomicU32 = AtomicU32::new(0);
static FREED: AtomicU32 = AtomicU32::new(0);
static CACHE: AtomicU32 = AtomicU32::new(0);
static COLD: AtomicU32 = AtomicU32::new(0);
static REPEAT: AtomicBool = AtomicBool::new(false);
static NULL: AtomicBool = AtomicBool::new(false);
fn owner() {
    assert_eq!(
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        OWNER.load(Ordering::Acquire)
    );
}
pub(super) fn instance_context(delta: i32) {
    owner();
    if delta >= 0 {
        CONTEXT[2].fetch_add(delta as u32, Ordering::AcqRel);
    } else {
        CONTEXT[2].fetch_sub(delta.unsigned_abs(), Ordering::AcqRel);
    }
}
pub(super) fn retain(count: u32) {
    owner();
    CACHE.fetch_add(count, Ordering::AcqRel);
    CONTEXT[2].fetch_add(count, Ordering::AcqRel);
}
pub(super) fn descriptors() -> [u32; 3] {
    let head = HEAD.get_or_init(|| {
        MODEL[19].store(3, Ordering::Release);
        Box::new(CONTEXT.as_ptr() as u32)
    });
    [
        allocate as *const () as u32,
        release as *const () as u32,
        head.as_ref() as *const _ as u32,
    ]
}
pub(super) fn arm(command: &str) {
    match command {
        "info-cold" => COLD.store(3, Ordering::Release),
        "info-null" => NULL.store(true, Ordering::Release),
        "info-repeat-growth" => REPEAT.store(true, Ordering::Release),
        "info-normal" => REPEAT.store(false, Ordering::Release),
        _ => unreachable!(),
    }
}
pub(super) fn stats() -> serde_json::Value {
    serde_json::json!({"created":CREATED.load(Ordering::Acquire),"freed":FREED.load(Ordering::Acquire),"cache":CACHE.load(Ordering::Acquire),"context":CONTEXT[2].load(Ordering::Acquire),"model_refs":MODEL[19].load(Ordering::Acquire)})
}
pub(super) fn assert_clean() {
    assert_eq!(
        CREATED.load(Ordering::Acquire),
        FREED.load(Ordering::Acquire)
    );
    assert_eq!(
        CONTEXT[2].load(Ordering::Acquire),
        7 + CACHE.load(Ordering::Acquire)
    );
    assert_eq!(MODEL[19].load(Ordering::Acquire), 3);
}
unsafe extern "C" fn allocate(path: *const c_char) -> *mut c_void {
    owner();
    let path = unsafe { CStr::from_ptr(path) }.to_bytes();
    assert_eq!(path, b"models/fixture/truck.psm");
    if NULL.swap(false, Ordering::AcqRel) {
        return ptr::null_mut();
    }
    let retained = COLD.swap(0, Ordering::AcqRel) + u32::from(REPEAT.load(Ordering::Acquire));
    CACHE.fetch_add(retained, Ordering::AcqRel);
    CONTEXT[2].fetch_add(1 + retained, Ordering::AcqRel);
    MODEL[19].fetch_add(1, Ordering::AcqRel);
    CREATED.fetch_add(1, Ordering::AcqRel);
    Box::into_raw(Box::new([1u32, MODEL.as_ptr() as u32, 0, 0])).cast()
}
unsafe extern "C" fn release(pointer: *mut c_void) {
    owner();
    let info = unsafe { Box::from_raw(pointer.cast::<[u32; 4]>()) };
    assert_eq!(*info, [1, MODEL.as_ptr() as u32, 0, 0]);
    MODEL[19].fetch_sub(1, Ordering::AcqRel);
    CONTEXT[2].fetch_sub(1, Ordering::AcqRel);
    FREED.fetch_add(1, Ordering::AcqRel);
}
