//! Bounded, read-only render diagnostics. No engine calls or render authorization.
use crate::api::EngineControl;
use serde::Serialize;
pub const CAPACITY: usize = 128;
pub const WORLD: u32 = 1;
pub const CAMERA: u32 = 2;
pub const QUEUE: u32 = 4;
pub const TRUNCATED: u32 = 8;
#[derive(Clone, Copy, Default)]
pub struct Record {
    pub kind: u32,
    pub thread: u32,
    pub caller: u32,
    pub device: u32,
    pub pool: u32,
    pub prototypes: u32,
    pub root: u32,
    pub first: u32,
    pub first_refs: u32,
    pub buckets: u32,
    pub flags: u32,
    pub matrices: [u32; 32],
}
impl Record {
    pub fn encode(&self, b: &mut EngineControl, index: u32) {
        b.fixture = [
            self.kind,
            self.thread,
            self.caller,
            self.device,
            self.pool,
            self.prototypes,
        ];
        b.entity = self.root;
        b.constructor = self.first;
        b.destructor = self.first_refs;
        b.count_before = self.buckets;
        b.reserved = self.flags;
        b.object_size = index;
        b.frames = self.matrices;
    }
    pub fn decode(b: &EngineControl) -> Self {
        Self {
            kind: b.fixture[0],
            thread: b.fixture[1],
            caller: b.fixture[2],
            device: b.fixture[3],
            pool: b.fixture[4],
            prototypes: b.fixture[5],
            root: b.entity,
            first: b.constructor,
            first_refs: b.destructor,
            buckets: b.count_before,
            flags: b.reserved,
            matrices: b.frames,
        }
    }
    pub fn json(&self) -> serde_json::Value {
        let matrix = |start, flag| {
            let values: Vec<f32> = self.matrices[start..start + 16]
                .iter()
                .copied()
                .map(f32::from_bits)
                .collect();
            (self.flags & flag != 0 && values.iter().all(|n| n.is_finite())).then_some(values)
        };
        serde_json::json!({"kind":match self.kind {1=>"flush_enter",2=>"pool_enter",3=>"pool_return",4=>"pool_after_flush",5=>"flush_return",_=>"unknown"},"thread":self.thread,"caller":self.caller,"device":self.device,"pool":self.pool,"queue_root":self.root,"prototype_prefix":self.prototypes,"bucket_prefix":self.buckets,"first_proto":self.first,"first_proto_refs":self.first_refs,"queue_readable":self.flags & QUEUE != 0,"truncated":self.flags & TRUNCATED != 0,"empty_observed":self.pool!=0 && self.flags & (QUEUE|TRUNCATED)==QUEUE && self.prototypes==0,"world_matrix":matrix(0,WORLD),"camera_matrix":matrix(16,CAMERA)})
    }
}
#[derive(Serialize)]
pub struct Summary {
    pub state: u32,
    pub generation: u32,
    pub records: u32,
    pub owner_thread: u32,
    pub flush_calls: u32,
    pub pool_calls: u32,
    pub foreign_calls: u32,
    pub flags: u32,
}
impl Summary {
    pub fn decode(b: &EngineControl) -> Self {
        Self {
            state: b.object_size,
            generation: b.cycles,
            records: b.registry_count,
            owner_thread: b.entity,
            flush_calls: b.count_before,
            pool_calls: b.count_created,
            foreign_calls: b.count_after,
            flags: b.reserved,
        }
    }
}
/// Walk the observed two-level lists with one shared 256-node budget.
/// A valid prefix is never called empty if capture was truncated or unreadable.
pub fn queue(record: &mut Record, mut read: impl FnMut(u32, &mut [u8]) -> Result<(), ()>) {
    type Reader<'a> = dyn FnMut(u32, &mut [u8]) -> Result<(), ()> + 'a;
    let word = |at: u32, off: u32, read: &mut Reader<'_>| {
        let mut b = [0; 4];
        let addr = at.checked_add(off).ok_or(())?;
        if at == 0 || !at.is_multiple_of(4) {
            return Err(());
        }
        read(addr, &mut b)?;
        Ok(u32::from_le_bytes(b))
    };
    let result = (|| {
        record.root = word(record.pool, 0x24, &mut read)?;
        let mut outer = word(record.root, 4, &mut read)?;
        let mut seen = [0u32; 256];
        let mut used = 0;
        while outer != 0 {
            if used == seen.len() {
                record.flags |= TRUNCATED;
                break;
            }
            if seen[..used].contains(&outer) {
                return Err(());
            }
            seen[used] = outer;
            used += 1;
            record.buckets += 1;
            let bucket = word(outer, 8, &mut read)?;
            let list = word(bucket, 4, &mut read)?;
            let mut inner = word(list, 4, &mut read)?;
            while inner != 0 {
                if used == seen.len() {
                    record.flags |= TRUNCATED;
                    break;
                }
                if seen[..used].contains(&inner) {
                    return Err(());
                }
                seen[used] = inner;
                used += 1;
                let proto = word(inner, 8, &mut read)?;
                let refs = word(proto, 4, &mut read)?;
                if record.first == 0 {
                    record.first = proto;
                    record.first_refs = refs;
                }
                record.prototypes += 1;
                inner = word(inner, 4, &mut read)?;
            }
            if record.flags & TRUNCATED != 0 {
                break;
            }
            outer = word(outer, 4, &mut read)?;
        }
        Ok(())
    })();
    if result.is_ok() {
        record.flags |= QUEUE;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_lists_distinguish_live_empty_invalid_cycles_and_abi_matrix_bits() {
        let mut bytes = vec![0u8; 8192];
        let put = |b: &mut [u8], a: usize, v: u32| b[a..a + 4].copy_from_slice(&v.to_le_bytes());
        for (a, v) in [
            (0x124, 0x200),
            (0x204, 0x300),
            (0x308, 0x400),
            (0x404, 0x500),
            (0x504, 0x600),
            (0x608, 0x700),
            (0x704, 2),
        ] {
            put(&mut bytes, a, v);
        }
        let capture = |bytes: &[u8]| {
            let mut r = Record {
                pool: 0x100,
                ..Record::default()
            };
            queue(&mut r, |a, b| {
                let src = bytes.get(a as usize..a as usize + b.len()).ok_or(())?;
                b.copy_from_slice(src);
                Ok(())
            });
            r
        };
        let live = capture(&bytes);
        assert_eq!(
            (live.prototypes, live.first_refs, live.flags),
            (1, 2, QUEUE)
        );
        put(&mut bytes, 0x604, 0x600);
        assert_eq!(capture(&bytes).flags & QUEUE, 0);
        put(&mut bytes, 0x504, 0);
        assert_eq!(capture(&bytes).json()["empty_observed"], true);
        put(&mut bytes, 0x204, 0xffff_fffc);
        assert_eq!(capture(&bytes).flags & QUEUE, 0);
        let mut r = live;
        r.flags |= WORLD | CAMERA;
        r.matrices = std::array::from_fn(|i| (i as f32 + 0.25).to_bits());
        let mut b = EngineControl::new(27);
        r.encode(&mut b, 12);
        let decoded = Record::decode(&b);
        assert_eq!(decoded.matrices, r.matrices);
        assert_eq!(decoded.json()["world_matrix"][12], 12.25);
        r.matrices[0] = f32::NAN.to_bits();
        assert!(r.json()["world_matrix"].is_null());
        for i in 0..257 {
            let node = 0x800 + i * 16;
            put(
                &mut bytes,
                node + 4,
                if i == 256 { 0 } else { (node + 16) as u32 },
            );
            put(&mut bytes, node + 8, 0x400);
        }
        put(&mut bytes, 0x204, 0x800);
        let capped = capture(&bytes);
        assert_ne!(capped.flags & TRUNCATED, 0);
        assert_eq!(capped.json()["empty_observed"], false);
    }
}
