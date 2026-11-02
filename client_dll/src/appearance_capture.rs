//! Read-only candidates from statically identified actor fields. Names are not
//! proof of active models/materials, colors, slot roles or a complete appearance.
//! Two equal copies/repeated witnesses are NOT an atomic tick or an ABA proof.
use crate::{
    pose_capture::{self, Unavailable},
    profile,
};
use multiplayer_server::career;
pub use multiplayer_server::vehicle_appearance::observation::{
    MAX_JSON, Observation, Slot, Source,
};
#[derive(Clone, Copy)]
pub struct Layout<'a> {
    pub actor: pose_capture::Layout<'a>,
    pub type_record: u32,
}
fn add(at: u32, offset: u32) -> Result<u32, Unavailable> {
    at.checked_add(offset)
        .filter(|v| *v != 0)
        .ok_or(Unavailable::Overflow)
}
fn word(
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
    at: u32,
) -> Result<u32, Unavailable> {
    let mut bytes = [0; 4];
    read(at, &mut bytes).map_err(|_| Unavailable::Missing)?;
    Ok(u32::from_le_bytes(bytes))
}
pub(crate) fn witness(
    layout: Layout<'_>,
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
) -> Result<[u32; 18], Unavailable> {
    if layout.type_record == 0 || !layout.type_record.is_multiple_of(4) {
        return Err(Unavailable::Type);
    }
    let pose = pose_capture::witness(layout.actor, read)?;
    let mut result = [0; 18];
    result[..13].copy_from_slice(&pose);
    let actor = pose[8];
    let item = word(read, add(actor, 0x60)?)?;
    if item == 0 || !item.is_multiple_of(4) {
        return Err(Unavailable::Type);
    }
    let object = word(read, add(item, 8)?)?;
    let record = word(read, add(item, 0x34)?)?;
    let size = word(read, add(item, 0x48)?)?;
    if object != actor || record != layout.type_record || size != profile::CAR_SIZE {
        return Err(Unavailable::Type);
    }
    add(actor, profile::CAR_SIZE - 1)?;
    result[13..17].copy_from_slice(&[item, object, record, size]);
    let mut count = [0];
    read(add(actor, profile::APPEARANCE_COUNT)?, &mut count).map_err(|_| Unavailable::Missing)?;
    result[17] = u32::from(count[0]);
    Ok(result)
}
#[derive(PartialEq, Eq)]
struct Copy {
    make: [u8; profile::APPEARANCE_MAKE_BYTES],
    names: Vec<u8>,
    count: u8,
}
fn copy(
    actor: u32,
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
) -> Result<Copy, Unavailable> {
    let mut count = [0];
    read(add(actor, profile::APPEARANCE_COUNT)?, &mut count).map_err(|_| Unavailable::Missing)?;
    if usize::from(count[0]) > profile::APPEARANCE_MAX_SLOTS {
        return Err(Unavailable::Invalid);
    }
    let mut make = [0; profile::APPEARANCE_MAKE_BYTES];
    read(add(actor, profile::APPEARANCE_MAKE)?, &mut make).map_err(|_| Unavailable::Missing)?;
    let mut names = vec![0; usize::from(count[0]) * profile::APPEARANCE_STRIDE];
    if !names.is_empty() {
        read(add(actor, profile::APPEARANCE_NAMES)?, &mut names)
            .map_err(|_| Unavailable::Missing)?;
    }
    Ok(Copy {
        make,
        names,
        count: count[0],
    })
}
fn identifier(bytes: &[u8]) -> Result<Option<&str>, Unavailable> {
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or(Unavailable::Invalid)?;
    if end == 0 {
        return Ok(None);
    }
    let value = &bytes[..end];
    if !value
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(b))
    {
        return Err(Unavailable::Invalid);
    }
    Ok(Some(
        std::str::from_utf8(value).map_err(|_| Unavailable::Invalid)?,
    ))
}
pub fn capture(
    layout: Layout<'_>,
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
) -> Result<Observation, Unavailable> {
    let before = witness(layout, &mut read)?;
    let first = copy(before[8], &mut read)?;
    let second = copy(before[8], &mut read).map_err(|_| Unavailable::Changed)?;
    let after = witness(layout, &mut read).map_err(|_| Unavailable::Changed)?;
    if before != after || first != second || before[17] != u32::from(first.count) {
        return Err(Unavailable::Changed);
    }
    let make = identifier(&first.make)?
        .ok_or(Unavailable::Missing)?
        .to_ascii_lowercase();
    let mut slots = Vec::new();
    for (i, bytes) in first
        .names
        .as_chunks::<{ profile::APPEARANCE_STRIDE }>()
        .0
        .iter()
        .enumerate()
    {
        let definition_key = identifier(bytes)?.map(|name| {
            career::hash(
                format!(
                    "makes/truck/{make}/acc/{}/!desc.def",
                    name.to_ascii_lowercase()
                )
                .as_bytes(),
            )
        });
        slots.push(Slot {
            index: i as u8,
            definition_key,
        });
    }
    Ok(Observation {
        schema: 1,
        profile: profile::ID.into(),
        source: Source::ProfiledActorNameCandidates,
        make_definition_key: career::hash(format!("makes/truck/{make}/description.def").as_bytes()),
        slots,
        candidate_only: true,
        engine_verified: false,
        world_synchronized: false,
    })
}

#[cfg(test)]
mod tests;
