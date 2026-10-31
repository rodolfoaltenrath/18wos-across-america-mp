//! Only finite enums, booleans and counters cross the recording boundary.
//! Unknown raw fields (argv, actor/scene pointers, reasons, thread ids) are dropped.
use crate::{deferred_load::Phase, load_gate::WorldCommand};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Gate {
    pub installed: bool,
    pub phase: Option<Phase>,
    pub in_flight: bool,
    pub epoch: u32,
    pub calls: u32,
    pub deferred: u32,
    pub replays: u32,
    pub recreated: u32,
    pub dropped: u32,
    pub foreign: u32,
    pub fallback_releases: u32,
    pub arguments_restored: u32,
    pub arguments_retained: u32,
    pub restore_pending: bool,
    pub fallback_pending: bool,
    pub last_command: Option<WorldCommand>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exports_no_pointer_or_text_and_refuses_unknown_commands_and_duplicate_fields() {
        let raw = serde_json::json!({"installed":true,"phase":"idle","in_flight":false,
            "epoch":2,"calls":1,"deferred":1,"replays":1,"recreated":1,"dropped":0,
            "foreign":0,"fallback_releases":0,"arguments_restored":1,"arguments_retained":0,
            "restore_pending":false,"fallback_pending":false,"last_command":"map",
            "actor":123456,"scene":987654,"reason":"private diagnostic","argv":"private save"});
        let gate: Gate = serde_json::from_value(raw.clone()).unwrap();
        let text = serde_json::to_string(&gate).unwrap();
        assert!(!text.contains("private") && !text.contains("actor") && !text.contains("scene"));
        assert_eq!(gate.epoch, 2);
        let mut bad = raw;
        bad["last_command"] = "private command".into();
        assert!(serde_json::from_value::<Gate>(bad).is_err());
        let duplicate = text.replacen('{', "{\"epoch\":1,", 1);
        assert!(serde_json::from_str::<Gate>(&duplicate).is_err());
    }
}
