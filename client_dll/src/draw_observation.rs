//! Bounded, read-only actors.car draw diagnostics. Not a draw authorization.
use crate::api::EngineControl;
use serde::Serialize;
pub const CAPACITY: usize = 32;
/// World stack in the core: head pointer, then 16 entries of matrix+inverse+cache.
pub const STACK_ENTRY: u32 = 0x84;
pub const STACK_ENTRIES: u32 = 16;
pub const WORLD: u32 = 1;
pub const CAMERA: u32 = 2;
pub const BODY: u32 = 4;
pub const DEPTH: u32 = 8;
#[derive(Clone, Copy, Default)]
pub struct Record {
    pub kind: u32,
    pub thread: u32,
    pub caller: u32,
    pub this: u32,
    pub vtable: u32,
    pub state: u32,
    pub body: u32,
    pub depth: u32,
    pub path: u32,
    pub path_info: u32,
    pub viewport: u32,
    pub flushes: u32,
    pub flags: u32,
    /// Quaternion x,y,z,w then position x,y,z, as raw f32 bits.
    pub pose: [u32; 7],
    pub matrices: [u32; 32],
}
/// Zero-based entry index of the current world top, if aligned inside the stack.
pub fn depth(head: u32, top: u32) -> Option<u32> {
    let offset = top.checked_sub(head.checked_add(4)?)?;
    (offset.is_multiple_of(STACK_ENTRY) && offset / STACK_ENTRY < STACK_ENTRIES)
        .then_some(offset / STACK_ENTRY)
}
impl Record {
    pub fn encode(&self, b: &mut EngineControl, index: u32) {
        b.fixture = [
            self.kind,
            self.thread,
            self.caller,
            self.this,
            self.vtable,
            self.state,
        ];
        b.entity = self.body;
        b.constructor = self.depth;
        b.destructor = self.path;
        b.count_before = self.path_info;
        b.count_created = self.viewport;
        b.count_after = self.flushes;
        b.reserved = self.flags;
        b.object_size = index;
        b.name = [0; 128];
        for (out, v) in b.name.as_chunks_mut::<4>().0.iter_mut().zip(self.pose) {
            *out = v.to_le_bytes();
        }
        b.frames = self.matrices;
    }
    pub fn decode(b: &EngineControl) -> Self {
        let mut pose = [0; 7];
        for (out, v) in pose.iter_mut().zip(b.name.as_chunks::<4>().0) {
            *out = u32::from_le_bytes(*v);
        }
        Self {
            kind: b.fixture[0],
            thread: b.fixture[1],
            caller: b.fixture[2],
            this: b.fixture[3],
            vtable: b.fixture[4],
            state: b.fixture[5],
            body: b.entity,
            depth: b.constructor,
            path: b.destructor,
            path_info: b.count_before,
            viewport: b.count_created,
            flushes: b.count_after,
            flags: b.reserved,
            pose,
            matrices: b.frames,
        }
    }
    pub fn json(&self) -> serde_json::Value {
        let floats = |values: &[u32], flag| {
            let values: Vec<f32> = values.iter().copied().map(f32::from_bits).collect();
            (self.flags & flag != 0 && values.iter().all(|n| n.is_finite())).then_some(values)
        };
        serde_json::json!({"kind":match self.kind {1=>"draw_enter",2=>"draw_return",_=>"unknown"},"thread":self.thread,"caller":self.caller,"this":self.this,"vtable":self.vtable,"state":self.state,"body":self.body,"world_depth":(self.flags & DEPTH != 0).then_some(self.depth),"path":self.path,"path_info":self.path_info,"viewport":self.viewport,"flushes_seen":self.flushes,"quaternion":floats(&self.pose[..4],BODY),"position":floats(&self.pose[4..],BODY),"world_matrix":floats(&self.matrices[..16],WORLD),"camera_matrix":floats(&self.matrices[16..],CAMERA)})
    }
}
#[derive(Serialize)]
pub struct Summary {
    pub state: u32,
    pub generation: u32,
    pub records: u32,
    pub owner_thread: u32,
    pub draw_calls: u32,
    pub flush_calls: u32,
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
            draw_calls: b.count_before,
            flush_calls: b.count_created,
            foreign_calls: b.count_after,
            flags: b.reserved,
        }
    }
}
/// Test visual status, carried in existing fields of the v3 block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Visual {
    pub state: u32,
    pub shown: u32,
    pub fault: u32,
    pub item: u32,
    pub drawn: u32,
    pub skipped: u32,
    /// Item access counter (+0x58); u32::MAX when unreadable.
    pub access: u32,
    pub last_draw_flush: u32,
    pub flushes: u32,
    pub offset: [f32; 3],
}
impl Visual {
    pub fn encode(&self, b: &mut EngineControl) {
        b.object_size = self.state;
        b.reserved = self.shown | (self.fault << 1);
        b.entity = self.item;
        b.count_before = self.drawn;
        b.count_created = self.skipped;
        b.count_after = self.access;
        b.cycles = self.last_draw_flush;
        b.registry_count = self.flushes;
        b.object_vtable = self.offset[0].to_bits();
        b.object_destructor = self.offset[1].to_bits();
        b.constructor = self.offset[2].to_bits();
    }
    pub fn decode(b: &EngineControl) -> Self {
        Self {
            state: b.object_size,
            shown: b.reserved & 1,
            fault: b.reserved >> 1,
            item: b.entity,
            drawn: b.count_before,
            skipped: b.count_created,
            access: b.count_after,
            last_draw_flush: b.cycles,
            flushes: b.registry_count,
            offset: [b.object_vtable, b.object_destructor, b.constructor].map(f32::from_bits),
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn visual_status_roundtrip() {
        let v = super::Visual {
            state: 1,
            shown: 1,
            fault: 1,
            item: 0x1234,
            drawn: 9,
            skipped: 2,
            access: 1,
            last_draw_flush: 40,
            flushes: 48,
            offset: [400.0, 0.0, -1.5],
        };
        let mut b = crate::api::EngineControl::new(40);
        v.encode(&mut b);
        assert_eq!(super::Visual::decode(&b), v);
    }
    use super::*;
    #[test]
    fn stack_depth_pose_and_matrix_bits_roundtrip() {
        assert_eq!(depth(0x1000, 0x1004), Some(0));
        assert_eq!(depth(0x1000, 0x1004 + 2 * STACK_ENTRY), Some(2));
        assert_eq!(depth(0x1000, 0x1004 + 15 * STACK_ENTRY), Some(15));
        assert_eq!(depth(0x1000, 0x1004 + 16 * STACK_ENTRY), None);
        assert_eq!(depth(0x1000, 0x1005), None);
        assert_eq!(depth(0x1000, 0x1000), None);
        assert_eq!(depth(u32::MAX, 0), None);
        let r = Record {
            kind: 1,
            this: 0x2000,
            depth: 3,
            flags: WORLD | BODY | DEPTH,
            pose: std::array::from_fn(|i| (i as f32 * 0.5).to_bits()),
            matrices: std::array::from_fn(|i| (i as f32 + 0.25).to_bits()),
            ..Record::default()
        };
        let mut b = EngineControl::new(34);
        r.encode(&mut b, 7);
        assert_eq!(b.object_size, 7);
        let d = Record::decode(&b);
        assert_eq!((d.pose, d.matrices, d.depth), (r.pose, r.matrices, 3));
        let json = d.json();
        assert_eq!(json["kind"], "draw_enter");
        assert_eq!(json["world_depth"], 3);
        assert_eq!(json["position"][2], 3.0);
        assert_eq!(json["quaternion"][1], 0.5);
        assert_eq!(json["world_matrix"][12], 12.25);
        assert!(json["camera_matrix"].is_null());
        let mut nan = d;
        nan.pose[0] = f32::NAN.to_bits();
        nan.flags &= !DEPTH;
        assert!(nan.json()["quaternion"].is_null());
        assert!(nan.json()["world_depth"].is_null());
    }
}
