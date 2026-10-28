//! Own x86 fixture: two module-lock contexts, C factories, thiscall virtuals.
//! Every allocation is created/destroyed by this host, across the DLL boundary.
use std::{
    ffi::c_void,
    ptr,
    sync::{
        OnceLock,
        atomic::{AtomicU32, Ordering},
    },
};
#[repr(C)]
struct Context {
    prefix: [u32; 2],
    refs: AtomicU32,
    parent: AtomicU32,
}
static ROOT: Context = Context {
    prefix: [0; 2],
    refs: AtomicU32::new(7),
    parent: AtomicU32::new(0),
};
static CHILD: Context = Context {
    prefix: [0; 2],
    refs: AtomicU32::new(0),
    parent: AtomicU32::new(0),
};
static TABLE: OnceLock<[usize; 5]> = OnceLock::new();
static CREATED: AtomicU32 = AtomicU32::new(0);
static FREED: AtomicU32 = AtomicU32::new(0);
static POST: AtomicU32 = AtomicU32::new(0);
static PRE: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
#[repr(C)]
struct Object {
    table: u32,
    padding: [u32; 21],
    destructor: u32,
}
pub struct Fixture {
    record: Box<[u32; 8]>,
    root_head: Box<u32>,
    game_head: Box<u32>,
}
impl Fixture {
    pub fn new() -> Self {
        OWNER.store(
            unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
            Ordering::Release,
        );
        assert_eq!(std::mem::offset_of!(Object, destructor), 0x58);
        CHILD
            .parent
            .store(&ROOT as *const _ as u32, Ordering::Release);
        TABLE.get_or_init(|| {
            [
                no_op as *const () as usize,
                no_op as *const () as usize,
                deleting_destructor as *const () as usize,
                pre_destructor as *const () as usize,
                post_constructor as *const () as usize,
            ]
        });
        Self {
            record: Box::new([
                c"fixture.car".as_ptr() as u32,
                0,
                0,
                0,
                0,
                std::mem::size_of::<Object>() as u32,
                construct as *const () as u32,
                destruct as *const () as u32,
            ]),
            root_head: Box::new(&ROOT as *const _ as u32),
            game_head: Box::new(&CHILD as *const _ as u32),
        }
    }
    pub fn bindings(&self) -> [u32; 6] {
        [
            self.record.as_ptr() as u32,
            create as *const () as u32,
            destroy as *const () as u32,
            self.root_head.as_ref() as *const _ as u32,
            self.game_head.as_ref() as *const _ as u32,
            TABLE.get().unwrap().as_ptr() as u32,
        ]
    }
    pub fn assert_clean(&self) {
        assert_eq!(ROOT.refs.load(Ordering::Acquire), 7);
        assert_eq!(CHILD.refs.load(Ordering::Acquire), 0);
        assert_eq!(
            CREATED.load(Ordering::Acquire),
            FREED.load(Ordering::Acquire)
        );
        assert_eq!(POST.load(Ordering::Acquire), PRE.load(Ordering::Acquire));
    }
}
pub fn stats() -> serde_json::Value {
    serde_json::json!({"created": CREATED.load(Ordering::Acquire), "freed": FREED.load(Ordering::Acquire),
        "post": POST.load(Ordering::Acquire), "pre": PRE.load(Ordering::Acquire),
        "root": ROOT.refs.load(Ordering::Acquire), "game": CHILD.refs.load(Ordering::Acquire)})
}
unsafe extern "thiscall" fn no_op(_: *mut Object) {}
unsafe extern "thiscall" fn post_constructor(object: *mut Object) {
    unsafe {
        (*object).padding[0] = 0x18aa;
    }
    POST.fetch_add(1, Ordering::AcqRel);
}
unsafe extern "thiscall" fn pre_destructor(object: *mut Object) {
    assert_eq!(unsafe { (*object).padding[0] }, 0x18aa);
    PRE.fetch_add(1, Ordering::AcqRel);
}
unsafe extern "thiscall" fn deleting_destructor(object: *mut Object, flags: u32) {
    assert_eq!(flags, 1);
    unsafe {
        drop(Box::from_raw(object));
    }
    FREED.fetch_add(1, Ordering::AcqRel);
}
unsafe extern "C" fn construct(storage: *mut c_void) -> *mut c_void {
    assert!(storage.is_null());
    CHILD.refs.fetch_add(1, Ordering::AcqRel);
    ROOT.refs.fetch_add(1, Ordering::AcqRel);
    CREATED.fetch_add(1, Ordering::AcqRel);
    Box::into_raw(Box::new(Object {
        table: TABLE.get().unwrap().as_ptr() as u32,
        padding: [0; 21],
        destructor: 0,
    }))
    .cast()
}
unsafe extern "C" fn destruct(object: *mut c_void, storage: *mut c_void) {
    assert!(storage.is_null());
    let table = unsafe { (*(object as *const Object)).table } as *const usize;
    let method: unsafe extern "thiscall" fn(*mut Object, u32) =
        unsafe { std::mem::transmute(*table.add(2)) };
    unsafe {
        method(object.cast(), 1);
    }
    CHILD.refs.fetch_sub(1, Ordering::AcqRel);
    ROOT.refs.fetch_sub(1, Ordering::AcqRel);
}
unsafe extern "C" fn create(
    record: *const c_void,
    variant: bool,
    storage: *mut c_void,
) -> *mut c_void {
    assert_eq!(
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        OWNER.load(Ordering::Acquire)
    );
    assert!(!variant);
    assert!(storage.is_null());
    ROOT.refs.fetch_add(1, Ordering::AcqRel);
    let record = record.cast::<u32>();
    let ctor: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
        unsafe { std::mem::transmute(*record.add(6) as usize) };
    let object = unsafe { ctor(ptr::null_mut()) } as *mut Object;
    unsafe {
        (*object).destructor = *record.add(7);
    }
    let table = unsafe { (*object).table } as *const usize;
    let post: unsafe extern "thiscall" fn(*mut Object) =
        unsafe { std::mem::transmute(*table.add(4)) };
    unsafe {
        post(object);
    }
    object.cast()
}
unsafe extern "C" fn destroy(object: *mut c_void, storage: *mut c_void) {
    assert_eq!(
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        OWNER.load(Ordering::Acquire)
    );
    assert!(storage.is_null());
    let table = unsafe { (*(object as *const Object)).table } as *const usize;
    let pre: unsafe extern "thiscall" fn(*mut Object) =
        unsafe { std::mem::transmute(*table.add(3)) };
    unsafe {
        pre(object.cast());
    }
    let dtor: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { std::mem::transmute((*(object as *const Object)).destructor as usize) };
    unsafe {
        dtor(object, ptr::null_mut());
    }
    ROOT.refs.fetch_sub(1, Ordering::AcqRel);
}
