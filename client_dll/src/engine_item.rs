//! Private c_item_t storage ownership. No world/process insertion or init calls.
use crate::{
    addresses::Address,
    api::{self, EngineControl},
    engine_contract::{Counters, word},
    engine_lifecycle::{self as actor, Create, Factory, add, executable, read},
    inspection::process::Process,
};
use std::ffi::c_void;
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
type Alloc = unsafe extern "thiscall" fn(*mut c_void) -> *mut c_void;
type Fix = unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void;
type Free = unsafe extern "C" fn(*mut c_void);
type Release = unsafe extern "C" fn(*mut c_void) -> u32;
struct Hooks {
    alloc: Alloc,
    fix: Fix,
    free: Free,
    release: Release,
    ops: u32,
    context: u32,
}
fn words<const N: usize>(process: &Process, at: u32) -> Result<[u32; N], String> {
    let mut out = [0; N];
    for (i, value) in out.iter_mut().enumerate() {
        *value = word(
            &mut |a, b| read(process, a, b),
            at.checked_add(i as u32 * 4).ok_or("item overflow")?,
        )?;
    }
    Ok(out)
}
fn resolve(process: &Process, expression: &str) -> Result<u32, String> {
    let modules = process.modules().map_err(|e| e.to_string())?;
    let expression: Address = expression.parse()?;
    let at = expression.resolve(
        u32::MAX.into(),
        |name| {
            modules
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| m.base)
                .ok_or("module missing".into())
        },
        |at| process.pointer(at).map_err(|e| e.to_string()),
    )?;
    u32::try_from(at).map_err(|e| e.to_string())
}
fn bindings(
    process: &Process,
    block: &EngineControl,
    fixture: bool,
) -> Result<(Factory, Hooks), String> {
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    let model = matches!(
        block.command,
        api::PRIVATE_MODEL | api::PRIVATE_MODEL_FIXTURE
    );
    let (factory, addresses) = if fixture
        && matches!(
            block.command,
            api::PRIVATE_ITEM_FIXTURE | api::PRIVATE_MODEL_FIXTURE
        ) {
        let owner = get("engine_fixture.exe")?;
        let values = words::<12>(process, block.fixture[0])?;
        let mut normalized = *block;
        normalized.fixture.copy_from_slice(&values[..6]);
        let factory = actor::fixture(process, &normalized)?;
        for callback in &values[6..10] {
            executable(*callback, owner)?;
        }
        (factory, values[6..].try_into().unwrap())
    } else if !fixture && matches!(block.command, api::PRIVATE_ITEM | api::PRIVATE_MODEL) {
        // actor::game verifies all three images, exact record, factories and ABI branch.
        let kind = if model {
            crate::profile::PMD
        } else {
            crate::profile::entity_type(api::text(&block.name)?)?
        };
        let factory = actor::game_type(process, kind)?;
        let engine = get("p3shared.dll")?;
        let exe = get("prism3d.exe")?;
        let addresses = [
            add(engine, 0xc9a0)?,
            add(exe, 0x13420)?,
            add(exe, 0x13440)?,
            add(exe, 0x13520)?,
            add(engine, 0x4523c)?,
            resolve(process, "game.dll+0x160df0->0x14c->0x0")?,
        ];
        executable(addresses[0], engine)?;
        for callback in &addresses[1..4] {
            executable(*callback, exe)?;
        }
        // Runtime-populated operation table; destruction must reach the paired engine path.
        let ops = words::<9>(process, addresses[4])?;
        let expected = [
            add(exe, 0x10210)?,
            add(exe, 0x10180)?,
            add(engine, 0xa9a0)?,
            add(engine, 0xaa40)?,
            add(exe, 0x10360)?,
            add(exe, 0x10370)?,
            add(exe, 0x10390)?,
            0,
            2,
        ];
        if ops != expected {
            return Err("container operations differ from profile".into());
        }
        (factory, addresses)
    } else {
        return Err("private item mode does not match attached pump".into());
    };
    if factory.size < 0x88 || addresses[5] == 0 {
        return Err("invalid item actor/context".into());
    }
    let hooks = Hooks {
        alloc: unsafe { std::mem::transmute::<usize, Alloc>(addresses[0] as usize) },
        fix: unsafe { std::mem::transmute::<usize, Fix>(addresses[1] as usize) },
        free: unsafe { std::mem::transmute::<usize, Free>(addresses[2] as usize) },
        release: unsafe { std::mem::transmute::<usize, Release>(addresses[3] as usize) },
        ops: addresses[4],
        context: addresses[5],
    };
    Ok((factory, hooks))
}
struct Item<'a> {
    pointer: *mut c_void,
    hooks: &'a Hooks,
    storage: bool,
    fixed: bool,
    constructed: bool,
}
impl Drop for Item<'_> {
    fn drop(&mut self) {
        // The item and fix are exclusively ours, on the outer engine loop.
        // The item never escapes to a process list, world, or another owner.
        unsafe {
            if self.fixed {
                let fixes = (self.pointer as *mut u32).add(0x58 / 4);
                if *fixes != 1 {
                    // Never free storage whose exclusive fix is no longer ours.
                    // The post-drop pool check reports the retained item as an error.
                    eprintln!("private item fix ownership changed; retaining container");
                    return;
                }
                *fixes = 0;
            }
            if self.storage && !self.constructed {
                (self.hooks.free)(self.pointer);
            }
            // Constructed storage is destroyed by the item's own operations;
            // they pass (object, object) to entity_destroy then free the storage.
            (self.hooks.release)(self.pointer);
        }
    }
}
fn memory_totals(process: &Process, manager: u32) -> Result<(u64, u64), String> {
    let values = words::<4>(
        process,
        manager.checked_add(0x60).ok_or("manager overflow")?,
    )?;
    Ok((
        u64::from(values[0]) | (u64::from(values[1]) << 32),
        u64::from(values[2]) | (u64::from(values[3]) << 32),
    ))
}
pub fn run(mut block: EngineControl, fixture: bool) -> Result<EngineControl, String> {
    if !(1..=16).contains(&block.cycles) {
        return Err("private item cycles must be 1..=16".into());
    }
    block.executed_thread = unsafe { GetCurrentThreadId() };
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let model = if matches!(
        block.command,
        api::PRIVATE_MODEL | api::PRIVATE_MODEL_FIXTURE
    ) {
        Some(crate::engine_model::ModelName::parse(api::text(
            &block.name,
        )?)?)
    } else {
        None
    };
    let (factory, hooks) = bindings(&process, &block, fixture)?;
    let model = model
        .map(|name| {
            crate::engine_model::Binding::resolve(
                &process,
                name,
                fixture.then_some((block.fixture[1], block.fixture[2])),
            )
        })
        .transpose()?;
    let mut verified = 0;
    let before = if fixture {
        None
    } else {
        Some(
            crate::memory::game_position(&mut verified)
                .map_err(|e| format!("loaded vehicle required: {e:?}"))?,
        )
    };
    let create: Create = unsafe { std::mem::transmute(factory.create as usize) };
    let requested = block.cycles;
    block.cycles = 0;
    for _ in 0..requested {
        let counters = Counters::capture(factory.heads, &mut |a, b| read(&process, a, b))?;
        block.count_before = counters.first();
        let pointer = unsafe { (hooks.alloc)(hooks.context as usize as *mut _) };
        if pointer.is_null() {
            return Err("container allocation returned null".into());
        }
        let mut item = Item {
            pointer,
            hooks: &hooks,
            storage: false,
            fixed: false,
            constructed: false,
        };
        let at = pointer as u32;
        let initial = words::<26>(&process, at)?;
        if initial[0] & 0xffff != 0xd2
            || initial[2] != 0
            || initial[12] != hooks.ops
            || initial[13] != 0
            || initial[18..24] != [0, 1, 0, 0, 0, 0]
            || initial[11] != 0
            || initial[15] != 0
        {
            return Err("new private container differs from contract".into());
        }
        let manager = initial[10];
        let totals = memory_totals(&process, manager)?;
        let total_alloc = totals
            .0
            .checked_add(factory.size.into())
            .ok_or("allocator total overflow")?;
        unsafe {
            (pointer as *mut u32).add(0x34 / 4).write(factory.record);
        }
        let storage = unsafe { (hooks.fix)(pointer, factory.size) };
        if storage.is_null() {
            return Err("container storage allocation returned null".into());
        }
        item.storage = true;
        item.fixed = true;
        let check: Result<(), String> = (|| {
            let allocated = words::<26>(&process, at)?;
            if allocated[2] != storage as u32
                || allocated[18] != factory.size
                || allocated[19] != 1
                || allocated[22] != 1
                || allocated[23] != 0
                || allocated[0] & 8 == 0
            {
                return Err("fixed container storage differs from contract".into());
            }
            let object = unsafe { create(factory.record as usize as *const _, false, storage) };
            if object.is_null() {
                return Err("placed actor constructor returned null".into());
            }
            item.constructed = true;
            if object != storage {
                return Err("placed actor returned different storage".into());
            }
            unsafe {
                let fields = object as *mut u32;
                fields.add(0x60 / 4).write(at);
                fields.add(0x50 / 4).write(0);
                fields.add(0x84 / 4).write(hooks.context);
            }
            block.object_vtable = word(&mut |a, b| read(&process, a, b), object as u32)?;
            block.object_destructor = word(
                &mut |a, b| read(&process, a, b),
                (object as u32).checked_add(0x58).ok_or("object overflow")?,
            )?;
            if block.object_vtable != factory.vtable
                || block.object_destructor != factory.destructor
            {
                return Err("placed actor header differs from contract".into());
            }
            counters.check(true, &mut |a, b| read(&process, a, b))?;
            block.count_created = counters.expected_created();
            if memory_totals(&process, manager)? != (total_alloc, totals.1) {
                return Err("unexpected allocated storage bytes".into());
            }
            if let Some(model) = &model {
                // SAFETY: constructor/header checks succeeded, the item owns
                // storage with exactly one fix, and we are on the outer loop.
                unsafe { model.apply(&process, object, factory.size)? };
            }
            Ok(())
        })();
        drop(item);
        let returned = words::<26>(&process, at)?;
        if returned[0] & 0xffff != 0x80 || returned[2] != 0 || returned[18..24] != [0; 6] {
            return Err("container did not return cleanly to anonymous pool".into());
        }
        let total_freed = totals
            .1
            .checked_add(factory.size.into())
            .ok_or("freed total overflow")?;
        if memory_totals(&process, manager)? != (total_alloc, total_freed) {
            return Err("container storage bytes did not balance".into());
        }
        counters.check(false, &mut |a, b| read(&process, a, b))?;
        check?;
        block.count_after = counters.first();
        block.cycles += 1;
    }
    if let Some(before) = before {
        let after = crate::memory::game_position(&mut verified)
            .map_err(|e| format!("vehicle changed after item probe: {e:?}"))?;
        if before != after {
            return Err("item lifecycle completed; local motion check inconclusive".into());
        }
    }
    block.object_size = factory.size;
    block.constructor = factory.constructor;
    block.destructor = factory.destructor;
    if model.is_some() {
        api::put(
            &mut block.message,
            "model_name verified; private item released; resources not initialized",
        )?;
    }
    Ok(block)
}
