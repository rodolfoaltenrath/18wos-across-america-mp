//! Private factory/item/model/resource diagnostics on the profiled outer loop.
//! Engine allocation and destruction stay paired; nothing enters the world.
use crate::{
    api::{self, EngineControl},
    engine_contract::{Counters, word},
    inspection::process::{Module, Process},
    profile,
};
use std::{ffi::c_void, path::Path, ptr};
use windows_sys::Win32::System::{
    LibraryLoader::{GetModuleHandleW, GetProcAddress},
    Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, VirtualQuery,
    },
    Threading::{GetCurrentProcessId, GetCurrentThreadId},
};
pub(super) type Create = unsafe extern "C" fn(*const c_void, bool, *mut c_void) -> *mut c_void;
type Destroy = unsafe extern "C" fn(*mut c_void, *mut c_void);
pub(super) struct Factory {
    pub(super) record: u32,
    pub(super) create: u32,
    pub(super) destroy: u32,
    pub(super) heads: [u32; 2],
    pub(super) vtable: u32,
    pub(super) constructor: u32,
    pub(super) destructor: u32,
    pub(super) size: u32,
}
pub(super) fn read(process: &Process, at: u32, out: &mut [u8]) -> Result<(), String> {
    process.read(at.into(), out).map_err(|e| e.to_string())
}
pub(super) fn executable(at: u32, owner: &Module) -> Result<(), String> {
    if u64::from(at) < owner.base || u64::from(at) - owner.base >= owner.size {
        return Err("factory callback outside expected module".into());
    }
    let mut region: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        VirtualQuery(
            at as usize as *const _,
            &mut region,
            std::mem::size_of_val(&region),
        )
    } == 0
        || region.State != MEM_COMMIT
        || region.Protect & PAGE_GUARD != 0
        || !matches!(
            region.Protect & 0xff,
            PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
        )
    {
        return Err("factory callback is not executable".into());
    }
    Ok(())
}
pub(super) fn add(module: &Module, rva: u32) -> Result<u32, String> {
    if u64::from(rva) >= module.size {
        return Err("profile RVA outside module".into());
    }
    u32::try_from(module.base + u64::from(rva)).map_err(|e| e.to_string())
}
pub(super) fn game(process: &Process) -> Result<Factory, String> {
    game_type(process, profile::CAR)
}
pub(super) fn game_type(process: &Process, kind: profile::EntityType) -> Result<Factory, String> {
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} not loaded"))
    };
    let exe = get("prism3d.exe")?;
    let engine = get("p3shared.dll")?;
    let game = get("game.dll")?;
    profile::verify_executable(Path::new(&exe.path))?;
    profile::verify_engine(Path::new(&engine.path))?;
    profile::verify_module(Path::new(&game.path))?;
    let records = crate::inspection::entities::registered(process).map_err(|e| e.to_string())?;
    let record = records
        .iter()
        .find(|r| r.name == kind.name)
        .ok_or("profiled type not registered")?;
    if record.address != add(game, kind.record)?
        || record.false_size != kind.size
        || record.false_constructor != add(game, kind.constructor)?
        || record.false_destructor != add(game, kind.destructor)?
        || record.true_constructor != 0
        || record.true_destructor != 0
        || record.true_size != 0
    {
        return Err("entity registration differs from the verified profile".into());
    }
    if kind.name == "decals.pmd" {
        for (slot, expected) in [(0x1665e4, 0xcedf0), (0x1665e0, 0xcee00)] {
            if word(&mut |a, b| read(process, a, b), add(game, slot)?)? != add(game, expected)? {
                return Err("PMD module lock callbacks differ from profile".into());
            }
        }
    }
    let create = add(engine, profile::CREATE_FACTORY)?;
    let destroy = add(engine, profile::DESTROY_FACTORY)?;
    // Resolve the exports in the loaded module as well as checking the file hash.
    for (name, expected) in [
        (
            c"?entity_create@@YAPAVentity_base_t@@PBUentity_reg_t@@_NPAX@Z",
            create,
        ),
        (c"?entity_destroy@@YAXPAVentity_base_t@@PAX@Z", destroy),
    ] {
        let found = unsafe { GetProcAddress(engine.base as usize as _, name.as_ptr().cast()) };
        if found.map(|f| f as usize as u32) != Some(expected) {
            return Err("entity factory export differs from profile".into());
        }
        executable(expected, engine)?;
    }
    executable(record.false_constructor, game)?;
    executable(record.false_destructor, game)?;
    Ok(Factory {
        record: record.address,
        create,
        destroy,
        heads: [
            add(engine, profile::ENGINE_CONTEXT)?,
            add(game, profile::GAME_CONTEXT)?,
        ],
        vtable: add(game, kind.vtable)?,
        constructor: record.false_constructor,
        destructor: record.false_destructor,
        size: record.false_size,
    })
}
pub(super) fn fixture(process: &Process, block: &EngineControl) -> Result<Factory, String> {
    let modules = process.modules().map_err(|e| e.to_string())?;
    let base = unsafe { GetModuleHandleW(ptr::null()) } as usize as u64;
    let module = modules
        .iter()
        .find(|m| m.base == base && m.name.eq_ignore_ascii_case("engine_fixture.exe"))
        .ok_or("factory fixture requires our engine_fixture.exe")?;
    let [record, create, destroy, engine_head, game_head, vtable] = block.fixture;
    executable(create, module)?;
    executable(destroy, module)?;
    let mut bytes = [0u8; 32];
    read(process, record, &mut bytes)?;
    let words: [u32; 8] =
        std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    if words[5] < 0x5c || words[5] > 0x10000 {
        return Err("fixture object size invalid".into());
    }
    executable(words[6], module)?;
    executable(words[7], module)?;
    Ok(Factory {
        record,
        create,
        destroy,
        heads: [engine_head, game_head],
        vtable,
        constructor: words[6],
        destructor: words[7],
        size: words[5],
    })
}
struct PrivateEntity {
    pointer: *mut c_void,
    destroy: Destroy,
}
impl Drop for PrivateEntity {
    fn drop(&mut self) {
        // Only the engine factory destroys an engine allocation; Rust never frees it.
        unsafe {
            (self.destroy)(self.pointer, ptr::null_mut());
        }
    }
}
pub fn run(mut block: EngineControl, is_fixture: bool) -> Result<EngineControl, String> {
    if matches!(
        block.command,
        api::VISUAL_SHOW | api::VISUAL_SHOW_FIXTURE | api::VISUAL_RELEASE
    ) {
        return crate::engine_visual::run(block, is_fixture);
    }
    if matches!(
        block.command,
        api::PRIVATE_INSTANCE | api::PRIVATE_INSTANCE_FIXTURE
    ) {
        return crate::engine_instance::run(block, is_fixture);
    }
    if matches!(block.command, api::PRIVATE_INFO | api::PRIVATE_INFO_FIXTURE) {
        return crate::engine_info::run(block, is_fixture);
    }
    if matches!(
        block.command,
        api::PRIVATE_ITEM
            | api::PRIVATE_ITEM_FIXTURE
            | api::PRIVATE_MODEL
            | api::PRIVATE_MODEL_FIXTURE
    ) {
        return crate::engine_item::run(block, is_fixture);
    }
    block.executed_thread = unsafe { GetCurrentThreadId() };
    if block.command == api::PHASE_PROBE {
        return Ok(block);
    }
    if !(1..=16).contains(&block.cycles) {
        return Err("private cycles must be 1..=16".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let factory = match block.command {
        api::PRIVATE_CYCLE if !is_fixture => game(&process)?,
        api::PRIVATE_FIXTURE if is_fixture => fixture(&process, &block)?,
        _ => return Err("private factory mode does not match the attached pump".into()),
    };
    let mut verified = 0;
    let before_position = if !is_fixture {
        Some(
            crate::memory::game_position(&mut verified)
                .map_err(|e| format!("loaded vehicle required: {e:?}"))?,
        )
    } else {
        None
    };
    let create: Create = unsafe { std::mem::transmute(factory.create as usize) };
    let destroy: Destroy = unsafe { std::mem::transmute(factory.destroy as usize) };
    let requested = block.cycles;
    block.cycles = 0;
    for _ in 0..requested {
        let counters = Counters::capture(factory.heads, &mut |at, out| read(&process, at, out))?;
        block.count_before = counters.first();
        // false selects the profiled branch; NULL storage gives allocation ownership to the engine.
        let pointer =
            unsafe { create(factory.record as usize as *const _, false, ptr::null_mut()) };
        if pointer.is_null() {
            return Err("factory returned null; no object to destroy".into());
        }
        let object = PrivateEntity { pointer, destroy };
        let at = u32::try_from(pointer as usize).map_err(|e| e.to_string())?;
        let check: Result<(), String> = (|| {
            block.object_vtable = word(&mut |a, b| read(&process, a, b), at)?;
            block.object_destructor = word(
                &mut |a, b| read(&process, a, b),
                at.checked_add(0x58).ok_or("object overflow")?,
            )?;
            if block.object_vtable != factory.vtable
                || block.object_destructor != factory.destructor
            {
                return Err("private object header differs from contract".into());
            }
            counters.check(true, &mut |a, b| read(&process, a, b))?;
            block.count_created = counters.expected_created();
            Ok(())
        })();
        drop(object); // Also runs on every Rust error/panic above. No pointer escapes this scope.
        let cleanup = counters.check(false, &mut |a, b| read(&process, a, b));
        if let Err(e) = cleanup {
            return Err(format!(
                "private object destruction left module references: {e}"
            ));
        }
        check?;
        block.count_after = counters.first();
        block.cycles += 1;
    }
    if let Some(position) = before_position {
        let after = crate::memory::game_position(&mut verified)
            .map_err(|e| format!("vehicle state changed after probe: {e:?}"))?;
        if after != position {
            return Err("local position changed during probe; lifecycle completed, motion check inconclusive".into());
        }
    }
    block.object_size = factory.size;
    block.constructor = factory.constructor;
    block.destructor = factory.destructor;
    Ok(block)
}
