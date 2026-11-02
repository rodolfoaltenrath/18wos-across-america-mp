//! Bounded read-only capture. Two equal reads and a repeated pointer witness
//! reject observed changes; this is not an atomic physics tick or an ABA proof.
use multiplayer_server::protocol::{Position, Rotation};

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Selection {
    #[default]
    Drawn,
    GamePose,
}
impl Selection {
    pub fn argument(self) -> &'static str {
        match self {
            Self::Drawn => "drawn",
            Self::GamePose => "game-pose",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Layout<'a> {
    pub root: u32,
    pub steps: &'a [u32],
    pub vtable: u32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Position,
    pub rotation: Rotation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    Missing,
    Overflow,
    Type,
    Changed,
    Invalid,
}
impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Missing => "pose chain or scene unavailable",
            Self::Overflow => "pose address overflow",
            Self::Type => "pose actor differs from profile",
            Self::Changed => "pose changed during capture",
            Self::Invalid => "pose coordinates or quaternion invalid",
        })
    }
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
    if at == 0 {
        return Err(Unavailable::Missing);
    }
    read(at, &mut bytes).map_err(|_| Unavailable::Missing)?;
    let v = u32::from_le_bytes(bytes);
    if v == 0 {
        return Err(Unavailable::Missing);
    }
    Ok(v)
}
pub(crate) fn witness(
    layout: Layout<'_>,
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
) -> Result<[u32; 13], Unavailable> {
    if layout.steps.len() > 8 || layout.vtable == 0 {
        return Err(Unavailable::Type);
    }
    let mut values = [0; 13];
    let mut at = layout.root;
    for (i, offset) in layout.steps.iter().enumerate() {
        let p = word(read, at)?;
        values[i] = p;
        at = add(p, *offset)?;
    }
    let actor = word(read, at)?;
    if word(read, actor)? != layout.vtable {
        return Err(Unavailable::Type);
    }
    values[8] = actor;
    values[9] = word(read, add(actor, 0x84)?)?;
    values[10] = word(read, add(actor, 0x1d0)?)?;
    values[11] = word(read, add(values[10], 0x3e4)?)?;
    values[12] = add(values[11], 0x14)?;
    // Bound the full 40-byte copy as well as its start.
    add(values[12], 39)?;
    Ok(values)
}
pub fn capture(
    layout: Layout<'_>,
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), ()>,
) -> Result<Pose, Unavailable> {
    let before = witness(layout, &mut read)?;
    let mut first = [0; 40];
    let mut second = [0; 40];
    read(before[12], &mut first).map_err(|_| Unavailable::Missing)?;
    read(before[12], &mut second).map_err(|_| Unavailable::Missing)?;
    let after = witness(layout, &mut read).map_err(|_| Unavailable::Changed)?;
    if first != second || before != after {
        return Err(Unavailable::Changed);
    }
    let f = |at: usize| f32::from_le_bytes(first[at..at + 4].try_into().unwrap());
    let rotation = Rotation {
        x: f(0),
        y: f(4),
        z: f(8),
        w: f(12),
    };
    let position = Position {
        x: f(28),
        y: f(32),
        z: f(36),
    };
    if !rotation.is_valid() || !position.is_valid() {
        return Err(Unavailable::Invalid);
    }
    Ok(Pose { position, rotation })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn own() -> BTreeMap<u32, u8> {
        let mut m = BTreeMap::new();
        for (at, value) in [
            (0x100, 0x200),
            (0x204, 0x1000),
            (0x1000, 0x9000),
            (0x1084, 0x8000),
            (0x11d0, 0x2000),
            (0x23e4, 0x3000),
        ] {
            put(&mut m, at, &(value as u32).to_le_bytes());
        }
        let mut block = [0; 40];
        for (offset, value) in [(12, 1f32), (28, 125f32), (32, -20f32), (36, 500f32)] {
            block[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        put(&mut m, 0x3014, &block);
        m
    }
    fn put(m: &mut BTreeMap<u32, u8>, at: u32, b: &[u8]) {
        for (i, v) in b.iter().enumerate() {
            m.insert(at + i as u32, *v);
        }
    }
    fn read(m: &BTreeMap<u32, u8>, at: u32, b: &mut [u8]) -> Result<(), ()> {
        for (i, v) in b.iter_mut().enumerate() {
            *v = *m.get(&(at + i as u32)).ok_or(())?;
        }
        Ok(())
    }
    fn layout() -> Layout<'static> {
        Layout {
            root: 0x100,
            steps: &[4],
            vtable: 0x9000,
        }
    }
    #[test]
    fn stable_pose_is_grouped_and_missing_invalid_or_wrong_type_never_emits() {
        let own = own();
        let p = capture(layout(), |at, b| read(&own, at, b)).unwrap();
        assert_eq!(
            p.position,
            Position {
                x: 125.,
                y: -20.,
                z: 500.
            }
        );
        assert_eq!(p.rotation.w, 1.);
        for (at, v, expected) in [
            (0x100, 0, Unavailable::Missing),
            (0x1084, 0, Unavailable::Missing),
            (0x1000, 42, Unavailable::Type),
            (0x3020, 0, Unavailable::Invalid),
            (0x3030, f32::NAN.to_bits(), Unavailable::Invalid),
        ] {
            let mut m = own.clone();
            put(&mut m, at, &v.to_le_bytes());
            assert_eq!(capture(layout(), |at, b| read(&m, at, b)), Err(expected));
        }
    }
    #[test]
    fn changed_bytes_chain_or_scene_are_rejected_in_one_bounded_attempt() {
        for change in [0x3014, 0x1084, 0x204] {
            let mut m = own();
            let mut copies = 0;
            let result = capture(layout(), |at, b| {
                if at == 0x3014 {
                    copies += 1;
                    if copies == 2 {
                        put(&mut m, change, &1u32.to_le_bytes());
                    }
                }
                read(&m, at, b)
            });
            assert_eq!(result, Err(Unavailable::Changed));
            assert_eq!(copies, 2);
        }
    }
    #[test]
    fn overflow_and_excessive_chain_are_refused() {
        let mut l = layout();
        l.steps = &[0; 9];
        assert_eq!(capture(l, |_, _| panic!()), Err(Unavailable::Type));
        l.steps = &[8];
        assert_eq!(
            capture(l, |_, b| {
                b.copy_from_slice(&(u32::MAX - 4).to_le_bytes());
                Ok(())
            }),
            Err(Unavailable::Overflow)
        );
    }
}
