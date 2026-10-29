//! Bounded command metadata reads. No argv, save arguments, writes or calls.
use crate::{pe_exports::Image, profile};
use serde::Serialize;
use std::collections::BTreeSet;
pub const MAX_COMMANDS: usize = 1024;
pub const BUFFER_SLOTS: usize = 100;
pub const BUFFER_STRIDE: u32 = 128;
const MAX_NAME: usize = 128;
#[derive(Debug, Serialize)]
pub struct Entry {
    pub address: u32,
    pub name: String,
    pub callback: u32,
    pub callback_module: Option<String>,
    pub callback_rva: Option<u32>,
    pub hash_matches: bool,
}
#[derive(Debug, Serialize)]
pub struct Audit {
    pub profile: &'static str,
    pub head: u32,
    pub entries: Vec<Entry>,
    pub map_descriptor: u32,
    pub map_callback: u32,
    pub map_registered: bool,
    pub map_profile_matches: bool,
    pub buffer_occupied: usize,
    pub buffer_counter: u32,
    /// This buffer is not assumed to be a consumable execution queue.
    pub execution_queue_proven: bool,
}
fn image<'a>(images: &'a [Image], name: &str) -> Result<&'a Image, String> {
    let mut found = images.iter().filter(|m| m.name.eq_ignore_ascii_case(name));
    let image = found.next().ok_or_else(|| format!("{name} not loaded"))?;
    if found.next().is_some()
        || image.size == 0
        || image
            .base
            .checked_add(image.size as u64)
            .is_none_or(|end| end > u32::MAX as u64 + 1)
    {
        return Err("ambiguous or non-x86 command image".into());
    }
    Ok(image)
}
fn at(image: &Image, rva: u32, bytes: u32) -> Result<u32, String> {
    if rva.checked_add(bytes).is_none_or(|end| end > image.size) {
        return Err("command profile RVA outside image".into());
    }
    u32::try_from(image.base + rva as u64).map_err(|_| "command address overflow".into())
}
fn word(
    read: &mut impl FnMut(u64, &mut [u8]) -> Result<(), String>,
    at: u32,
) -> Result<u32, String> {
    let mut bytes = [0; 4];
    at.checked_add(3).ok_or("command pointer overflow")?;
    read(at.into(), &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn header(
    read: &mut impl FnMut(u64, &mut [u8]) -> Result<(), String>,
    at: u32,
) -> Result<[u32; 4], String> {
    let mut bytes = [0; 16];
    if at == 0 || at.checked_add(15).is_none() {
        return Err("invalid command descriptor".into());
    }
    read(at.into(), &mut bytes)?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
fn name(
    read: &mut impl FnMut(u64, &mut [u8]) -> Result<(), String>,
    start: u32,
) -> Result<Vec<u8>, String> {
    if start == 0 {
        return Err("null command name".into());
    }
    let mut out = Vec::new();
    for i in 0..MAX_NAME {
        let mut b = [0];
        read(
            start
                .checked_add(i as u32)
                .ok_or("command name overflow")?
                .into(),
            &mut b,
        )?;
        if b[0] == 0 {
            return (!out.is_empty())
                .then_some(out)
                .ok_or("empty command name".into());
        }
        out.push(b[0]);
    }
    Err("command name exceeds bounded metadata limit".into())
}
pub fn name_hash(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0u32, |sum, &b| sum.wrapping_add(b as i8 as i32 as u32))
}
/// Read-only preflight for the passive map observer. Bindings are addresses of
/// words, not functions to invoke. No argv element or argument is inspected.
pub fn map_observer_slot(
    descriptor: u32,
    callback: u32,
    bindings: [u32; 3],
    read: impl FnMut(u64, &mut [u8]) -> Result<(), String>,
) -> Result<u32, String> {
    command_slot(descriptor, b"map", callback, bindings, read)
}
/// Same preflight for any profiled command descriptor of the shared handler:
/// exact NUL-terminated name, signed-sum hash, callback, stable header.
pub fn command_slot(
    descriptor: u32,
    expected: &[u8],
    callback: u32,
    bindings: [u32; 3],
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), String>,
) -> Result<u32, String> {
    if descriptor == 0 || !descriptor.is_multiple_of(4) || callback == 0 {
        return Err("invalid command descriptor alignment or callback".into());
    }
    if expected.is_empty() || expected.len() > 15 || expected.contains(&0) {
        return Err("invalid expected command name".into());
    }
    let end = descriptor
        .checked_add(16)
        .ok_or("command descriptor overflow")?;
    for (i, &binding) in bindings.iter().enumerate() {
        if binding == 0
            || !binding.is_multiple_of(4)
            || (descriptor..end).contains(&binding)
            || bindings[..i].contains(&binding)
        {
            return Err("invalid or aliased command binding".into());
        }
        word(&mut read, binding)?;
    }
    let before = header(&mut read, descriptor)?;
    let mut name = [0; 16];
    let name = &mut name[..=expected.len()];
    read(before[0].into(), name)?;
    if &name[..expected.len()] != expected
        || name[expected.len()] != 0
        || before[1] != callback
        || before[2] != name_hash(expected)
    {
        return Err("command descriptor name/hash/callback mismatch".into());
    }
    if header(&mut read, descriptor)? != before {
        return Err("command descriptor changed during preflight".into());
    }
    Ok(descriptor + 4)
}
pub fn inspect(
    images: &[Image],
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), String>,
) -> Result<Audit, String> {
    let core = image(images, "prism3d.exe")?;
    let game = image(images, "game.dll")?;
    let head_slot = at(core, profile::COMMAND_HEAD, 4)?;
    let head = word(&mut read, head_slot)?;
    let map_descriptor = at(game, profile::MAP_DESCRIPTOR, 16)?;
    let map_callback = at(game, profile::MAP_CALLBACK, 1)?;
    let map_name = at(game, profile::MAP_NAME, 4)?;
    let map_header = header(&mut read, map_descriptor)?;
    let mut next = head;
    let mut seen = BTreeSet::new();
    let mut records = Vec::new();
    let mut entries = Vec::new();
    while next != 0 {
        if entries.len() == MAX_COMMANDS || !seen.insert(next) {
            return Err("command registry cycle or record limit".into());
        }
        let h = header(&mut read, next)?;
        let bytes = name(&mut read, h[0])?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| "command name is not UTF-8")?
            .to_owned();
        let owner = images
            .iter()
            .find(|m| h[1] as u64 >= m.base && (h[1] as u64 - m.base) < m.size as u64);
        entries.push(Entry {
            address: next,
            name: text,
            callback: h[1],
            callback_module: owner.map(|m| m.name.clone()),
            callback_rva: owner.map(|m| (h[1] as u64 - m.base) as u32),
            hash_matches: h[2] == name_hash(&bytes),
        });
        records.push((next, h, bytes));
        next = h[3];
    }
    let buffer = at(
        core,
        profile::COMMAND_BUFFER,
        (BUFFER_SLOTS as u32) * BUFFER_STRIDE,
    )?;
    let counter_slot = at(core, profile::COMMAND_BUFFER_COUNTER, 4)?;
    let counter = word(&mut read, counter_slot)?;
    let mut occupancy = [false; BUFFER_SLOTS];
    for (i, used) in occupancy.iter_mut().enumerate() {
        let mut b = [0];
        read((buffer + i as u32 * BUFFER_STRIDE).into(), &mut b)?;
        *used = b[0] != 0;
    }
    // A second pass detects common concurrent mutations, not ABA or quiescence.
    for (address, h, bytes) in &records {
        if header(&mut read, *address)? != *h || name(&mut read, h[0])? != *bytes {
            return Err("command registry changed during audit".into());
        }
    }
    for (i, used) in occupancy.iter().enumerate() {
        let mut b = [0];
        read((buffer + i as u32 * BUFFER_STRIDE).into(), &mut b)?;
        if (b[0] != 0) != *used {
            return Err("command buffer changed during audit".into());
        }
    }
    if word(&mut read, head_slot)? != head
        || word(&mut read, counter_slot)? != counter
        || header(&mut read, map_descriptor)? != map_header
    {
        return Err("command metadata changed during audit".into());
    }
    let maps: Vec<_> = entries.iter().filter(|e| e.name == "map").collect();
    let map_registered = maps.len() == 1 && maps[0].address == map_descriptor;
    let map_profile_matches = map_registered
        && map_header[0] == map_name
        && map_header[1] == map_callback
        && maps[0].hash_matches;
    Ok(Audit {
        profile: profile::ID,
        head,
        entries,
        map_descriptor,
        map_callback,
        map_registered,
        map_profile_matches,
        buffer_occupied: occupancy.iter().filter(|&&b| b).count(),
        buffer_counter: counter,
        execution_queue_proven: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_observer_preflight_checks_header_bindings_and_mutation_without_reading_argv() {
        let mut memory = [0u8; 160];
        memory[32..36].copy_from_slice(b"map\0");
        for (i, value) in [32u32, 0x1234, 318, 0].into_iter().enumerate() {
            memory[64 + i * 4..68 + i * 4].copy_from_slice(&value.to_le_bytes());
        }
        // Values are deliberately not readable pointers; preflight reads words only.
        for i in [128usize, 132, 136] {
            memory[i..i + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        let read = |at: u64, out: &mut [u8]| {
            out.copy_from_slice(
                memory
                    .get(at as usize..at as usize + out.len())
                    .ok_or("unmapped")?,
            );
            Ok(())
        };
        assert_eq!(
            map_observer_slot(64, 0x1234, [128, 132, 136], read).unwrap(),
            68
        );
        for (d, c, b) in [
            (65, 0x1234, [128, 132, 136]),
            (64, 0x1235, [128, 132, 136]),
            (64, 0x1234, [128, 128, 136]),
            (64, 0x1234, [64, 132, 136]),
            (64, 0x1234, [128, 132, 1]),
            (u32::MAX - 3, 0x1234, [128, 132, 136]),
        ] {
            assert!(map_observer_slot(d, c, b, read).is_err());
        }
        let mut headers = 0;
        assert!(
            map_observer_slot(64, 0x1234, [128, 132, 136], |at, out| {
                read(at, out)?;
                if at == 64 {
                    headers += 1;
                    if headers == 2 {
                        out[12] = 1;
                    }
                }
                Ok(())
            })
            .unwrap_err()
            .contains("changed")
        );
        memory[72..76].fill(0);
        assert!(
            map_observer_slot(64, 0x1234, [128, 132, 136], |at, out| {
                out.copy_from_slice(&memory[at as usize..at as usize + out.len()]);
                Ok(())
            })
            .unwrap_err()
            .contains("mismatch")
        );
    }
    struct Fixture {
        images: Vec<Image>,
        core: Vec<u8>,
        game: Vec<u8>,
    }
    impl Fixture {
        fn new() -> Self {
            let mut s = Self {
                images: vec![
                    Image {
                        name: "prism3d.exe".into(),
                        base: 0x400000,
                        size: 0x78000,
                    },
                    Image {
                        name: "game.dll".into(),
                        base: 0x10000000,
                        size: 0x160000,
                    },
                ],
                core: vec![0; 0x78000],
                game: vec![0; 0x160000],
            };
            s.core[profile::COMMAND_HEAD as usize..][..4]
                .copy_from_slice(&(0x10000000 + profile::MAP_DESCRIPTOR).to_le_bytes());
            s.game[profile::MAP_NAME as usize..][..4].copy_from_slice(b"map\0");
            let h = [
                0x10000000 + profile::MAP_NAME,
                0x10000000 + profile::MAP_CALLBACK,
                name_hash(b"map"),
                0,
            ];
            for (i, w) in h.iter().enumerate() {
                s.game[profile::MAP_DESCRIPTOR as usize + i * 4..][..4]
                    .copy_from_slice(&w.to_le_bytes());
            }
            s.core[profile::COMMAND_BUFFER as usize] = b'm';
            s.core[profile::COMMAND_BUFFER as usize + 128] = b'q';
            s
        }
        fn read(&self, at: u64, out: &mut [u8]) -> Result<(), String> {
            for (image, data) in self.images.iter().zip([&self.core, &self.game]) {
                if at >= image.base && at - image.base < data.len() as u64 {
                    let start = (at - image.base) as usize;
                    out.copy_from_slice(data.get(start..start + out.len()).ok_or("fixture end")?);
                    return Ok(());
                }
            }
            Err("fixture unmapped".into())
        }
    }
    #[test]
    fn audits_map_registration_and_only_first_buffer_bytes_without_arguments() {
        let f = Fixture::new();
        let a = inspect(&f.images, |at, out| {
            let start = 0x400000 + profile::COMMAND_BUFFER as u64;
            if (start..start + 12800).contains(&at) {
                assert_eq!((at - start) % 128, 0);
                assert_eq!(out.len(), 1);
            }
            f.read(at, out)
        })
        .unwrap();
        assert!(a.map_registered && a.map_profile_matches);
        assert_eq!(a.buffer_occupied, 2);
        assert_eq!(a.entries[0].callback_rva, Some(profile::MAP_CALLBACK));
        assert!(!a.execution_queue_proven);
        assert_eq!(name_hash(&[0xff]), u32::MAX);
    }
    #[test]
    fn reports_changed_callback_and_hash_without_authorizing_calls() {
        let mut f = Fixture::new();
        f.game[profile::MAP_DESCRIPTOR as usize + 4..][..4]
            .copy_from_slice(&0x1234u32.to_le_bytes());
        f.game[profile::MAP_DESCRIPTOR as usize + 8..][..4].fill(0);
        let a = inspect(&f.images, |at, out| f.read(at, out)).unwrap();
        assert!(a.map_registered && !a.map_profile_matches);
        assert_eq!(a.entries[0].callback_module, None);
        assert!(!a.entries[0].hash_matches);
    }
    #[test]
    fn rejects_cycle_unreadable_names_and_non_x86_images() {
        let mut f = Fixture::new();
        f.game[profile::MAP_DESCRIPTOR as usize + 12..][..4]
            .copy_from_slice(&(0x10000000 + profile::MAP_DESCRIPTOR).to_le_bytes());
        assert!(
            inspect(&f.images, |at, out| f.read(at, out))
                .unwrap_err()
                .contains("cycle")
        );
        let mut f = Fixture::new();
        f.game[profile::MAP_NAME as usize..][..MAX_NAME].fill(b'x');
        assert!(
            inspect(&f.images, |at, out| f.read(at, out))
                .unwrap_err()
                .contains("limit")
        );
        f.images[0].base = u32::MAX as u64;
        assert!(inspect(&f.images, |at, out| f.read(at, out)).is_err());
    }
    #[test]
    fn rejects_racing_head_and_buffer_without_presenting_a_stable_snapshot() {
        let f = Fixture::new();
        let mut heads = 0;
        assert!(
            inspect(&f.images, |at, out| {
                f.read(at, out)?;
                if at == 0x400000 + profile::COMMAND_HEAD as u64 {
                    heads += 1;
                    if heads == 2 {
                        out.fill(0);
                    }
                }
                Ok(())
            })
            .unwrap_err()
            .contains("changed")
        );
        let mut slots = 0;
        assert!(
            inspect(&f.images, |at, out| {
                f.read(at, out)?;
                if at == 0x400000 + profile::COMMAND_BUFFER as u64 {
                    slots += 1;
                    if slots == 2 {
                        out.fill(0);
                    }
                }
                Ok(())
            })
            .unwrap_err()
            .contains("changed")
        );
    }
}
#[cfg(test)]
mod command_slot_tests {
    use super::*;
    #[test]
    fn command_slot_requires_the_exact_terminated_name_and_its_hash() {
        let mut memory = [0u8; 160];
        memory[32..37].copy_from_slice(b"exit\0");
        memory[40..46].copy_from_slice(b"exitx\0");
        let header = |m: &mut [u8; 160], name: u32, hash: u32| {
            for (i, v) in [name, 0x1234, hash, 0].into_iter().enumerate() {
                m[64 + i * 4..68 + i * 4].copy_from_slice(&v.to_le_bytes());
            }
        };
        let run = |m: &[u8; 160], expected: &[u8]| {
            command_slot(64, expected, 0x1234, [128, 132, 136], |at, out| {
                out.copy_from_slice(
                    m.get(at as usize..at as usize + out.len())
                        .ok_or("unmapped")?,
                );
                Ok(())
            })
        };
        header(&mut memory, 32, name_hash(b"exit"));
        assert_eq!(run(&memory, b"exit").unwrap(), 68);
        assert!(run(&memory, b"exi").is_err());
        assert!(run(&memory, b"map").is_err());
        assert!(run(&memory, b"").is_err());
        assert!(run(&memory, b"exit\0").is_err());
        header(&mut memory, 40, name_hash(b"exit"));
        assert!(run(&memory, b"exit").is_err());
        header(&mut memory, 32, name_hash(b"exit") + 1);
        assert!(run(&memory, b"exit").is_err());
    }
}
