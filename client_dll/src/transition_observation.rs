//! Passive lifecycle observations carried by the existing v3 control block.
use crate::api::EngineControl;
use serde::Serialize;
pub const CAPACITY: usize = 512;
pub const ACTIVE: u32 = 1;
pub const TRUNCATED: u32 = 4;
pub const LOST: u32 = 32;
/// Per-record readability bits (separate from Summary.flags).
pub const COMMAND_READ: u32 = 1 << 5;
pub const COMMAND_UNREADABLE: u32 = 1 << 6;
pub const COMMAND_UNTERMINATED: u32 = 1 << 7;
/// Copies argv[0] only, including a terminator within 32 bytes. On failure no
/// partial name is retained. Does not read argv[1] or any command arguments.
pub fn command_name(
    argv: u32,
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> ([u8; 32], u32) {
    let mut pointer = [0; 4];
    if argv == 0 || read(argv, &mut pointer).is_err() {
        return ([0; 32], COMMAND_UNREADABLE);
    }
    let pointer = u32::from_le_bytes(pointer);
    if pointer == 0 {
        return ([0; 32], COMMAND_UNREADABLE);
    }
    let mut name = [0; 32];
    for i in 0..name.len() {
        let Some(at) = pointer.checked_add(i as u32) else {
            return ([0; 32], COMMAND_UNREADABLE);
        };
        if read(at, &mut name[i..i + 1]).is_err() {
            return ([0; 32], COMMAND_UNREADABLE);
        }
        if name[i] == 0 {
            return (name, COMMAND_READ);
        }
    }
    ([0; 32], COMMAND_UNTERMINATED)
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Serialize)]
pub struct Record {
    /// 1/2 init enter/return, 3/4 done enter/return, 5 first subsequent draw;
    /// 6/7 block init enter/return, 8/9 destruction REQUEST enter/return;
    /// 10 profiled command dispatch (after cmd_argv, before command execution).
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
    /// For kind 10, context[0] is the handler's scene pointer (+0x14c), not block data.
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
        let command = (self.kind == 10 && self.readable & COMMAND_READ != 0)
            .then(|| crate::api::text(&self.command).ok())
            .flatten();
        let command_status = (self.kind == 10).then_some(if self.readable & COMMAND_READ != 0 {
            if command.is_some() {
                "complete"
            } else {
                "invalid_utf8"
            }
        } else if self.readable & COMMAND_UNTERMINATED != 0 {
            "unterminated"
        } else if self.readable & COMMAND_UNREADABLE != 0 {
            "unreadable"
        } else {
            "not_recorded"
        });
        let block_context = |i| if block { context(i) } else { None };
        serde_json::json!({"kind":match self.kind {1=>"init_enter",2=>"init_return",3=>"done_enter",4=>"done_return",5=>"first_draw",6=>"block_init_enter",7=>"block_init_return",8=>"destroy_request_enter",9=>"destroy_request_return",10=>"command_dispatch",_=>"unknown"},"command":command,"command_status":command_status,"handler_scene":(self.kind==10).then(||context(0)).flatten(),"thread":self.thread,"caller":self.caller,"this":self.this,"argument":self.argument,"phase":(1..=4).contains(&self.kind).then_some(self.argument as i32),"depth":self.depth,"flushes_seen":self.flushes,"scene":value(0),"physics":value(1),"first_model":value(2),"model_count":value(3),"readable":self.readable,
        "observed_car":self.context[5],"block":block.then_some(self.argument),"block_data":block_context(0),"block_context":block_context(1),"block_strong":block_context(2),"block_residency":block_context(3),"block_access":block_context(4),"queue_words":[block_context(6),block_context(7)],"returned":matches!(self.kind,7|9).then_some(self.returned != 0),"models":(self.readable & 16 != 0).then(|| &self.models[..(self.values[3] as usize).min(16)]),"models_truncated":self.readable & 8 != 0 && self.values[3]>16})
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
    fn command_capture_reads_only_argv_zero_and_drops_partial_or_unterminated_names() {
        let capture = |text: &[u8]| {
            let mut reads = Vec::new();
            let result = command_name(64, |at, out| {
                reads.push((at, out.len()));
                if at == 64 {
                    assert_eq!(out.len(), 4);
                    out.copy_from_slice(&128u32.to_le_bytes());
                    return Ok(());
                }
                assert!((128..160).contains(&at));
                assert_eq!(out.len(), 1);
                out.copy_from_slice(
                    text.get(at as usize - 128..at as usize - 127)
                        .ok_or("unreadable name")?,
                );
                Ok(())
            });
            (result, reads)
        };
        let ((name, flags), reads) = capture(b"load\0");
        assert_eq!(flags, COMMAND_READ);
        assert_eq!(crate::api::text(&name).unwrap(), "load");
        assert_eq!(reads.len(), 6); // One pointer and exactly five bytes, never argv[1].
        assert_eq!(capture(b"lo").0, ([0; 32], COMMAND_UNREADABLE));
        assert_eq!(capture(&[b'x'; 32]).0, ([0; 32], COMMAND_UNTERMINATED));
        let mut maximal = [b'x'; 32];
        maximal[31] = 0;
        assert_eq!(capture(&maximal).0, (maximal, COMMAND_READ));
        assert_eq!(
            command_name(0, |_, _| unreachable!()),
            ([0; 32], COMMAND_UNREADABLE)
        );
        assert_eq!(
            command_name(64, |_, _| Err("unmapped".into())),
            ([0; 32], COMMAND_UNREADABLE)
        );
        assert_eq!(
            command_name(64, |_, out| {
                out.fill(0);
                Ok(())
            }),
            ([0; 32], COMMAND_UNREADABLE)
        );
        assert_eq!(
            command_name(64, |at, out| {
                if at == 64 {
                    out.copy_from_slice(&(u32::MAX - 1).to_le_bytes());
                } else {
                    out.fill(b'x');
                }
                Ok(())
            }),
            ([0; 32], COMMAND_UNREADABLE)
        );
    }
    #[test]
    fn command_record_preserves_scene_and_distinguishes_invalid_data_from_a_name() {
        let mut r = Record {
            kind: 10,
            readable: COMMAND_READ | 0x100,
            context: [0x7000, 0, 0, 0, 0, 0x6000, 0, 0],
            ..Record::default()
        };
        crate::api::put(&mut r.command, "load").unwrap();
        let mut b = EngineControl::new(46);
        r.encode(&mut b);
        assert_eq!(Record::decode(&b), r);
        let j = r.json();
        assert_eq!(j["command"], "load");
        assert_eq!(j["command_status"], "complete");
        assert_eq!(j["handler_scene"], 0x7000);
        assert!(j["block_data"].is_null());
        assert!(j["phase"].is_null());
        r.command[0] = 0xff;
        assert_eq!(r.json()["command_status"], "invalid_utf8");
        assert!(r.json()["command"].is_null());
        r.readable = COMMAND_UNTERMINATED;
        assert_eq!(r.json()["command_status"], "unterminated");
        r.readable = COMMAND_UNREADABLE;
        assert_eq!(r.json()["command_status"], "unreadable");
    }
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
