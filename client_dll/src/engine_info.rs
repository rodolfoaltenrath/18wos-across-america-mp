//! Private PSM info/release only: no instance, draw, entity init or insertion.
use crate::{
    api::{self, EngineControl},
    engine_contract::word,
    engine_lifecycle::{add, executable, read},
    engine_model::ModelName,
    inspection::process::Process,
};
use std::ffi::{c_char, c_void};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
type Allocate = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type Release = unsafe extern "C" fn(*mut c_void);
struct Info {
    pointer: *mut c_void,
    release: Release,
}
impl Drop for Info {
    fn drop(&mut self) {
        unsafe { (self.release)(self.pointer) };
    }
}
fn value(process: &Process, at: u32) -> Result<u32, String> {
    word(&mut |a, b| read(process, a, b), at)
}
fn offset(at: u32, n: u32) -> Result<u32, String> {
    at.checked_add(n).ok_or("info address overflow".into())
}
#[derive(PartialEq)]
struct Context {
    pointer: u32,
    parent: u32,
    count: u32,
}
fn context(process: &Process, head: u32) -> Result<Context, String> {
    let pointer = value(process, head)?;
    if pointer == 0 || !pointer.is_multiple_of(4) {
        return Err("PSM context is null or misaligned".into());
    }
    Ok(Context {
        pointer,
        parent: value(process, offset(pointer, 12)?)?,
        count: value(process, offset(pointer, 8)?)?,
    })
}
fn same(a: &Context, b: &Context) -> Result<(), String> {
    if a.pointer != b.pointer || a.parent != b.parent {
        return Err("PSM context identity changed".into());
    }
    Ok(())
}
pub fn run(mut block: EngineControl, fixture: bool) -> Result<EngineControl, String> {
    if !(1..=16).contains(&block.cycles) {
        return Err("info cycles must be 1..=16".into());
    }
    let model = ModelName::parse(api::text(&block.name)?)?;
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let modules = process.modules().map_err(|e| e.to_string())?;
    let (allocate, release, head) = if fixture && block.command == api::PRIVATE_INFO_FIXTURE {
        let owner = modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case("engine_fixture.exe"))
            .ok_or("info fixture requires our host")?;
        executable(block.fixture[0], owner)?;
        executable(block.fixture[1], owner)?;
        (block.fixture[0], block.fixture[1], block.fixture[2])
    } else if !fixture && block.command == api::PRIVATE_INFO {
        let audit = crate::inspection::resources::inspect(&process).map_err(|e| e.to_string())?;
        if !audit.psm_exports_match {
            return Err("PSM resource bindings do not match the verified profile".into());
        }
        let owner = modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case("psm.dll"))
            .ok_or("PSM module missing")?;
        let core = modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case("prism3d.exe"))
            .ok_or("core module missing")?;
        for (slot, expected) in [
            (0xe9d4, 0x13520),
            (0xeb00, 0x13060),
            (0xea58, 0x13630),
            (0xeb28, 0x13700),
        ] {
            if value(&process, add(owner, slot)?)? != add(core, expected)? {
                return Err("PSM core resource/allocator callbacks differ from profile".into());
            }
        }
        let a = add(owner, 0x20e0)?;
        let r = add(owner, 0x2650)?;
        executable(a, owner)?;
        executable(r, owner)?;
        (a, r, add(owner, 0xeb84)?)
    } else {
        return Err("private info mode does not match the attached pump".into());
    };
    let allocate: Allocate = unsafe { std::mem::transmute(allocate as usize) };
    let release: Release = unsafe { std::mem::transmute(release as usize) };
    let mut verified = 0;
    let position = if fixture {
        None
    } else {
        Some(
            crate::memory::game_position(&mut verified)
                .map_err(|e| format!("loaded vehicle required: {e:?}"))?,
        )
    };
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
        // SAFETY: checked game exports or our own fixture, cdecl with one owned
        // CString argument; invoked only by the proven outer-loop dispatcher.
        let pointer = unsafe { allocate(model.as_cstr().as_ptr()) };
        if pointer.is_null() {
            if context(&process, head)? != before {
                return Err("info allocation failed and module references changed".into());
            }
            return Err("PSM info allocation returned null".into());
        }
        let info = Info { pointer, release };
        let at = pointer as u32;
        let created = context(&process, head)?;
        let check = (|| {
            same(&before, &created)?;
            if !at.is_multiple_of(4) || value(&process, at)? != 1 || created.count <= before.count {
                return Err("PSM info header/module lease differs from contract".to_string());
            }
            if value(&process, offset(at, 8)?)? != 0 || value(&process, offset(at, 12)?)? != 0 {
                return Err("binary PSM info unexpectedly contains animations".into());
            }
            let item = value(&process, offset(at, 4)?)?;
            if item == 0 || !item.is_multiple_of(4) {
                return Err("PSM info model item is null or misaligned".into());
            }
            let refs = value(&process, offset(item, 0x4c)?)?;
            if refs == 0 {
                return Err("PSM model item lacks an owned reference".into());
            }
            Ok((item, refs))
        })();
        drop(info); // The provider releases its own allocation on every Rust error.
        let after = context(&process, head)?;
        same(&before, &after)?;
        let (item, refs) = check?;
        // Only an OS-mediated diagnostic read after release: this is no longer
        // our reference. Never dereference it in Rust or call/free the item.
        // Unreadable/reset storage makes this proof fail, not a second release.
        let remaining = value(&process, offset(item, 0x4c)?)?;
        if remaining != refs - 1 {
            return Err("info released; model item reference did not balance".into());
        }
        // A first load may retain cached resources. Never hide growth repeated
        // by subsequent cycles, or claim that retained cache bytes were freed.
        if after.count < before.count
            || created.count <= after.count
            || (after.count > before.count && created.count - after.count != 1)
            || (index > 0 && after.count != before.count)
        {
            return Err(format!(
                "info released; module/cache delta requires analysis: {}/{}/{}",
                before.count, created.count, after.count
            ));
        }
        retained += after.count - before.count;
        block.count_before = before.count;
        block.count_created = created.count;
        block.count_after = after.count;
        block.cycles += 1;
    }
    if let Some(position) = position
        && crate::memory::game_position(&mut verified)
            .map_err(|e| format!("vehicle unavailable after info release: {e:?}"))?
            != position
    {
        return Err("info lifetime completed; local motion check inconclusive".into());
    }
    block.object_size = 16;
    api::put(
        &mut block.message,
        &format!(
            "PSM info released; model reference balanced; retained module delta={retained}; no instance or draw"
        ),
    )?;
    Ok(block)
}
