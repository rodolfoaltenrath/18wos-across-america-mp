//! Own entity-draw fixture: thiscall virtual in a read-only page, a body chain
//! and a world stack laid out like the observed core globals.
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_READONLY,
        PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect, VirtualQuery,
    },
};
type Words<const N: usize> = Box<[AtomicU32; N]>;
fn words<const N: usize>() -> Words<N> {
    Box::new(std::array::from_fn(|_| AtomicU32::new(0)))
}
fn address<const N: usize>(w: &Words<N>) -> u32 {
    w.as_ptr() as u32
}
const SLOT: usize = 0x6c / 4;
/// Head word, then 16 entries of 0x84 bytes; the observed draw runs at depth 2.
const STACK_WORDS: usize = 1 + 16 * 33;
const DEPTH: u32 = 2;
static CAR: AtomicU32 = AtomicU32::new(0);
static CALLS: AtomicU32 = AtomicU32::new(0);
static PAGE: AtomicUsize = AtomicUsize::new(0);
static INVALID: AtomicBool = AtomicBool::new(false);
pub struct Fixture {
    page: *mut c_void,
    car: Words<128>,
    physics: Words<256>,
    body: Words<16>,
    world: Words<STACK_WORDS>,
    camera_head: Box<AtomicU32>,
    camera: Words<16>,
    path: Words<3>,
    viewport: Box<AtomicU32>,
    frame: u32,
}
impl Fixture {
    pub fn new() -> Self {
        let page =
            unsafe { VirtualAlloc(ptr::null(), 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        assert!(!page.is_null());
        unsafe {
            page.cast::<usize>()
                .add(SLOT)
                .write(draw as *const () as usize)
        };
        let mut old = 0;
        assert_ne!(
            unsafe { VirtualProtect(page, 4096, PAGE_READONLY, &mut old) },
            0
        );
        let (car, physics, body, world, camera, path) = (
            words::<128>(),
            words::<256>(),
            words::<16>(),
            words::<STACK_WORDS>(),
            words::<16>(),
            words::<3>(),
        );
        car[0].store(page as u32, Ordering::Release);
        // State byte at +0x1e9 inside the word at +0x1e8.
        car[0x1e8 / 4].store(7 << 8, Ordering::Release);
        car[0x1d0 / 4].store(address(&physics), Ordering::Release);
        physics[0x3e4 / 4].store(address(&body), Ordering::Release);
        body[0x14 / 4 + 3].store(1f32.to_bits(), Ordering::Release);
        for (i, v) in [2f32, 3.0].into_iter().enumerate() {
            body[0x30 / 4 + 1 + i].store(v.to_bits(), Ordering::Release);
        }
        let top = 1 + DEPTH as usize * 33;
        world[0].store(address(&world) + 4 + DEPTH * 0x84, Ordering::Release);
        for m in [&world[top..top + 16], &camera[..]] {
            for i in [0, 5, 10, 15] {
                m[i].store(1f32.to_bits(), Ordering::Release);
            }
        }
        path[0].store(0x1111, Ordering::Release);
        path[2].store(0x2222, Ordering::Release);
        CAR.store(address(&car), Ordering::Release);
        PAGE.store(page as usize, Ordering::Release);
        Self {
            page,
            camera_head: Box::new(AtomicU32::new(address(&camera))),
            car,
            physics,
            body,
            world,
            camera,
            path,
            viewport: Box::new(AtomicU32::new(0x3333)),
            frame: 0,
        }
    }
    pub fn descriptors(&self) -> [u32; 6] {
        [
            self.page as u32 + SLOT as u32 * 4,
            draw as *const () as u32,
            address(&self.world),
            self.camera_head.as_ref() as *const AtomicU32 as u32,
            &self.path[2] as *const AtomicU32 as u32,
            self.viewport.as_ref() as *const AtomicU32 as u32,
        ]
    }
    pub fn car(&self) -> u32 {
        address(&self.car)
    }
    pub fn frame(&mut self) {
        self.frame += 1;
        self.body[0x30 / 4].store((self.frame as f32).to_bits(), Ordering::Release);
        self.physics[0x3e4 / 4].store(
            if INVALID.load(Ordering::Acquire) {
                1
            } else {
                address(&self.body)
            },
            Ordering::Release,
        );
        let top = 1 + DEPTH as usize * 33;
        self.world[top + 12].store((self.frame as f32).to_bits(), Ordering::Release);
        self.camera[14].store((-(self.frame as f32)).to_bits(), Ordering::Release);
        unsafe { call(self.car()) };
        assert_eq!(
            self.world[0].load(Ordering::Acquire),
            address(&self.world) + 4 + DEPTH * 0x84
        );
    }
}
unsafe fn call(car: u32) {
    let slot = (PAGE.load(Ordering::Acquire) as *mut usize).wrapping_add(SLOT);
    let function: unsafe extern "thiscall" fn(*mut c_void) =
        unsafe { std::mem::transmute(AtomicUsize::from_ptr(slot).load(Ordering::Acquire)) };
    unsafe {
        SetLastError(0x1820);
        function(car as *mut c_void);
        assert_eq!(GetLastError(), 0x1821);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            assert_eq!(
                AtomicUsize::from_ptr(self.page.cast::<usize>().add(SLOT)).load(Ordering::Acquire),
                draw as *const () as usize
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
        "draw-invalid" => INVALID.store(true, Ordering::Release),
        "draw-valid" => INVALID.store(false, Ordering::Release),
        "draw-foreign" => unsafe { call(CAR.load(Ordering::Acquire)) },
        "draw-stats" => {
            println!("{}", CALLS.load(Ordering::Acquire));
            return true;
        }
        _ => return false,
    }
    println!("armed");
    true
}
unsafe extern "thiscall" fn draw(this: *mut c_void) {
    assert_eq!(this as u32, CAR.load(Ordering::Acquire));
    assert_eq!(unsafe { GetLastError() }, 0x1820);
    CALLS.fetch_add(1, Ordering::AcqRel);
    unsafe { SetLastError(0x1821) };
}
