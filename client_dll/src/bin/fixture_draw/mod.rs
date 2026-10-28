//! Own entity-draw fixture: thiscall virtual in a read-only page, a body chain
//! and a world stack laid out like the observed core globals. Also provides
//! cdecl gfx push/pop/translate/qrotate and a psm_draw that queues the item
//! (+0x58 access) until the fixture's flush consumes it.
mod lifecycle;
use std::{
    ffi::c_void,
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
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
const SLOT: usize = 0x6c / 4;
/// Head word, then 16 entries of 0x84 bytes (33 words).
const STACK_WORDS: usize = 1 + 16 * 33;
static DEPTH: AtomicU32 = AtomicU32::new(2);
static STATE: AtomicU32 = AtomicU32::new(7);
static CAR: AtomicU32 = AtomicU32::new(0);
/// Two player entities with the same chain; switching simulates a reload.
static CARS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static CALLS: AtomicU32 = AtomicU32::new(0);
static PAGE: AtomicUsize = AtomicUsize::new(0);
static WORLD: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
static INVALID: AtomicBool = AtomicBool::new(false);
static RETAIN: AtomicBool = AtomicBool::new(false);
static SHADER_CALLS: AtomicU32 = AtomicU32::new(0);
static LIGHT_CALLS: AtomicU32 = AtomicU32::new(0);
static SCENE: AtomicU32 = AtomicU32::new(0);
static LIGHT_INVALID: AtomicBool = AtomicBool::new(false);
#[derive(Default)]
struct Drawn {
    calls: u32,
    item: u32,
    depth: u32,
    position: [f32; 3],
    quaternion: [f32; 4],
    pending: Vec<u32>,
}
static DRAWN: Mutex<Drawn> = Mutex::new(Drawn {
    calls: 0,
    item: 0,
    depth: 0,
    position: [0.0; 3],
    quaternion: [0.0; 4],
    pending: Vec::new(),
});
static LAST: Mutex<([f32; 3], [f32; 4])> = Mutex::new(([0.0; 3], [0.0; 4]));
pub struct Fixture {
    page: *mut c_void,
    cars: [Words<256>; 2],
    _blocks: lifecycle::Blocks,
    _scene: Words<1>,
    physics: Words<256>,
    body: Words<16>,
    world: Words<STACK_WORDS>,
    camera_head: Box<AtomicU32>,
    camera: Words<16>,
    path: Words<3>,
    viewport: Box<AtomicU32>,
    frame: u32,
}
fn top(depth: u32) -> usize {
    1 + depth as usize * 33
}
impl Fixture {
    pub fn new() -> Self {
        let page =
            unsafe { VirtualAlloc(ptr::null(), 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        assert!(!page.is_null());
        unsafe {
            lifecycle::setup(page);
            page.cast::<usize>()
                .add(SLOT)
                .write(draw as *const () as usize);
            page.cast::<usize>()
                .add(0x48 / 4)
                .write(get_light as *const () as usize);
        };
        let mut old = 0;
        assert_ne!(
            unsafe { VirtualProtect(page, 4096, PAGE_READONLY, &mut old) },
            0
        );
        let (cars, physics, body, world, camera, path) = (
            [words::<256>(), words::<256>()],
            words::<256>(),
            words::<16>(),
            words::<STACK_WORDS>(),
            words::<16>(),
            words::<3>(),
        );
        for (car, slot) in cars.iter().zip(&CARS) {
            car[0].store(page as u32, Ordering::Release);
            car[0x1d0 / 4].store(address(&physics), Ordering::Release);
            car[0x200 / 4].store(0x11110000, Ordering::Release);
            car[0x300 / 4].store(3, Ordering::Release);
            slot.store(address(car), Ordering::Release);
        }
        let scene = words::<1>();
        scene[0].store(page as u32, Ordering::Release);
        SCENE.store(address(&scene), Ordering::Release);
        for car in &cars {
            car[0x84 / 4].store(address(&scene), Ordering::Release);
        }
        physics[0x3e4 / 4].store(address(&body), Ordering::Release);
        body[0x14 / 4 + 3].store(1f32.to_bits(), Ordering::Release);
        for (i, v) in [2f32, 3.0].into_iter().enumerate() {
            body[0x30 / 4 + 1 + i].store(v.to_bits(), Ordering::Release);
        }
        for depth in 0..3 {
            for i in [0, 5, 10, 15] {
                world[top(depth) + i].store(1f32.to_bits(), Ordering::Release);
            }
        }
        for i in [0, 5, 10, 15] {
            camera[i].store(1f32.to_bits(), Ordering::Release);
        }
        path[0].store(0x1111, Ordering::Release);
        path[2].store(0x2222, Ordering::Release);
        CAR.store(address(&cars[0]), Ordering::Release);
        PAGE.store(page as usize, Ordering::Release);
        WORLD.store(address(&world), Ordering::Release);
        OWNER.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
        Self {
            _blocks: lifecycle::Blocks::new(&cars),
            page,
            camera_head: Box::new(AtomicU32::new(address(&camera))),
            cars,
            _scene: scene,
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
    pub fn transition_descriptors(&self) -> [u32; 6] {
        lifecycle::descriptors(self.page)
    }
    pub fn block_descriptors(&self) -> [u32; 6] {
        lifecycle::block_descriptors(self.page)
    }
    pub fn command_descriptors(&self) -> [u32; 6] {
        lifecycle::command_descriptors(self.page)
    }
    /// psm_draw, gfx_push, gfx_pop, gfx_translate, gfx_qrotate, psm_set_shader.
    pub fn visual_descriptors() -> [u32; 8] {
        [
            psm_draw as *const () as u32,
            push as *const () as u32,
            pop as *const () as u32,
            translate as *const () as u32,
            qrotate as *const () as u32,
            set_shader as *const () as u32,
            set_light as *const () as u32,
            get_light as *const () as u32,
        ]
    }
    pub fn car(&self) -> u32 {
        address(&self.cars[0])
    }
    pub fn frame(&mut self) {
        lifecycle::frame(CAR.load(Ordering::Acquire));
        self.frame += 1;
        let depth = DEPTH.load(Ordering::Acquire);
        for car in &self.cars {
            car[0x1e8 / 4].store(STATE.load(Ordering::Acquire) << 8, Ordering::Release);
            car[0x84 / 4].store(
                if LIGHT_INVALID.load(Ordering::Acquire) {
                    0
                } else {
                    SCENE.load(Ordering::Acquire)
                },
                Ordering::Release,
            );
        }
        self.body[0x30 / 4].store((self.frame as f32).to_bits(), Ordering::Release);
        self.physics[0x3e4 / 4].store(
            if INVALID.load(Ordering::Acquire) {
                1
            } else {
                address(&self.body)
            },
            Ordering::Release,
        );
        let head = address(&self.world) + 4 + depth * 0x84;
        self.world[0].store(head, Ordering::Release);
        self.world[top(depth) + 12].store((self.frame as f32).to_bits(), Ordering::Release);
        self.camera[14].store((-(self.frame as f32)).to_bits(), Ordering::Release);
        unsafe { call(CAR.load(Ordering::Acquire)) };
        assert_eq!(self.world[0].load(Ordering::Acquire), head);
    }
    /// Runs after the render fixture's flush: queued draws release their access.
    pub fn consume(&mut self) {
        if RETAIN.load(Ordering::Acquire) {
            return;
        }
        for item in std::mem::take(&mut DRAWN.lock().unwrap().pending) {
            unsafe { *(item as *mut u32).add(0x58 / 4) -= 1 };
        }
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
        lifecycle::check_restored(self.page);
        assert!(DRAWN.lock().unwrap().pending.is_empty() || RETAIN.load(Ordering::Acquire));
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
    if lifecycle::command(line, CAR.load(Ordering::Acquire)) {
        return true;
    }
    match line {
        "draw-invalid" => INVALID.store(true, Ordering::Release),
        "draw-light-invalid" => LIGHT_INVALID.store(true, Ordering::Release),
        "draw-light-valid" => LIGHT_INVALID.store(false, Ordering::Release),
        "draw-valid" => INVALID.store(false, Ordering::Release),
        "draw-foreign" => unsafe { call(CAR.load(Ordering::Acquire)) },
        "draw-respawn" => {
            let next = if CAR.load(Ordering::Acquire) == CARS[0].load(Ordering::Acquire) {
                1
            } else {
                0
            };
            CAR.store(CARS[next].load(Ordering::Acquire), Ordering::Release);
        }
        "draw-shallow" => DEPTH.store(0, Ordering::Release),
        "draw-deep" => DEPTH.store(2, Ordering::Release),
        "draw-interior" => STATE.store(4, Ordering::Release),
        "draw-exterior" => STATE.store(7, Ordering::Release),
        "visual-retain" => RETAIN.store(true, Ordering::Release),
        "visual-consume" => RETAIN.store(false, Ordering::Release),
        "draw-stats" => {
            println!("{}", CALLS.load(Ordering::Acquire));
            return true;
        }
        "visual-last" => {
            let d = DRAWN.lock().unwrap();
            println!(
                "{}",
                serde_json::json!({"calls":d.calls,"item":d.item,"depth":d.depth,"position":d.position,"quaternion":d.quaternion,"pending":d.pending.len(),"shader_calls":SHADER_CALLS.load(Ordering::Acquire),"light_calls":LIGHT_CALLS.load(Ordering::Acquire)})
            );
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
fn owner() {
    assert_eq!(
        unsafe { GetCurrentThreadId() },
        OWNER.load(Ordering::Acquire)
    );
}
fn head() -> &'static AtomicU32 {
    unsafe { &*(WORLD.load(Ordering::Acquire) as *const AtomicU32) }
}
fn depth() -> u32 {
    (head().load(Ordering::Acquire) - WORLD.load(Ordering::Acquire) - 4) / 0x84
}
unsafe extern "C" fn push() {
    owner();
    assert!(depth() < 15);
    head().fetch_add(0x84, Ordering::AcqRel);
}
unsafe extern "C" fn pop() {
    owner();
    assert!(depth() > 0);
    head().fetch_sub(0x84, Ordering::AcqRel);
}
unsafe extern "C" fn translate(v: *const f32) {
    owner();
    LAST.lock().unwrap().0 = unsafe { *v.cast::<[f32; 3]>() };
}
unsafe extern "C" fn qrotate(q: *const f32) {
    owner();
    LAST.lock().unwrap().1 = unsafe { *q.cast::<[f32; 4]>() };
}
unsafe extern "C" fn psm_draw(item: *mut c_void, cull: u32, color: u32) {
    owner();
    assert_eq!((cull, color), (1, u32::MAX));
    if SHADER_CALLS.load(Ordering::Acquire) != 0 {
        let data = unsafe { *item.cast::<u32>().add(2) as *const u32 };
        let callback: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
            unsafe { std::mem::transmute(*data.add(0x60 / 4) as usize) };
        assert_eq!(
            unsafe { callback(*data.add(0x5c / 4) as *mut c_void) } as usize,
            0x1822
        );
    }
    let (position, quaternion) = *LAST.lock().unwrap();
    let mut d = DRAWN.lock().unwrap();
    d.calls += 1;
    d.item = item as u32;
    d.depth = depth();
    d.position = position;
    d.quaternion = quaternion;
    unsafe { *item.cast::<u32>().add(0x58 / 4) += 1 };
    d.pending.push(item as u32);
}
unsafe extern "C" fn set_shader(
    item: *mut c_void,
    part: u32,
    path: *const std::ffi::c_char,
) -> *mut c_void {
    owner();
    assert!(!item.is_null() && part < 3);
    let path = unsafe { std::ffi::CStr::from_ptr(path) };
    assert_eq!(path.to_bytes(), b"models/fixture/truck.shd");
    SHADER_CALLS.fetch_add(1, Ordering::AcqRel);
    // A borrowed, non-null item handle is all the caller inspects.
    item
}
unsafe extern "C" fn set_light(
    item: *mut c_void,
    callback: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
    context: *mut c_void,
) {
    owner();
    let data = unsafe { *item.cast::<u32>().add(2) as *mut u32 };
    unsafe {
        *data.add(0x60 / 4) = callback.map_or(0, |f| f as usize as u32);
        *data.add(0x5c / 4) = context as u32;
    }
}
unsafe extern "C" fn get_light(context: *mut c_void) -> *mut c_void {
    owner();
    assert_eq!(context as u32, SCENE.load(Ordering::Acquire));
    LIGHT_CALLS.fetch_add(1, Ordering::AcqRel);
    0x1822usize as *mut c_void
}
