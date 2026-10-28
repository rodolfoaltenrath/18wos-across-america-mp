//! Own cdecl fixture with inline storage and a deliberately dirty EAX high part.
//! No game callback or commercial asset is needed to test the boundary.
use super::*;
use std::ffi::{CStr, c_char};

static DEFINITIONS: OnceLock<[Box<[u32; 24]>; 9]> = OnceLock::new();
static CALLS: AtomicU32 = AtomicU32::new(0);
static FAILURE: AtomicU32 = AtomicU32::new(0);
pub(super) fn roots() -> [u32; 9] {
    let rows = DEFINITIONS.get_or_init(|| {
        let mut good = [0u32; 24];
        good[1] = c"model_name".as_ptr() as u32;
        good[4] = 21;
        good[6] = 64;
        good[7] = u32::MAX;
        good[8] = 0x200;
        good[9] = u32::MAX;
        good[10] = u32::MAX;
        good[13] = setter as *const () as u32;
        let changes = [
            (5, 2),
            (8, 0x201),
            (6, 65),
            (10, 0),
            (9, 0),
            (4, 14), // Generic string shares this setter, but is not model_name.
            (7, 0),
            (13, 1),
        ];
        std::array::from_fn(|i| {
            let mut row = good;
            if i > 0 {
                let (at, value) = changes[i - 1];
                row[at] = value;
            }
            Box::new(row)
        })
    });
    std::array::from_fn(|i| rows[i].as_ptr() as u32)
}
pub(super) fn callback() -> u32 {
    setter as *const () as u32
}
pub(super) fn calls() -> u32 {
    CALLS.load(Ordering::Acquire)
}
pub(super) fn arm(command: &str) {
    FAILURE.store(
        match command {
            "model-fail-set" => 1,
            "model-no-write" => 2,
            "model-side-effect" => 3,
            _ => unreachable!(),
        },
        Ordering::Release,
    );
}

// Only AL belongs to a C++ bool return. Upper EAX bits intentionally remain
// nonzero for both success and failure; ret has no callee argument cleanup.
#[unsafe(naked)]
unsafe extern "C" fn setter(_: *mut c_void, _: *const u32, _: u32, _: *const c_char) -> bool {
    core::arch::naked_asm!(
        "push ebp", "mov ebp, esp", "and esp, -16",
        "push dword ptr [ebp+20]", "push dword ptr [ebp+16]",
        "push dword ptr [ebp+12]", "push dword ptr [ebp+8]",
        "call {implementation}", "or eax, 0x6a180000",
        "mov esp, ebp", "pop ebp", "ret",
        implementation = sym set_impl,
    );
}
unsafe extern "C" fn set_impl(
    object: *mut c_void,
    definition: *const u32,
    index: u32,
    source: *const c_char,
) -> bool {
    assert_eq!(
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        OWNER.load(Ordering::Acquire)
    );
    assert_eq!(definition as u32, roots()[0]);
    assert_eq!(index, 0);
    let object = unsafe { &mut *object.cast::<Object>() };
    assert_eq!(object.padding[0], 0x18aa);
    assert_ne!(object.tail[1], 0); // object+0x60: our fixed item
    assert_eq!(object.model_name[0], 0);
    let source = unsafe { CStr::from_ptr(source) }.to_bytes_with_nul();
    assert!(source.len() <= 64);
    let failure = FAILURE.swap(0, Ordering::AcqRel);
    CALLS.fetch_add(1, Ordering::AcqRel);
    if failure != 2 {
        object.model_name[..source.len()].copy_from_slice(source);
    }
    if failure == 3 {
        object.tail[0] ^= 1; // Own unused byte, outside the permitted model field.
    }
    failure != 1
}
