//! Own provider with stable item pool, data, duplicate shaders and three packets.
use super::*;
use std::{ffi::c_char, sync::Mutex};
static OPS: [u32; 9] = [0; 9];
struct State {
    item: Box<[u32; 26]>,
    data: Option<Box<[u32; 64]>>,
    packets: [Box<[u32; 8]>; 3],
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static SHADERS: OnceLock<[Box<[AtomicU32; 26]>; 2]> = OnceLock::new();
static CREATED: AtomicU32 = AtomicU32::new(0);
static FREED: AtomicU32 = AtomicU32::new(0);
static NULL: AtomicBool = AtomicBool::new(false);
static BAD: AtomicBool = AtomicBool::new(false);
static COLD: AtomicU32 = AtomicU32::new(0);
static REPEAT: AtomicBool = AtomicBool::new(false);
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(State {
            item: Box::new([0; 26]),
            data: None,
            packets: std::array::from_fn(|_| Box::new([0; 8])),
        })
    })
}
fn shaders() -> &'static [Box<[AtomicU32; 26]>; 2] {
    SHADERS.get_or_init(|| {
        std::array::from_fn(|i| {
            let v = Box::new(std::array::from_fn(|_| AtomicU32::new(0)));
            v[19].store(3 + i as u32 * 2, Ordering::Release);
            v
        })
    })
}
fn owner() {
    assert_eq!(
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        OWNER.load(Ordering::Acquire)
    );
}
pub(super) fn descriptors() -> [u32; 6] {
    let base = super::info::descriptors();
    [
        base[0],
        base[1],
        base[2],
        allocate as *const () as u32,
        free as *const () as u32,
        OPS.as_ptr() as u32,
    ]
}
pub(super) fn arm(command: &str) {
    match command {
        "instance-null" => NULL.store(true, Ordering::Release),
        "instance-bad-header" => BAD.store(true, Ordering::Release),
        "instance-cold" => COLD.store(2, Ordering::Release),
        "instance-repeat-growth" => REPEAT.store(true, Ordering::Release),
        "instance-normal" => REPEAT.store(false, Ordering::Release),
        _ => unreachable!(),
    }
}
pub(super) fn stats() -> serde_json::Value {
    let state = state().lock().unwrap();
    serde_json::json!({"created":CREATED.load(Ordering::Acquire),"freed":FREED.load(Ordering::Acquire),"bytes_allocated":CREATED.load(Ordering::Acquire)*256,"bytes_freed":FREED.load(Ordering::Acquire)*256,"pool_refs":state.item[19],"shader_refs":[shaders()[0][19].load(Ordering::Acquire),shaders()[1][19].load(Ordering::Acquire)]})
}
pub(super) fn assert_clean() {
    let state = state().lock().unwrap();
    assert!(state.data.is_none());
    assert_eq!(state.item[19], 0);
    assert!(state.packets.iter().all(|p| p[0] == 0));
    assert_eq!(
        CREATED.load(Ordering::Acquire),
        FREED.load(Ordering::Acquire)
    );
    assert_eq!(shaders()[0][19].load(Ordering::Acquire), 3);
    assert_eq!(shaders()[1][19].load(Ordering::Acquire), 5);
}
unsafe extern "C" fn allocate(
    info: *mut c_void,
    skins: *const *const c_char,
    count: u32,
    capacity: u32,
) -> *mut c_void {
    owner();
    assert!(skins.is_null());
    assert_eq!((count, capacity), (0, 1));
    let info = info.cast::<u32>();
    assert_eq!(unsafe { *info }, 1);
    if NULL.swap(false, Ordering::AcqRel) {
        return ptr::null_mut();
    }
    let mut s = state().lock().unwrap();
    assert!(s.data.is_none());
    assert_eq!(s.item[19], 0);
    let item = s.item.as_mut_ptr();
    let mut data = Box::new([0u32; 64]);
    let address = data.as_mut_ptr() as u32;
    data[1] = 3;
    data[2] = info as u32;
    data[8] = address + 120;
    data[12] = address + 132;
    data[20] = 1f32.to_bits();
    data[21] = u32::MAX;
    data[22] = if BAD.swap(false, Ordering::AcqRel) {
        2
    } else {
        1
    };
    for i in 0..3 {
        let shader = &shaders()[usize::from(i == 2)];
        shader[19].fetch_add(1, Ordering::AcqRel);
        data[30 + i] = shader.as_ptr() as u32;
        let packet = &mut s.packets[i];
        packet.fill(0);
        packet[0] = 1;
        packet[1] = item as u32;
        packet[3] = i as u32;
        data[33 + i] = packet.as_ptr() as u32;
    }
    s.item.fill(0);
    s.item[0] = 0x8a;
    s.item[2] = address;
    s.item[12] = OPS.as_ptr() as u32;
    s.item[18] = 256;
    s.item[19] = 1;
    s.item[20] = 1;
    s.data = Some(data);
    unsafe {
        *info += 1;
    }
    super::info::instance_context(1);
    super::info::retain(COLD.swap(0, Ordering::AcqRel) + u32::from(REPEAT.load(Ordering::Acquire)));
    CREATED.fetch_add(1, Ordering::AcqRel);
    item.cast()
}
unsafe extern "C" fn free(pointer: *mut c_void) {
    owner();
    let mut s = state().lock().unwrap();
    assert_eq!(pointer, s.item.as_mut_ptr().cast());
    assert_eq!((s.item[19], s.item[20], s.item[22]), (1, 1, 0));
    let data = s.data.take().unwrap();
    let info = data[2] as *mut u32;
    assert_eq!(unsafe { *info }, 2);
    unsafe {
        *info -= 1;
    }
    for i in 0..3 {
        let shader = &shaders()[usize::from(i == 2)];
        shader[19].fetch_sub(1, Ordering::AcqRel);
        assert_eq!(s.packets[i][0], 1);
        s.packets[i][0] = 0;
    }
    s.item.fill(0);
    s.item[0] = 0x80;
    super::info::instance_context(-1);
    FREED.fetch_add(1, Ordering::AcqRel);
}
