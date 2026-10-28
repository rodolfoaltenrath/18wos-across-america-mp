//! Read-only decoding of the profiled evalue_def_t chain. Type codes, flags,
//! descriptor addresses and offsets remain observations, not a setter ABI.
use serde::Serialize;
use std::collections::BTreeSet;
#[derive(Debug, Serialize)]
pub struct EntityValue {
    pub address: u32,
    pub name: String,
    pub cached_hash: u32,
    pub descriptor: u32,
    pub type_code: u32,
    pub flags: u32,
    pub element_size: u32,
    /// Inline array capacity, or allocation growth parameter for indirect tables.
    pub table_bound: i32,
    pub value_offset: i32,
    pub auxiliary_offset: i32,
    pub count_offset: i32,
    pub get_callback: u32,
    pub set_callback: u32,
    pub clear_callback: u32,
    pub raw: [u32; 24],
}
pub fn schema(
    root: u32,
    limit: usize,
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<Vec<EntityValue>, String> {
    if root == 0 || !(1..=2048).contains(&limit) {
        return Err("schema requires a root and limit 1..=2048".into());
    }
    let mut at = root;
    let mut visited = BTreeSet::new();
    let mut rows = Vec::new();
    let mut identities = Vec::new();
    while at != 0 {
        if !at.is_multiple_of(4) || !visited.insert(at) {
            return Err("misaligned or cyclic entity value list".into());
        }
        if rows.len() >= limit {
            return Err("entity value list exceeds limit".into());
        }
        at.checked_add(95).ok_or("entity value address overflow")?;
        let mut bytes = [0; 96];
        read(at, &mut bytes)?;
        let words: [u32; 24] = std::array::from_fn(|i| {
            u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        let name_at = words[1];
        if name_at == 0 {
            return Err("entity value has a null name".into());
        }
        let mut name = Vec::new();
        let mut terminated = false;
        for index in 0..256u32 {
            let mut byte = [0];
            read(
                name_at
                    .checked_add(index)
                    .ok_or("entity value name overflow")?,
                &mut byte,
            )?;
            if byte[0] == 0 {
                terminated = true;
                break;
            }
            name.push(byte[0]);
        }
        if !terminated || name.is_empty() {
            return Err("empty or unterminated entity value name".into());
        }
        let name = String::from_utf8(name).map_err(|_| "entity value name is not UTF-8")?;
        if name.chars().any(char::is_control) {
            return Err("control character in entity value name".into());
        }
        identities.push((at, words[0], name_at));
        rows.push(EntityValue {
            address: at,
            name,
            cached_hash: words[2],
            descriptor: words[3],
            type_code: words[4],
            flags: words[5],
            element_size: words[6],
            table_bound: words[7] as i32,
            value_offset: words[8] as i32,
            auxiliary_offset: words[9] as i32,
            count_offset: words[10] as i32,
            get_callback: words[12],
            set_callback: words[13],
            clear_callback: words[14],
            raw: words,
        });
        at = words[0];
    }
    // Hash/cache flag fields can change lazily. Structural links and names cannot.
    for (at, next, name) in identities {
        let mut bytes = [0; 8];
        read(at, &mut bytes)?;
        if u32::from_le_bytes(bytes[..4].try_into().unwrap()) != next
            || u32::from_le_bytes(bytes[4..].try_into().unwrap()) != name
        {
            return Err("entity value list changed during capture".into());
        }
    }
    Ok(rows)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 768];
        for (at, value) in [
            (64, 160u32),
            (68, 320),
            (80, 14),
            (92, u32::MAX),
            (96, 48),
            (116, 0x401000),
            (164, 336),
            (176, 99),
            (188, u32::MAX),
            (192, u32::MAX),
        ] {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        b[320..326].copy_from_slice(b"model\0");
        b[336..345].copy_from_slice(b"position\0");
        b
    }
    fn read(b: &[u8], at: u32, out: &mut [u8]) -> Result<(), String> {
        out.copy_from_slice(
            b.get(at as usize..at as usize + out.len())
                .ok_or("unreadable fixture")?,
        );
        Ok(())
    }
    #[test]
    fn reads_own_inherited_metadata_without_executing_callbacks_or_guessing_types() {
        let b = fixture();
        let rows = schema(64, 16, |at, out| read(&b, at, out)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "model");
        assert_eq!(rows[0].value_offset, 48);
        assert_eq!(rows[0].set_callback, 0x401000);
        assert_eq!(rows[1].type_code, 99);
        assert_eq!(rows[1].value_offset, -1);
        assert!(schema(64, 1, |at, out| read(&b, at, out)).is_err());
        assert!(schema(0, 16, |at, out| read(&b, at, out)).is_err());
    }
    #[test]
    fn rejects_cycles_bad_names_overflow_unreadable_and_structural_changes() {
        let b = fixture();
        let mut cycle = b.clone();
        cycle[160..164].copy_from_slice(&64u32.to_le_bytes());
        assert!(
            schema(64, 16, |at, out| read(&cycle, at, out))
                .unwrap_err()
                .contains("cyclic")
        );
        let mut bad = b.clone();
        bad[320..576].fill(b'x');
        assert!(
            schema(64, 16, |at, out| read(&bad, at, out))
                .unwrap_err()
                .contains("unterminated")
        );
        assert!(schema(u32::MAX - 3, 16, |_, _| unreachable!()).is_err());
        assert!(schema(65, 16, |_, _| unreachable!()).is_err());
        assert!(schema(64, 16, |_, _| Err("unmapped".into())).is_err());
        assert!(
            schema(64, 16, |at, out| {
                read(&b, at, out)?;
                if at == 64 && out.len() == 8 {
                    out[..4].fill(0);
                }
                Ok(())
            })
            .unwrap_err()
            .contains("changed")
        );
    }
}
