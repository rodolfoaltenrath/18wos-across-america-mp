//! Own graphics fixture: thiscall forwarding, cdecl flush and retained lists.
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    System::{
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_READONLY,
            PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect, VirtualQuery,
        },
        Threading::GetCurrentThreadId,
    },
};
type Words<const N: usize> = Box<[AtomicU32; N]>;
fn words<const N: usize>() -> Words<N> {
    Box::new(std::array::from_fn(|_| AtomicU32::new(0)))
}
fn address<const N: usize>(w: &Words<N>) -> u32 {
    w.as_ptr() as u32
}
static PAGE: AtomicUsize = AtomicUsize::new(0);
static DEVICE: AtomicU32 = AtomicU32::new(0);
static LIST: AtomicU32 = AtomicU32::new(0);
static PROTOS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static OWNER: AtomicU32 = AtomicU32::new(0);
static INVALID: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU32 = AtomicU32::new(0);
static SAVED_SHIM: AtomicUsize = AtomicUsize::new(0);
static REPEATS: AtomicU32 = AtomicU32::new(1);
pub struct Fixture {
    page: *mut c_void,
    world: Words<16>,
    camera: Words<16>,
    heads: [Box<AtomicU32>; 2],
    _device: Words<4>,
    pool: Words<16>,
    _root: Words<8>,
    _outer: Words<3>,
    _bucket: Words<8>,
    list: Words<8>,
    nodes: [Words<3>; 2],
    protos: [Words<20>; 2],
    frame: u32,
}
impl Fixture {
    pub fn new() -> Self {
        let page =
            unsafe { VirtualAlloc(ptr::null(), 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        assert!(!page.is_null());
        unsafe {
            page.cast::<usize>().write(render as *const () as usize);
            page.cast::<usize>()
                .add(1)
                .write(flush as *const () as usize);
        }
        let mut old = 0;
        assert_ne!(
            unsafe { VirtualProtect(page, 4096, PAGE_READONLY, &mut old) },
            0
        );
        let (world, camera, device, pool, root, outer, bucket, list) = (
            words::<16>(),
            words::<16>(),
            words::<4>(),
            words::<16>(),
            words::<8>(),
            words::<3>(),
            words::<8>(),
            words::<8>(),
        );
        for m in [&world, &camera] {
            for i in [0, 5, 10, 15] {
                m[i].store(1f32.to_bits(), Ordering::Release);
            }
        }
        let nodes: [Words<3>; 2] = std::array::from_fn(|_| words());
        let protos: [Words<20>; 2] = std::array::from_fn(|_| words());
        pool[9].store(address(&root), Ordering::Release);
        root[1].store(address(&outer), Ordering::Release);
        outer[2].store(address(&bucket), Ordering::Release);
        bucket[1].store(address(&list), Ordering::Release);
        nodes[0][1].store(address(&nodes[1]), Ordering::Release);
        for i in 0..2 {
            nodes[i][2].store(address(&protos[i]), Ordering::Release);
            PROTOS[i].store(address(&protos[i]), Ordering::Release);
        }
        PAGE.store(page as usize, Ordering::Release);
        DEVICE.store(address(&device), Ordering::Release);
        LIST.store(address(&list), Ordering::Release);
        OWNER.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
        POOL_ADDRESS.store(address(&pool), Ordering::Release);
        let heads = [
            Box::new(AtomicU32::new(address(&world))),
            Box::new(AtomicU32::new(address(&camera))),
        ];
        Self {
            page,
            world,
            camera,
            heads,
            _device: device,
            pool,
            _root: root,
            _outer: outer,
            _bucket: bucket,
            list,
            nodes,
            protos,
            frame: 0,
        }
    }
    pub fn descriptors(&self) -> [u32; 6] {
        [
            self.page as u32,
            render as *const () as u32,
            self.page as u32 + 4,
            flush as *const () as u32,
            self.heads[0].as_ref() as *const AtomicU32 as u32,
            self.heads[1].as_ref() as *const AtomicU32 as u32,
        ]
    }
    pub fn frame(&mut self) {
        self.frame += 1;
        self.world[12].store((self.frame as f32).to_bits(), Ordering::Release);
        self.camera[12].store((-(self.frame as f32)).to_bits(), Ordering::Release);
        self.heads[0].store(
            if INVALID.load(Ordering::Acquire) {
                1
            } else {
                address(&self.world)
            },
            Ordering::Release,
        );
        self.list[1].store(address(&self.nodes[0]), Ordering::Release);
        for p in &self.protos {
            p[1].store(1, Ordering::Release);
        }
        let call: unsafe extern "C" fn() = unsafe {
            std::mem::transmute(
                AtomicUsize::from_ptr((self.page as *mut usize).add(1)).load(Ordering::Acquire),
            )
        };
        unsafe {
            SetLastError(0x1810);
            call();
            assert_eq!(GetLastError(), 0x1813);
        }
        assert_eq!(self.list[1].load(Ordering::Acquire), 0);
        assert_ne!(address(&self.pool), 0);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            assert_eq!(
                AtomicUsize::from_ptr(self.page.cast()).load(Ordering::Acquire),
                render as *const () as usize
            );
            assert_eq!(
                AtomicUsize::from_ptr(self.page.cast::<usize>().add(1)).load(Ordering::Acquire),
                flush as *const () as usize
            );
            let mut region: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
            assert_ne!(
                VirtualQuery(self.page, &mut region, std::mem::size_of_val(&region)),
                0
            );
            assert_eq!(region.Protect, PAGE_READONLY);
            assert_ne!(VirtualFree(self.page, 0, MEM_RELEASE), 0);
        }
    }
}
pub fn command(line: &str) -> bool {
    match line {
        "render-many" => REPEATS.store(100, Ordering::Release),
        "render-one" => REPEATS.store(1, Ordering::Release),
        "render-steal" | "render-return" => unsafe {
            let slot = (PAGE.load(Ordering::Acquire) as *mut usize).add(1);
            let mut old = 0;
            assert_ne!(VirtualProtect(slot.cast(), 4, PAGE_READWRITE, &mut old), 0);
            let word = AtomicUsize::from_ptr(slot);
            if line == "render-steal" {
                SAVED_SHIM.store(
                    word.swap(stolen_flush as *const () as usize, Ordering::AcqRel),
                    Ordering::Release,
                );
            } else {
                assert_eq!(
                    word.swap(SAVED_SHIM.load(Ordering::Acquire), Ordering::AcqRel),
                    stolen_flush as *const () as usize
                );
            }
            let mut ignored = 0;
            assert_ne!(VirtualProtect(slot.cast(), 4, old, &mut ignored), 0);
        },
        "render-invalid" => INVALID.store(true, Ordering::Release),
        "render-valid" => INVALID.store(false, Ordering::Release),
        "render-foreign" => unsafe {
            SetLastError(0x1811);
            let call: unsafe extern "thiscall" fn(*mut c_void, *mut c_void) = std::mem::transmute(
                AtomicUsize::from_ptr(PAGE.load(Ordering::Acquire) as *mut usize)
                    .load(Ordering::Acquire),
            );
            call(
                DEVICE.load(Ordering::Acquire) as *mut c_void,
                ptr::null_mut(),
            );
            assert_eq!(GetLastError(), 0x1812);
        },
        "render-stats" => {
            println!("{}", CALLS.load(Ordering::Acquire));
            return true;
        }
        _ => return false,
    }
    println!("armed");
    true
}
unsafe extern "thiscall" fn render(device: *mut c_void, pool: *mut c_void) {
    assert_eq!(device as u32, DEVICE.load(Ordering::Acquire));
    assert_eq!(unsafe { GetLastError() }, 0x1811);
    if !pool.is_null() {
        assert_eq!(
            unsafe { GetCurrentThreadId() },
            OWNER.load(Ordering::Acquire)
        );
    }
    CALLS.fetch_add(1, Ordering::AcqRel);
    unsafe { SetLastError(0x1812) };
}
unsafe extern "C" fn flush() {
    assert_eq!(
        unsafe { GetCurrentThreadId() },
        OWNER.load(Ordering::Acquire)
    );
    assert_eq!(unsafe { GetLastError() }, 0x1810);
    // The actual pool is read from the owned Fixture's public list graph owner.
    let pool = POOL_ADDRESS.load(Ordering::Acquire);
    let call: unsafe extern "thiscall" fn(*mut c_void, *mut c_void) = unsafe {
        std::mem::transmute(
            AtomicUsize::from_ptr(PAGE.load(Ordering::Acquire) as *mut usize)
                .load(Ordering::Acquire),
        )
    };
    for _ in 0..REPEATS.load(Ordering::Acquire) {
        unsafe {
            SetLastError(0x1811);
            call(
                DEVICE.load(Ordering::Acquire) as *mut c_void,
                pool as *mut c_void,
            );
            assert_eq!(GetLastError(), 0x1812);
        }
    }
    unsafe {
        (*(LIST.load(Ordering::Acquire) as *const AtomicU32).add(1)).store(0, Ordering::Release);
    }
    for p in &PROTOS {
        unsafe {
            (*(p.load(Ordering::Acquire) as *const AtomicU32).add(1)).store(0, Ordering::Release);
        }
    }
    unsafe { SetLastError(0x1813) };
}
static POOL_ADDRESS: AtomicU32 = AtomicU32::new(0);
unsafe extern "C" fn stolen_flush() {
    unsafe { flush() };
}
