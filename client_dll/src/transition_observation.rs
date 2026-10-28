//! Passive lifecycle observations carried by the existing v3 control block.
use crate::api::EngineControl;
use serde::Serialize;
pub const CAPACITY: usize = 512;
pub const ACTIVE: u32 = 1;
pub const TRUNCATED: u32 = 4;
pub const LOST: u32 = 32;
#[derive(Clone, Copy, Default, Debug, PartialEq, Serialize)]
pub struct Record {
    /// 1/2 init enter/return, 3/4 done enter/return, 5 first subsequent draw;
    /// 6/7 block init enter/return, 8/9 destruction REQUEST enter/return.
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
    /// Block data/context/strong/residency/access, observed car, queue raw words.
    pub context: [u32; 8],
    pub returned: u32,
    pub models: [u32; 16],
    pub command: [u8; 32],
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
        b.frames[4..12].copy_from_slice(&self.context);
        b.frames[12] = self.returned;
        b.frames[13..29].copy_from_slice(&self.models);
        b.name[..32].copy_from_slice(&self.command);
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
            context: b.frames[4..12].try_into().unwrap(),
            returned: b.frames[12],
            models: b.frames[13..29].try_into().unwrap(),
            command: b.name[..32].try_into().unwrap(),
        }
    }
    pub fn json(&self) -> serde_json::Value {
        let value = |i: usize| (self.readable & (1 << i) != 0).then_some(self.values[i]);
        let context = |i: usize| (self.readable & (1 << (8 + i)) != 0).then_some(self.context[i]);
        let block = (6..=9).contains(&self.kind);
        let command = (self.kind == 10 && self.readable & 32 != 0)
            .then(|| crate::api::text(&self.command).ok())
            .flatten();
        serde_json::json!({"kind":match self.kind {1=>"init_enter",2=>"init_return",3=>"done_enter",4=>"done_return",5=>"first_draw",6=>"block_init_enter",7=>"block_init_return",8=>"destroy_request_enter",9=>"destroy_request_return",10=>"command_dispatch",_=>"unknown"},"command":command,"thread":self.thread,"caller":self.caller,"this":self.this,"argument":self.argument,"phase":(self.kind<=4).then_some(self.argument as i32),"depth":self.depth,"flushes_seen":self.flushes,"scene":value(0),"physics":value(1),"first_model":value(2),"model_count":value(3),"readable":self.readable,
        "observed_car":self.context[5],"block":block.then_some(self.argument),"block_data":context(0),"block_context":context(1),"block_strong":context(2),"block_residency":context(3),"block_access":context(4),"queue_words":[context(6),context(7)],"returned":matches!(self.kind,7|9).then_some(self.returned != 0),"models":(self.readable & 16 != 0).then(|| &self.models[..(self.values[3] as usize).min(16)]),"models_truncated":self.readable & 8 != 0 && self.values[3]>16})
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
    pub block_init_calls: u32,
    pub destroy_requests: u32,
    pub blocks_installed: bool,
    pub command_calls: u32,
    pub commands_installed: bool,
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
            block_init_calls: b.frames[0],
            destroy_requests: b.frames[1],
            blocks_installed: b.frames[2] != 0,
            command_calls: b.frames[3],
            commands_installed: b.frames[4] != 0,
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
            ..Record::default()
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
    #[test]
    fn block_record_keeps_identity_boolean_and_bounded_models_in_the_v3_payload() {
        let r = Record {
            kind: 7,
            argument: 0x2000,
            values: [0x3000, 0, 0, 20],
            context: [0x4000, 0x5000, 1, 1, 0, 0x6000, 0x7000, 1],
            readable: 31 | 0xdf00,
            returned: 0,
            models: std::array::from_fn(|i| i as u32 + 10),
            ..Record::default()
        };
        let mut b = EngineControl::new(46);
        r.encode(&mut b);
        assert_eq!(Record::decode(&b), r);
        let json = r.json();
        assert!(json["phase"].is_null());
        assert_eq!(json["returned"], false);
        assert_eq!(json["block_data"], 0x4000);
        assert_eq!(json["observed_car"], 0x6000);
        assert_eq!(json["models"].as_array().unwrap().len(), 16);
        assert_eq!(json["models_truncated"], true);
        assert_eq!(json["queue_words"][1], 1);
        let absent = Record { readable: 0, ..r };
        assert!(absent.json()["models"].is_null());
        assert!(absent.json()["block_strong"].is_null());
    }
}
