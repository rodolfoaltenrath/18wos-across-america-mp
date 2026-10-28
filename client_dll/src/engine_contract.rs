//! Counter forest for one engine-factory lock and one game-constructor lock.
//! Shared ancestors gain more than one reference; neither counter is a mutex.
use std::collections::{BTreeMap, BTreeSet};
type Reader<'a> = dyn FnMut(u32, &mut [u8]) -> Result<(), String> + 'a;
pub fn word(read: &mut Reader<'_>, at: u32) -> Result<u32, String> {
    let mut bytes = [0; 4];
    read(at, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
#[derive(Debug)]
struct Node {
    value: u32,
    increments: u32,
    parent: u32,
}
pub struct Counters {
    heads: [(u32, u32); 2],
    nodes: BTreeMap<u32, Node>,
}
impl Counters {
    pub fn capture(heads: [u32; 2], read: &mut Reader<'_>) -> Result<Self, String> {
        let mut result = Self {
            heads: [(0, 0); 2],
            nodes: BTreeMap::new(),
        };
        for (index, head) in heads.into_iter().enumerate() {
            let mut at = word(read, head)?;
            if at == 0 {
                return Err("module reference context is null".into());
            }
            result.heads[index] = (head, at);
            let mut seen = BTreeSet::new();
            while at != 0 {
                if !at.is_multiple_of(4)
                    || !seen.insert(at)
                    || seen.len() > 16
                    || at.checked_add(12).is_none()
                {
                    return Err("invalid/cyclic module reference context".into());
                }
                let count = word(read, at + 8)?;
                let parent = word(read, at + 12)?;
                let node = result.nodes.entry(at).or_insert(Node {
                    value: count,
                    increments: 0,
                    parent,
                });
                if node.value != count || node.parent != parent {
                    return Err("module count changed during capture".into());
                }
                node.increments += 1;
                node.value
                    .checked_add(node.increments)
                    .ok_or("module reference count overflow")?;
                at = parent;
            }
        }
        result.check(false, read)?;
        Ok(result)
    }
    pub fn first(&self) -> u32 {
        self.nodes[&self.heads[0].1].value
    }
    pub fn expected_created(&self) -> u32 {
        let n = &self.nodes[&self.heads[0].1];
        n.value + n.increments
    }
    pub fn check(&self, created: bool, read: &mut Reader<'_>) -> Result<(), String> {
        for &(head, root) in &self.heads {
            if word(read, head)? != root {
                return Err("module reference context changed".into());
            }
        }
        for (&at, node) in &self.nodes {
            if word(read, at + 12)? != node.parent {
                return Err("module reference parent changed".into());
            }
            let expected = node.value + if created { node.increments } else { 0 };
            let observed = word(read, at + 8)?;
            if observed != expected {
                return Err(format!(
                    "module count at 0x{at:x}: {observed}, expected {expected}"
                ));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_ancestor_counts_twice_rejects_leaks_cycles_and_overflow() {
        let mut mem = [0u32; 64];
        mem[1] = 64;
        mem[2] = 128;
        mem[18] = 7;
        mem[34] = 0;
        mem[35] = 64;
        let read = |mem: &[u32; 64], at: u32, out: &mut [u8]| {
            let value = mem.get(at as usize / 4).ok_or("unmapped")?;
            out.copy_from_slice(&value.to_le_bytes());
            Ok(())
        };
        let counters = Counters::capture([4, 8], &mut |at, out| read(&mem, at, out)).unwrap();
        assert_eq!(counters.first(), 7);
        assert_eq!(counters.expected_created(), 9);
        mem[18] = 9;
        mem[34] = 1;
        counters
            .check(true, &mut |at, out| read(&mem, at, out))
            .unwrap();
        assert!(
            counters
                .check(false, &mut |at, out| read(&mem, at, out))
                .is_err()
        );
        mem[18] = 7;
        mem[34] = 0;
        counters
            .check(false, &mut |at, out| read(&mem, at, out))
            .unwrap();
        mem[35] = 128;
        assert!(Counters::capture([4, 8], &mut |at, out| read(&mem, at, out)).is_err());
        mem[35] = 64;
        mem[18] = u32::MAX;
        assert!(Counters::capture([4, 8], &mut |at, out| read(&mem, at, out)).is_err());
    }
}
