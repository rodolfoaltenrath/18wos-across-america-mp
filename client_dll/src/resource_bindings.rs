//! Read-only resource binding audit. A matching slot/export is an observation,
//! never permission to call it or proof of its ABI, thread, or ownership.
use crate::{
    pe_exports::{self, Image},
    profile,
};
use serde::Serialize;

pub const PSM_APIS: [(&str, u32, u32); 6] = [
    ("psm_alloc_info", 0x16111c, 0x20e0),
    ("psm_info_release", 0x160ec8, 0x2650),
    ("psm_alloc", 0x160b4c, 0x3160),
    ("psm_alloc_simple", 0x160998, 0x31a0),
    ("psm_free", 0x160d80, 0x3750),
    ("psm_draw", 0x160ba4, 0x6a10),
];
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BindingState {
    MatchedExport,
    Unbound,
    UnexpectedTarget,
    UnprofiledTarget,
    ProviderMissing,
}
#[derive(Debug, Serialize)]
pub struct Binding {
    pub label: String,
    pub slot_rva: u32,
    pub target: u32,
    pub expected: Option<u32>,
    pub target_module: Option<String>,
    pub target_rva: Option<u32>,
    pub state: BindingState,
}
#[derive(Debug, Serialize)]
pub struct Audit {
    pub profile: &'static str,
    pub psm_exports_match: bool,
    pub pmd_slots_unbound: bool,
    pub bindings: Vec<Binding>,
}
fn module<'a>(images: &'a [Image], name: &str) -> Result<Option<&'a Image>, String> {
    let mut matches = images.iter().filter(|m| m.name.eq_ignore_ascii_case(name));
    let found = matches.next();
    if matches.next().is_some() {
        return Err(format!("ambiguous resource module {name}"));
    }
    if let Some(m) = found
        && (m.size == 0
            || m.base
                .checked_add(u64::from(m.size) - 1)
                .is_none_or(|end| end > u32::MAX.into()))
    {
        return Err("resource audit requires bounded x86 images".into());
    }
    Ok(found)
}
fn address(image: &Image, rva: u32, len: u32) -> Result<u32, String> {
    if u64::from(rva) + u64::from(len) > u64::from(image.size) {
        return Err("resource RVA outside image".into());
    }
    u32::try_from(image.base + u64::from(rva)).map_err(|e| e.to_string())
}
pub fn inspect(
    images: &[Image],
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), String>,
) -> Result<Audit, String> {
    let game = module(images, "game.dll")?.ok_or("game.dll not loaded")?;
    let provider = module(images, "psm.dll")?;
    let specs: Vec<_> = PSM_APIS
        .into_iter()
        .map(|(name, slot, export)| (name, slot, Some(export)))
        .chain([
            ("pmd_load_candidate", 0x160e20, None),
            ("pmd_release_candidate", 0x16114c, None),
        ])
        .collect();
    let mut bindings = Vec::new();
    for (label, slot_rva, export) in specs {
        let at = address(game, slot_rva, 4)?;
        let mut bytes = [0; 4];
        read(at.into(), &mut bytes)?;
        let target = u32::from_le_bytes(bytes);
        let expected = match (export, provider) {
            (Some(rva), Some(image)) => {
                let expected = address(image, rva, 1)?;
                let actual = pe_exports::resolve(images, "psm.dll", label, &mut read)?;
                if actual != u64::from(expected) {
                    return Err(format!("loaded export {label} differs from profile"));
                }
                Some(expected)
            }
            _ => None,
        };
        let state = if export.is_some() && provider.is_none() {
            BindingState::ProviderMissing
        } else if target == 0 {
            BindingState::Unbound
        } else if let Some(expected) = expected {
            if expected == target {
                BindingState::MatchedExport
            } else {
                BindingState::UnexpectedTarget
            }
        } else {
            BindingState::UnprofiledTarget
        };
        let owner = images.iter().find(|m| {
            u64::from(target) >= m.base && u64::from(target) - m.base < u64::from(m.size)
        });
        bindings.push(Binding {
            label: label.into(),
            slot_rva,
            target,
            expected,
            target_module: owner.map(|m| m.name.clone()),
            target_rva: owner.map(|m| (u64::from(target) - m.base) as u32),
            state,
        });
    }
    // Reject a changing capture rather than presenting stale callback addresses.
    for binding in &bindings {
        let mut bytes = [0; 4];
        read(address(game, binding.slot_rva, 4)?.into(), &mut bytes)?;
        if u32::from_le_bytes(bytes) != binding.target {
            return Err("resource slots changed during capture".into());
        }
    }
    Ok(Audit {
        profile: profile::ID,
        psm_exports_match: bindings[..6]
            .iter()
            .all(|b| b.state == BindingState::MatchedExport),
        pmd_slots_unbound: bindings[6..]
            .iter()
            .all(|b| b.state == BindingState::Unbound),
        bindings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        images: Vec<Image>,
        game: Vec<u8>,
        psm: Vec<u8>,
    }
    fn put(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    impl Fixture {
        fn new(game_base: u64, psm_base: u64) -> Self {
            let mut game = vec![0; 0x180000];
            let mut psm = vec![0; 0x10000];
            psm[..2].copy_from_slice(b"MZ");
            put(&mut psm, 60, 64);
            psm[64..68].copy_from_slice(b"PE\0\0");
            psm[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
            psm[88..90].copy_from_slice(&0x10bu16.to_le_bytes());
            for (at, value) in [
                (184, 256),
                (188, 128),
                (272, 1),
                (276, 6),
                (280, 6),
                (284, 512),
                (288, 544),
                (292, 576),
            ] {
                put(&mut psm, at, value);
            }
            let mut text = 600;
            for (i, (name, slot, export)) in PSM_APIS.into_iter().enumerate() {
                put(
                    &mut game,
                    slot as usize,
                    (psm_base + u64::from(export)) as u32,
                );
                put(&mut psm, 512 + i * 4, export);
                put(&mut psm, 544 + i * 4, text as u32);
                psm[576 + i * 2..578 + i * 2].copy_from_slice(&(i as u16).to_le_bytes());
                psm[text..text + name.len()].copy_from_slice(name.as_bytes());
                text += name.len() + 1;
            }
            Self {
                images: vec![
                    Image {
                        name: "game.dll".into(),
                        base: game_base,
                        size: game.len() as u32,
                    },
                    Image {
                        name: "psm.dll".into(),
                        base: psm_base,
                        size: psm.len() as u32,
                    },
                ],
                game,
                psm,
            }
        }
        fn read(&self, at: u64, out: &mut [u8]) -> Result<(), String> {
            for (image, bytes) in self.images.iter().zip([&self.game, &self.psm]) {
                if let Some(offset) = at.checked_sub(image.base)
                    && offset < bytes.len() as u64
                {
                    let start = offset as usize;
                    out.copy_from_slice(
                        bytes
                            .get(start..start + out.len())
                            .ok_or("truncated fixture read")?,
                    );
                    return Ok(());
                }
            }
            Err("unmapped own fixture".into())
        }
        fn audit(&self) -> Result<Audit, String> {
            inspect(&self.images, |at, out| self.read(at, out))
        }
    }
    #[test]
    fn matches_rebased_named_exports_and_keeps_legacy_unbound_distinct() {
        for bases in [(0x10000000, 0x20000000), (0x04000000, 0x06000000)] {
            let f = Fixture::new(bases.0, bases.1);
            let report = f.audit().unwrap();
            assert!(report.psm_exports_match && report.pmd_slots_unbound);
            assert_eq!(report.bindings.len(), 8);
            assert_eq!(report.bindings[0].target_module.as_deref(), Some("psm.dll"));
            assert_eq!(report.bindings[0].target_rva, Some(0x20e0));
            assert!(
                report.bindings[6..]
                    .iter()
                    .all(|b| b.expected.is_none() && b.state == BindingState::Unbound)
            );
        }
    }
    #[test]
    fn reports_wrong_targets_missing_provider_and_unprofiled_pmd_without_calling_anything() {
        let mut f = Fixture::new(0x10000000, 0x20000000);
        put(&mut f.game, PSM_APIS[0].1 as usize, 0x20002650); // Same module, wrong export.
        put(&mut f.game, 0x160e20, 0x20003160); // Never infer a PMD ABI from a nonzero pointer.
        let report = f.audit().unwrap();
        assert!(!report.psm_exports_match && !report.pmd_slots_unbound);
        assert_eq!(report.bindings[0].state, BindingState::UnexpectedTarget);
        assert_eq!(report.bindings[6].state, BindingState::UnprofiledTarget);
        let missing = inspect(&f.images[..1], |at, out| f.read(at, out)).unwrap();
        assert!(
            missing.bindings[..6]
                .iter()
                .all(|b| b.state == BindingState::ProviderMissing)
        );
        put(&mut f.game, PSM_APIS[0].1 as usize, 0);
        assert_eq!(f.audit().unwrap().bindings[0].state, BindingState::Unbound);
    }
    #[test]
    fn rejects_changed_slots_changed_exports_unreadable_and_out_of_range_images() {
        let mut f = Fixture::new(0x10000000, 0x20000000);
        let mut reads = 0;
        let result = inspect(&f.images, |at, out| {
            f.read(at, out)?;
            if at == f.images[0].base + u64::from(PSM_APIS[0].1) {
                reads += 1;
                if reads == 2 {
                    out[0] ^= 1;
                }
            }
            Ok(())
        });
        assert!(result.unwrap_err().contains("changed during capture"));
        put(&mut f.psm, 512, 0x20e4);
        assert!(
            f.audit()
                .unwrap_err()
                .contains("export psm_alloc_info differs")
        );
        assert!(inspect(&f.images, |_, _| Err("unreadable".into())).is_err());
        f.images[1].base = u64::from(u32::MAX) - 0x100;
        assert!(f.audit().is_err());
        f.images[1].base = 0x20000000;
        f.images[0].size = 0x1000;
        assert!(f.audit().is_err());
    }
}
