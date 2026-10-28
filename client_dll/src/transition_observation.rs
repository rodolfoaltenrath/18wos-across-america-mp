//! Passive lifecycle observations carried by the existing v3 control block.
use crate::api::EngineControl;
use serde::Serialize;
pub const CAPACITY: usize = 128;
pub const ACTIVE: u32 = 1;
pub const TRUNCATED: u32 = 4;
pub const LOST: u32 = 32;
#[derive(Clone, Copy, Default, Debug, PartialEq, Serialize)]
pub struct Record {
    /// 1/2 init enter/return, 3/4 done enter/return, 5 first subsequent draw.
    pub kind: u32,
    pub thread: u32,
    pub caller: u32,
    pub this: u32,
    pub argument: u32,
    pub depth: u32,
    pub flushes: u32,
    /// Scene +84, physics +1d0, first model +200, model count byte +300.
    /// Bits in readable identify successful ReadProcessMemory calls.
    pub values: [u32; 4],
    pub readable: u32,
}
impl Record {
    pub fn encode(&self, b: &mut EngineControl) {
        b.fixture = [
            self.kind,
            self.thread,
            self.caller,
            self.this,
            self.argument,
            self.depth,
        ];
        b.frames[..4].copy_from_slice(&self.values);
        b.count_before = self.flushes;
        b.reserved = self.readable;
    }
    pub fn decode(b: &EngineControl) -> Self {
        Self {
            kind: b.fixture[0],
            thread: b.fixture[1],
            caller: b.fixture[2],
            this: b.fixture[3],
            argument: b.fixture[4],
            depth: b.fixture[5],
            flushes: b.count_before,
            values: b.frames[..4].try_into().unwrap(),
            readable: b.reserved,
        }
    }
    pub fn json(&self) -> serde_json::Value {
        let value = |i: usize| (self.readable & (1 << i) != 0).then_some(self.values[i]);
        serde_json::json!({"kind":match self.kind {1=>"init_enter",2=>"init_return",3=>"done_enter",4=>"done_return",5=>"first_draw",_=>"unknown"},"thread":self.thread,"caller":self.caller,"this":self.this,"argument":self.argument,"phase":self.argument as i32,"depth":self.depth,"flushes_seen":self.flushes,"scene":value(0),"physics":value(1),"first_model":value(2),"model_count":value(3),"readable":self.readable})
    }
}
#[derive(Serialize)]
pub struct Summary {
    pub state: u32,
    pub generation: u32,
    pub records: u32,
    pub owner_thread: u32,
    pub init_calls: u32,
    pub done_calls: u32,
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
            init_calls: b.count_before,
            done_calls: b.count_created,
            foreign_calls: b.count_after,
            flags: b.reserved,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_record_preserves_argument_and_unreadable_fields() {
        let r = Record {
            kind: 3,
            thread: 123,
            caller: 456,
            this: 789,
            argument: (-10i32) as u32,
            depth: 2,
            flushes: 80,
            values: [0x1000, 0x2000, 0, 2],
            readable: 11,
        };
        let mut b = EngineControl::new(46);
        r.encode(&mut b);
        assert_eq!(Record::decode(&b), r);
        assert_eq!(r.json()["kind"], "done_enter");
        assert_eq!(r.json()["phase"], -10);
        assert_eq!(Record::decode(&b).argument, 0xfffffff6);
        assert!(r.json()["first_model"].is_null());
        assert_eq!(r.json()["model_count"], 2);
    }
}
