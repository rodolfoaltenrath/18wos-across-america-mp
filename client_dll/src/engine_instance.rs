//! Private PSM c_item ownership. No draw, physics, world insertion or frame retention.
use crate::{
    api::{self, EngineControl},
    engine_info::{self as info, Info, context, offset, same, value},
    engine_lifecycle::{add, executable},
    engine_model::ModelName,
    inspection::process::Process,
    pe_exports::{self, Image},
};
use std::{
    collections::BTreeMap,
    ffi::{c_char, c_void},
    ptr,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
type Allocate = unsafe extern "C" fn(*mut c_void, *const *const c_char, u32, u32) -> *mut c_void;
type Free = unsafe extern "C" fn(*mut c_void);
struct Instance {
    pointer: *mut c_void,
    free: Free,
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { (self.free)(self.pointer) };
    }
}
pub(super) fn bindings(
    process: &Process,
    block: &EngineControl,
    fixture: bool,
) -> Result<(Allocate, Free, u32), String> {
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    let addresses = if fixture
        && matches!(
            block.command,
            api::PRIVATE_INSTANCE_FIXTURE | api::VISUAL_SHOW_FIXTURE
        ) {
        let owner = get("engine_fixture.exe")?;
        executable(block.fixture[3], owner)?;
        executable(block.fixture[4], owner)?;
        [block.fixture[3], block.fixture[4], block.fixture[5]]
    } else if !fixture && matches!(block.command, api::PRIVATE_INSTANCE | api::VISUAL_SHOW) {
        let psm = get("psm.dll")?;
        let core = get("prism3d.exe")?;
        let engine = get("p3shared.dll")?;
        let game = get("game.dll")?;
        for (slot, expected) in [
            (0xeb40, 0x131b0),
            (0xe94c, 0x13420), // ci_alloc_mem_fix, not mm_alloc_mem
            (0xeaec, 0x133f0),
            (0xe9ac, 0x13410),
            (0xe9e8, 0x2720),
            (0xea24, 0x2750),
        ] {
            let actual = value(process, add(psm, slot)?)?;
            if actual != add(core, expected)? {
                return Err(format!(
                    "PSM instance core callback +0x{slot:x} differs: 0x{actual:x}, expected core+0x{expected:x}"
                ));
            }
        }
        if value(process, add(psm, 0xb038)?)? != add(engine, 0xd820)? {
            return Err("PSM packet setter differs from profile".into());
        }
        let images = modules
            .iter()
            .map(|m| {
                Ok(Image {
                    name: m.name.clone(),
                    base: m.base,
                    size: u32::try_from(m.size).map_err(|e| e.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (name, rva, slot) in [
            ("psm_alloc_inst", 0x31c0, 0x161088),
            ("psm_free", 0x3750, 0x160d80),
        ] {
            let expected = add(psm, rva)?;
            if pe_exports::resolve(&images, "psm.dll", name, |a, b| {
                process.read(a, b).map_err(|e| e.to_string())
            })? != u64::from(expected)
                || value(process, add(game, slot)?)? != expected
            {
                return Err("PSM instance export/slot differs from profile".into());
            }
            executable(expected, psm)?;
        }
        let ops = add(psm, 0xe568)?;
        let expected = [
            add(core, 0x10210)?,
            add(core, 0x10180)?,
            add(core, 0x102b0)?,
            add(psm, 0x2eb0)?,
            add(core, 0x10360)?,
            add(core, 0x10370)?,
            add(psm, 0x1010)?,
            u32::MAX,
            u32::MAX,
        ];
        for (i, v) in expected.into_iter().enumerate() {
            let actual = value(process, offset(ops, i as u32 * 4)?)?;
            if actual != v {
                return Err(format!(
                    "PSM instance operation {i} differs: 0x{actual:x}, expected 0x{v:x}"
                ));
            }
        }
        [add(psm, 0x31c0)?, add(psm, 0x3750)?, ops]
    } else {
        return Err("private instance mode does not match attached pump".into());
    };
    Ok((
        unsafe { std::mem::transmute::<usize, Allocate>(addresses[0] as usize) },
        unsafe { std::mem::transmute::<usize, Free>(addresses[1] as usize) },
        addresses[2],
    ))
}
fn counters(process: &Process, at: u32) -> Result<[u32; 3], String> {
    Ok([
        value(process, offset(at, 0x4c)?)?,
        value(process, offset(at, 0x50)?)?,
        value(process, offset(at, 0x58)?)?,
    ])
}
pub(super) struct Checked {
    size: u32,
    shaders: BTreeMap<u32, (u32, u32)>,
}
pub(super) fn check_instance(
    process: &Process,
    at: u32,
    descriptor: u32,
    ops: u32,
) -> Result<Checked, String> {
    if !at.is_multiple_of(4)
        || value(process, at)? & 0x8a != 0x8a
        || counters(process, at)? != [1, 1, 0]
        || value(process, offset(at, 0x30)?)? != ops
    {
        return Err("PSM instance item ownership differs from contract".into());
    }
    let data = value(process, offset(at, 8)?)?;
    let size = value(process, offset(at, 0x48)?)?;
    if data == 0 || !data.is_multiple_of(4) || !(0xb8..=16 * 1024 * 1024).contains(&size) {
        return Err("PSM instance storage differs from contract".into());
    }
    offset(data, size)?;
    if value(process, offset(data, 8)?)? != descriptor
        || value(process, offset(data, 0x58)?)? != 1
        || value(process, offset(data, 0x54)?)? != u32::MAX
        || value(process, offset(data, 0x50)?)? != 1f32.to_bits()
    {
        return Err("PSM instance header differs from contract".into());
    }
    for field in (0x38..=0x4c).step_by(4) {
        if value(process, offset(data, field)?)? != 0 {
            return Err("PSM instance initial transform differs from contract".into());
        }
    }
    let parts = value(process, offset(data, 4)?)?;
    if parts > 256 || value(process, data)? > 4096 {
        return Err("PSM instance model counts exceed diagnostic limits".into());
    }
    let shaders = value(process, offset(data, 0x20)?)?;
    let packets = value(process, offset(data, 0x30)?)?;
    let inside = |at: u32, bytes: u32| {
        at >= data && at.checked_add(bytes).is_some_and(|end| end <= data + size)
    };
    if parts > 0 && (!inside(shaders, parts * 4) || !inside(packets, parts * 4)) {
        return Err("PSM instance tables outside storage".into());
    }
    let mut references = BTreeMap::new();
    for i in 0..parts {
        let shader = value(process, offset(shaders, i * 4)?)?;
        let packet = value(process, offset(packets, i * 4)?)?;
        if shader == 0
            || packet == 0
            || !shader.is_multiple_of(4)
            || !packet.is_multiple_of(4)
            || value(process, packet)? != 1
            || value(process, offset(packet, 4)?)? != at
            || value(process, offset(packet, 12)?)? != i
        {
            return Err("PSM shader/packet ownership differs from contract".into());
        }
        let refs = value(process, offset(shader, 0x4c)?)?;
        let entry = references.entry(shader).or_insert((refs, 0u32));
        if entry.0 != refs {
            return Err("PSM shader references changed during capture".into());
        }
        entry.1 += 1;
        if refs < entry.1 {
            return Err("PSM shader lacks owned references".into());
        }
    }
    Ok(Checked {
        size,
        shaders: references,
    })
}
pub fn run(mut block: EngineControl, fixture: bool) -> Result<EngineControl, String> {
    if !(1..=16).contains(&block.cycles) {
        return Err("instance cycles must be 1..=16".into());
    }
    let path = api::text(&block.name)?;
    let model = ModelName::parse(path)?;
    if !fixture && path != "models/ai/truck/models/truck.psm" {
        return Err("private instance currently restricted to the profiled truck model".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let (alloc_info, release, head) = info::bindings(&process, &block, fixture)?;
    let (allocate, free, ops) = bindings(&process, &block, fixture)?;
    let mut verified = 0;
    let position = if fixture {
        None
    } else {
        Some(
            crate::memory::game_position(&mut verified)
                .map_err(|e| format!("loaded vehicle required: {e:?}"))?,
        )
    };
    let initial = context(&process, head)?;
    initial
        .count
        .checked_add(2)
        .ok_or("PSM context counter overflow")?;
    let pointer = unsafe { alloc_info(model.as_cstr().as_ptr()) };
    if pointer.is_null() {
        return Err("PSM info allocation returned null".into());
    }
    let descriptor = Info { pointer, release };
    let descriptor_at = pointer as u32;
    let info_context = context(&process, head)?;
    same(&initial, &info_context)?;
    if info_context.count <= initial.count {
        return Err("PSM info module lease unavailable".into());
    }
    let retained_info = info_context.count - initial.count - 1;
    if value(&process, descriptor_at)? != 1
        || value(&process, offset(descriptor_at, 8)?)? != 0
        || value(&process, offset(descriptor_at, 12)?)? != 0
    {
        return Err("binary PSM info header differs from contract".into());
    }
    let model_item = value(&process, offset(descriptor_at, 4)?)?;
    if model_item == 0 || !model_item.is_multiple_of(4) {
        return Err("PSM model item unavailable".into());
    }
    block.executed_thread = unsafe { GetCurrentThreadId() };
    let requested = block.cycles;
    block.cycles = 0;
    let mut retained = 0;
    for index in 0..requested {
        let before = context(&process, head)?;
        before
            .count
            .checked_add(1)
            .ok_or("PSM context counter overflow")?;
        let model_before = counters(&process, model_item)?;
        // Profiled cdecl: borrowed held info, no skin overrides, one slot of capacity.
        let pointer = unsafe { allocate(descriptor.pointer, ptr::null(), 0, 1) };
        if pointer.is_null() {
            if value(&process, descriptor_at)? != 1
                || context(&process, head)? != before
                || counters(&process, model_item)? != model_before
            {
                return Err("null instance changed ownership".into());
            }
            return Err("PSM instance allocation returned null".into());
        }
        let instance = Instance { pointer, free };
        let created = context(&process, head)?;
        let checked = (|| {
            same(&before, &created)?;
            if value(&process, descriptor_at)? != 2 {
                return Err("PSM instance did not hold the info reference".into());
            }
            check_instance(&process, pointer as u32, descriptor_at, ops)
        })();
        drop(instance); // Provider must free its own item even if inspection fails.
        let after = context(&process, head)?;
        same(&before, &after)?;
        if value(&process, descriptor_at)? != 1
            || counters(&process, model_item)? != model_before
            || value(&process, offset(pointer as u32, 0x4c)?)? != 0
        {
            return Err("PSM instance released; info/model/item ownership did not balance".into());
        }
        let checked = checked?;
        for (shader, (refs, count)) in checked.shaders {
            // Diagnostic OS read of released ownership, never a Rust dereference.
            if value(&process, offset(shader, 0x4c)?)? != refs - count {
                return Err("PSM instance released; shader references did not balance".into());
            }
        }
        if after.count < before.count
            || created.count <= after.count
            || (after.count > before.count && created.count - after.count != 1)
            || (index > 0 && after.count != before.count)
        {
            return Err(format!(
                "PSM instance released; module/cache delta requires analysis: {}/{}/{}",
                before.count, created.count, after.count
            ));
        }
        retained += after.count - before.count;
        block.count_before = before.count;
        block.count_created = created.count;
        block.count_after = after.count;
        block.object_size = checked.size;
        block.cycles += 1;
    }
    let held = context(&process, head)?;
    drop(descriptor);
    let after = context(&process, head)?;
    same(&initial, &after)?;
    if held.count.checked_sub(1) != Some(after.count) {
        return Err("instances released; final info module lease did not balance".into());
    }
    if let Some(position) = position
        && crate::memory::game_position(&mut verified)
            .map_err(|e| format!("vehicle unavailable: {e:?}"))?
            != position
    {
        return Err("instance lifecycle completed; local motion check inconclusive".into());
    }
    api::put(
        &mut block.message,
        &format!(
            "PSM instances/info released; item, shader and model refs balanced; retained module delta info={retained_info} instance={retained}; no draw"
        ),
    )?;
    Ok(block)
}
