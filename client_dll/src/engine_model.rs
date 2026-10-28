//! Restricted scalar model_name setter for an exclusively owned, uninitialized
//! PMD. This module never initializes resources or keeps an object between frames.
use crate::{
    engine_lifecycle::{add, executable, read},
    engine_values::{self, EntityValue},
    inspection::process::Process,
    profile,
};
use std::ffi::{CString, c_char, c_void};
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;

const OFFSET: usize = 0x200;
const CAPACITY: usize = 64;
type SetString = unsafe extern "C" fn(*mut c_void, *const u32, u32, *const c_char) -> bool;

pub(super) struct ModelName(CString);
impl ModelName {
    pub(super) fn parse(path: &str) -> Result<Self, String> {
        // A deliberately small game-relative path contract. No truncation,
        // platform path normalization, or arbitrary engine resource names.
        if path.len() >= CAPACITY
            || !path.starts_with("models/")
            || !path.ends_with(".psm")
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
            || !path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b))
        {
            return Err(
                "model requires a relative models/*.psm ASCII path of at most 63 bytes".into(),
            );
        }
        Ok(Self(CString::new(path).map_err(|e| e.to_string())?))
    }
}

fn validate(row: &EntityValue, expected_setter: u32) -> Result<(), String> {
    if row.name != "model_name"
        || row.address == 0
        || !row.address.is_multiple_of(4)
        || expected_setter == 0
        || row.set_callback != expected_setter
        || row.type_code != 21
        || row.flags != 0
        || row.element_size != CAPACITY as u32
        || row.table_bound != -1
        || row.value_offset != OFFSET as i32
        || row.auxiliary_offset != -1
        || row.count_offset != -1
    {
        return Err(format!(
            "model_name metadata differs: type={} flags=0x{:x} size={} bound={} offset={} auxiliary={} count={} setter=0x{:x} (expected 0x{expected_setter:x})",
            row.type_code,
            row.flags,
            row.element_size,
            row.table_bound,
            row.value_offset,
            row.auxiliary_offset,
            row.count_offset,
            row.set_callback,
        ));
    }
    Ok(())
}

pub(super) struct Binding {
    definition: u32,
    snapshot: [u8; 96],
    setter: SetString,
    model: ModelName,
}
impl Binding {
    pub(super) fn resolve(
        process: &Process,
        model: ModelName,
        fixture: Option<(u32, u32)>,
    ) -> Result<Self, String> {
        let modules = process.modules().map_err(|e| e.to_string())?;
        let (root, setter, limit) = if let Some((root, setter)) = fixture {
            let owner = modules
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case("engine_fixture.exe"))
                .ok_or("model fixture requires our engine_fixture.exe")?;
            executable(setter, owner)?;
            (root, setter, 1)
        } else {
            // Called after the PMD factory has verified all three image hashes.
            let game = modules
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case("game.dll"))
                .ok_or("game module missing")?;
            let engine = modules
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case("p3shared.dll"))
                .ok_or("engine module missing")?;
            let setter = add(engine, profile::STRING_SETTER)?;
            let export = unsafe {
                GetProcAddress(
                    engine.base as usize as _,
                    c"?me_string_set@@YA_NPAXPAUevalue_def_t@@IPBD@Z"
                        .as_ptr()
                        .cast(),
                )
            };
            if export.map(|f| f as usize as u32) != Some(setter) {
                return Err("string setter export differs from profile".into());
            }
            executable(setter, engine)?;
            (add(game, profile::PMD.values)?, setter, 2048)
        };
        let rows = engine_values::schema(root, limit, |at, out| read(process, at, out))?;
        let mut candidates = rows.iter().filter(|r| r.name == "model_name");
        let row = candidates.next().ok_or("model_name definition missing")?;
        if candidates.next().is_some() {
            return Err("duplicate model_name definition".into());
        }
        validate(row, setter)?;
        let snapshot = std::array::from_fn(|i| row.raw[i / 4].to_le_bytes()[i % 4]);
        Ok(Self {
            definition: row.address,
            snapshot,
            // SAFETY: address belongs to the checked module; the profiled export
            // or our own fixture supplies the four-argument cdecl/bool ABI.
            setter: unsafe { std::mem::transmute::<usize, SetString>(setter as usize) },
            model,
        })
    }

    /// # Safety
    /// object is an exclusively fixed, constructed PMD in engine-owned storage
    /// of exactly PMD.size bytes, on the proven outer loop thread. It is not
    /// initialized, registered in the world, or visible to another owner.
    pub(super) unsafe fn apply(
        &self,
        process: &Process,
        object: *mut c_void,
        size: u32,
    ) -> Result<(), String> {
        if size != profile::PMD.size || object.is_null() {
            return Err("model setter requires private PMD-sized storage".into());
        }
        let at = object as u32;
        at.checked_add(size - 1).ok_or("model object overflow")?;
        let mut definition = [0; 96];
        read(process, self.definition, &mut definition)?;
        if definition != self.snapshot {
            return Err("model_name definition changed before setter".into());
        }
        let mut before = vec![0; size as usize];
        read(process, at, &mut before)?;
        if before[OFFSET] != 0 {
            return Err("private model_name is not initially empty".into());
        }
        // SAFETY: validated scalar inline definition; source is an owned CString
        // of at most 63 bytes; index zero. The caller retains the exclusive item.
        let accepted = unsafe {
            (self.setter)(
                object,
                self.definition as *const u32,
                0,
                self.model.0.as_ptr(),
            )
        };
        let mut after = vec![0; before.len()];
        read(process, at, &mut after)?;
        if before[..OFFSET] != after[..OFFSET]
            || before[OFFSET + CAPACITY..] != after[OFFSET + CAPACITY..]
        {
            return Err("model setter changed bytes outside model_name".into());
        }
        if !accepted {
            return Err("model_name setter returned false".into());
        }
        let expected = self.model.0.as_bytes_with_nul();
        if &after[OFFSET..OFFSET + expected.len()] != expected {
            return Err("model_name setter readback differs".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_bounded_game_relative_model_paths_without_truncation() {
        let longest = format!("models/{}.psm", "a".repeat(52));
        assert_eq!(longest.len(), 63);
        assert!(ModelName::parse(&longest).is_ok());
        for path in [
            "",
            "/models/a.psm",
            "models/../a.psm",
            "models//a.psm",
            "models/./a.psm",
            "models/a\\b.psm",
            "models/a\0.psm",
            "models/á.psm",
            "models/a.png",
            "models/a b.psm",
            "models/a:stream.psm",
            "models/a\n.psm",
        ] {
            assert!(ModelName::parse(path).is_err(), "{path:?}");
        }
        assert!(ModelName::parse(&format!("models/{}.psm", "a".repeat(53))).is_err());
    }
}
