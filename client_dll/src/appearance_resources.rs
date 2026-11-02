//! Read-only candidate PSM links. No calls, retained references or draw changes.
//! Repeated copies reject observed changes, not an atomic frame or ABA proof.
use crate::{
    appearance_capture::{self, Layout},
    pose_capture::Unavailable,
};
pub use multiplayer_server::vehicle_appearance::resources::{
    Instance, MAX_JSON, MAX_MATERIALS, ResourceObservation,
};
pub const FIELDS: [u32; 3] = [0x30c, 0x310, 0x314];
const MAX_READ_BYTES: usize = 256 * 1024;
const MAX_READ_CALLS: usize = 8192;
fn add(at: u32, offset: u32) -> Result<u32, Unavailable> {
    at.checked_add(offset).ok_or(Unavailable::Overflow)
}
fn aligned(at: u32) -> Result<u32, Unavailable> {
    if at == 0 || !at.is_multiple_of(4) {
        return Err(Unavailable::Type);
    }
    Ok(at)
}
fn word(
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
    at: u32,
) -> Result<u32, Unavailable> {
    let mut b = [0; 4];
    read(at, &mut b).map_err(|_| Unavailable::Missing)?;
    Ok(u32::from_le_bytes(b))
}
fn field(
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
    at: u32,
    n: u32,
) -> Result<u32, Unavailable> {
    word(read, add(at, n)?)
}
// Internal witness includes addresses/identity words, never serialized.
#[derive(PartialEq, Eq)]
struct Named {
    at: u32,
    identity: u32,
    data: u32,
    ops: u32,
    flags: u32,
    name: [u8; 64],
}
fn named(
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
    at: u32,
    suffix: &str,
) -> Result<(Named, String), Unavailable> {
    aligned(at)?;
    let flags = word(read, at)? & 0xffff;
    if flags & 0x488 != 0x408 || field(read, at, 0x4c)? == 0 {
        return Err(Unavailable::Type);
    }
    let data = aligned(field(read, at, 8)?)?;
    let ops = aligned(field(read, at, 0x30)?)?;
    let identity = field(read, at, 4)?;
    let mut name = [0; 64];
    read(add(at, 0x68)?, &mut name).map_err(|_| Unavailable::Missing)?;
    let end = name
        .iter()
        .position(|b| *b == 0)
        .ok_or(Unavailable::Invalid)?;
    let text = std::str::from_utf8(&name[..end]).map_err(|_| Unavailable::Invalid)?;
    if !text.to_ascii_lowercase().ends_with(suffix)
        || text.split('/').any(|p| matches!(p, "" | "." | ".."))
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b))
    {
        return Err(Unavailable::Invalid);
    }
    let normalized = text.to_ascii_lowercase();
    let key = multiplayer_server::career::hash(normalized.as_bytes());
    Ok((
        Named {
            at,
            identity,
            data,
            ops,
            flags,
            name,
        },
        key,
    ))
}
#[derive(PartialEq, Eq)]
struct Copy {
    item: u32,
    data: u32,
    size: u32,
    flags: u32,
    identity: u32,
    info: u32,
    model: Named,
    parts: u32,
    table: u32,
    packets: u32,
    materials: Vec<(Named, [u32; 4])>,
    result: Instance,
}
fn instance(
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
    item: u32,
    ops: u32,
) -> Result<Copy, Unavailable> {
    aligned(item)?;
    let flags = word(read, item)? & 0xffff;
    if flags & 0x88 != 0x88 || field(read, item, 0x30)? != ops || field(read, item, 0x4c)? == 0 {
        return Err(Unavailable::Type);
    }
    let identity = field(read, item, 4)?;
    let data = aligned(field(read, item, 8)?)?;
    let size = field(read, item, 0x48)?;
    if !(0xb8..=16 * 1024 * 1024).contains(&size) {
        return Err(Unavailable::Invalid);
    }
    let end = add(data, size)?;
    let info = aligned(field(read, data, 8)?)?;
    if word(read, info)? == 0 {
        return Err(Unavailable::Type);
    }
    let model_item = field(read, info, 4)?;
    let (model, model_key) = named(read, model_item, ".psm")?;
    let parts = field(read, data, 4)?;
    if parts > MAX_MATERIALS as u32 {
        return Err(Unavailable::Invalid);
    }
    let table = field(read, data, 0x20)?;
    let packets = field(read, data, 0x30)?;
    let inside = |at: u32| {
        at.is_multiple_of(4) && at >= data && at.checked_add(parts * 4).is_some_and(|n| n <= end)
    };
    if parts > 0 && (!inside(table) || !inside(packets)) {
        return Err(Unavailable::Type);
    }
    let mut materials = Vec::new();
    let mut material_keys = Vec::new();
    for i in 0..parts {
        let shader = field(read, table, i * 4)?;
        let packet = aligned(field(read, packets, i * 4)?)?;
        let owner = field(read, packet, 4)?;
        let index = field(read, packet, 12)?;
        if word(read, packet)? == 0 || owner != item || index != i {
            return Err(Unavailable::Type);
        }
        let (material, key) = named(read, shader, ".shd")?;
        materials.push((material, [packet, owner, index, field(read, packet, 8)?]));
        material_keys.push(key);
    }
    Ok(Copy {
        item,
        data,
        size,
        flags,
        identity,
        info,
        model,
        parts,
        table,
        packets,
        materials,
        result: Instance {
            model_key,
            material_keys,
        },
    })
}
fn copies(
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
    actor: u32,
    ops: u32,
) -> Result<Vec<Option<Copy>>, Unavailable> {
    FIELDS
        .into_iter()
        .map(|offset| {
            let item = field(read, actor, offset)?;
            if item == 0 {
                Ok(None)
            } else {
                instance(read, item, ops).map(Some)
            }
        })
        .collect()
}
pub fn capture(
    layout: Layout<'_>,
    ops: u32,
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
) -> Result<ResourceObservation, Unavailable> {
    aligned(ops)?;
    let mut calls = 0usize;
    let mut bytes = 0usize;
    let mut bounded = |at: u32, out: &mut [u8]| {
        calls += 1;
        bytes += out.len();
        if calls > MAX_READ_CALLS
            || bytes > MAX_READ_BYTES
            || at.checked_add(out.len() as u32).is_none()
        {
            return Err(());
        }
        read(at, out)
    };
    let before = appearance_capture::witness(layout, &mut bounded)?;
    let names = appearance_capture::capture(layout, &mut bounded)?;
    let first = copies(&mut bounded, before[8], ops)?;
    let second = copies(&mut bounded, before[8], ops).map_err(|_| Unavailable::Changed)?;
    let names_after =
        appearance_capture::capture(layout, &mut bounded).map_err(|_| Unavailable::Changed)?;
    let after =
        appearance_capture::witness(layout, &mut bounded).map_err(|_| Unavailable::Changed)?;
    if before != after || first != second || names != names_after {
        return Err(Unavailable::Changed);
    }
    let result = ResourceObservation {
        schema: 1,
        names,
        instances: first.into_iter().map(|i| i.map(|c| c.result)).collect(),
        candidate_only: true,
        complete_assembly: false,
        engine_verified: false,
        world_synchronized: false,
    };
    result.validate().map_err(|_| Unavailable::Invalid)?;
    Ok(result)
}
#[cfg(test)]
mod tests;
