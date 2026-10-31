//! Pointer-free evidence required before releasing the controller's DLL lease.
//! Stopping a worker is insufficient when callbacks or engine resources remain.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub schema: u32,
    pub worker_joined: bool,
    pub cleanup_complete: bool,
    pub dispatcher_detached: bool,
    pub process_pinned: bool,
    pub phase_pending: u32,
    pub phase_running: u32,
    pub visual_retained: bool,
}
impl State {
    pub fn require_stopped(&self) -> Result<(), &'static str> {
        if self.schema != 1 {
            return Err("unknown lifecycle schema; DLL retained");
        }
        if !self.worker_joined || !self.cleanup_complete {
            return Err("worker or engine cleanup incomplete; DLL retained");
        }
        if !self.dispatcher_detached || self.phase_pending != 0 || self.phase_running != 0 {
            return Err("callbacks or queued operations remain; DLL retained");
        }
        if self.visual_retained {
            return Err("engine visual remains allocated; DLL retained");
        }
        Ok(())
    }
    pub fn require_unload(&self) -> Result<(), &'static str> {
        self.require_stopped()?;
        if self.process_pinned {
            return Err(
                "observer pins DLL until process exit; stop is complete but unload is unavailable",
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unload_requires_every_piece_of_evidence_and_known_schema() {
        let clean = State {
            schema: 1,
            worker_joined: true,
            cleanup_complete: true,
            dispatcher_detached: true,
            ..State::default()
        };
        assert!(clean.require_unload().is_ok());
        for changed in [
            State {
                schema: 2,
                ..clean.clone()
            },
            State {
                worker_joined: false,
                ..clean.clone()
            },
            State {
                cleanup_complete: false,
                ..clean.clone()
            },
            State {
                dispatcher_detached: false,
                ..clean.clone()
            },
            State {
                process_pinned: true,
                ..clean.clone()
            },
            State {
                phase_pending: 1,
                ..clean.clone()
            },
            State {
                phase_running: 1,
                ..clean.clone()
            },
            State {
                visual_retained: true,
                ..clean.clone()
            },
        ] {
            assert!(changed.require_unload().is_err());
        }
        let json = serde_json::to_string(&clean).unwrap();
        assert!(json.len() < 512);
        assert!(
            serde_json::from_str::<State>(&json)
                .unwrap()
                .require_unload()
                .is_ok()
        );
        assert!(serde_json::from_str::<State>("{}").is_err());
        let extra = json.replace("\"schema\":1", "\"schema\":1,\"unload_allowed\":true");
        assert!(serde_json::from_str::<State>(&extra).is_err());
        let pinned = State {
            process_pinned: true,
            ..clean
        };
        assert!(pinned.require_stopped().is_ok());
        assert!(pinned.require_unload().is_err());
    }
}
