//! Read-only entity registry decoding for the hash-identified x86 engine.
//! The result describes candidates; it never calls an engine function.
use serde::Serialize;
use std::collections::BTreeSet;
#[derive(Debug, Serialize)]
pub struct EntityRecord {
    pub name: String,
    pub address: u32,
    pub flags: u32,
    pub true_size: u32,
    pub true_constructor: u32,
    pub true_destructor: u32,
    pub false_size: u32,
    pub false_constructor: u32,
    pub false_destructor: u32,
}
fn word(
    read: &mut impl FnMut(u32, &mut [u8]) -> Result<(), String>,
    at: u32,
) -> Result<u32, String> {
    let mut bytes = [0; 4];
    read(at, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
pub fn registry(
    head_address: u32,
    limit: usize,
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<Vec<EntityRecord>, String> {
    if !(1..=4096).contains(&limit) {
        return Err("registry limit must be 1..=4096".into());
    }
    let head = word(&mut read, head_address)?;
    let mut node = head;
    let mut visited = BTreeSet::new();
    let mut result = Vec::new();
    while node != 0 {
        if result.len() >= limit || !node.is_multiple_of(4) || !visited.insert(node) {
            return Err("registry cycle, misalignment or node limit".into());
        }
        let mut links = [0; 12];
        read(node, &mut links)?;
        let next = u32::from_le_bytes(links[4..8].try_into().unwrap());
        let record = u32::from_le_bytes(links[8..12].try_into().unwrap());
        if record == 0 || !record.is_multiple_of(4) {
            return Err("invalid registry record pointer".into());
        }
        let mut bytes = [0; 32];
        read(record, &mut bytes)?;
        let words: [u32; 8] = std::array::from_fn(|i| {
            u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        if words[0] == 0 || words[2] > 16 * 1024 * 1024 || words[5] > 16 * 1024 * 1024 {
            return Err("invalid registry record".into());
        }
        let mut name = Vec::new();
        for offset in 0..256u32 {
            let mut byte = [0];
            read(
                words[0]
                    .checked_add(offset)
                    .ok_or("name address overflow")?,
                &mut byte,
            )?;
            if byte[0] == 0 {
                break;
            }
            name.push(byte[0]);
        }
        if name.is_empty() || name.len() == 256 {
            return Err("empty or unterminated entity name".into());
        }
        let name = String::from_utf8(name).map_err(|_| "entity name is not UTF-8")?;
        result.push(EntityRecord {
            name,
            address: record,
            flags: words[1],
            true_size: words[2],
            true_constructor: words[3],
            true_destructor: words[4],
            false_size: words[5],
            false_constructor: words[6],
            false_destructor: words[7],
        });
        node = next;
    }
    if word(&mut read, head_address)? != head {
        return Err("entity registry changed during capture; retry".into());
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 256];
        for (at, value) in [
            (4, 32u32),
            (40, 64),
            (64, 128),
            (68, 1),
            (72, 640),
            (76, 0x401000),
            (80, 0x402000),
            (84, 640),
            (88, 0x401000),
            (92, 0x402000),
        ] {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        b[128..134].copy_from_slice(b"truck\0");
        b
    }
    #[test]
    fn reads_bounded_registry_and_rejects_cycles_unreadable_and_changed_list() {
        let bytes = fixture();
        let read = |at: u32, out: &mut [u8]| {
            let src = bytes
                .get(at as usize..at as usize + out.len())
                .ok_or("unreadable")?;
            out.copy_from_slice(src);
            Ok(())
        };
        let rows = registry(4, 8, read).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "truck");
        assert_eq!(rows[0].true_size, 640);
        assert!(registry(300, 8, read).is_err());
        assert!(registry(4, 0, read).is_err());
        let mut changed = bytes.clone();
        changed[36..40].copy_from_slice(&32u32.to_le_bytes());
        assert!(
            registry(4, 8, |at, out| {
                out.copy_from_slice(&changed[at as usize..at as usize + out.len()]);
                Ok(())
            })
            .unwrap_err()
            .contains("cycle")
        );
        let mut count = 0;
        assert!(
            registry(4, 8, |at, out| {
                read(at, out)?;
                if at == 4 {
                    count += 1;
                    if count > 1 {
                        out.copy_from_slice(&0u32.to_le_bytes());
                    }
                }
                Ok(())
            })
            .unwrap_err()
            .contains("changed")
        );
    }
}
