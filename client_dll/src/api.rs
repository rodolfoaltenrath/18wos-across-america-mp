//! Versioned, pointer-free control ABI, also usable as a remote thread payload.
pub const ABI_VERSION: u32 = 1;
pub const START: u32 = 1;
pub const STATUS: u32 = 2;
pub const STOP: u32 = 3;
pub const SIMULATED: u32 = 1;
pub const GAME: u32 = 2;
pub const MEMORY: u32 = 3;

pub const UI_ATTACH: u32 = 1;
pub const UI_PROBE: u32 = 2;
pub const UI_REGISTRY: u32 = 3;
pub const UI_DETACH: u32 = 4;
pub const UI_STATUS: u32 = 5;
pub const UI_INSPECT_TYPE: u32 = 6;
pub const UI_TRACE: u32 = 7;
pub const UI_PUMP_ATTACH: u32 = 8;
pub const UI_PUMP_FIXTURE: u32 = 9;
pub const UI_PUMP_STATUS: u32 = 10;
pub const UI_PUMP_REMOVE: u32 = 11;
pub const PHASE_PROBE: u32 = 12;
pub const PRIVATE_CYCLE: u32 = 13;
pub const PRIVATE_FIXTURE: u32 = 14;
pub const ENGINE_ABI_VERSION: u32 = 3;
/// Separate extension ABI; does not change the original Control v1 layout.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EngineControl {
    pub version: u32,
    pub size: u32,
    pub command: u32,
    pub result: u32,
    pub window: u32,
    pub window_thread: u32,
    pub executed_thread: u32,
    pub active: u32,
    pub completed: u32,
    pub inflight: u32,
    pub registry_count: u32,
    pub reserved: u32,
    pub entity: u32,
    pub object_size: u32,
    pub constructor: u32,
    pub destructor: u32,
    pub frame_count: u32,
    pub executable_base: u32,
    pub executable_frame: u32,
    pub phase: u32,
    pub phase_thread: u32,
    pub phase_site: u32,
    pub phase_hits: u32,
    pub phase_pending: u32,
    pub phase_running: u32,
    pub phase_completed: u32,
    pub cycles: u32,
    pub count_before: u32,
    pub count_created: u32,
    pub count_after: u32,
    pub object_vtable: u32,
    pub object_destructor: u32,
    /// Own-fixture only: record, factory, destroy, engine head, game head, vtable.
    pub fixture: [u32; 6],
    pub frames: [u32; 32],
    pub name: [u8; 128],
    pub message: [u8; 256],
}
impl EngineControl {
    pub fn new(command: u32) -> Self {
        Self {
            version: ENGINE_ABI_VERSION,
            size: std::mem::size_of::<Self>() as u32,
            command,
            result: 0,
            window: 0,
            window_thread: 0,
            executed_thread: 0,
            active: 0,
            completed: 0,
            inflight: 0,
            registry_count: 0,
            reserved: 0,
            entity: 0,
            object_size: 0,
            constructor: 0,
            destructor: 0,
            frame_count: 0,
            executable_base: 0,
            executable_frame: 0,
            phase: 0,
            phase_thread: 0,
            phase_site: 0,
            phase_hits: 0,
            phase_pending: 0,
            phase_running: 0,
            phase_completed: 0,
            cycles: 0,
            count_before: 0,
            count_created: 0,
            count_after: 0,
            object_vtable: 0,
            object_destructor: 0,
            fixture: [0; 6],
            frames: [0; 32],
            name: [0; 128],
            message: [0; 256],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ClientConfig {
    pub version: u32,
    pub size: u32,
    pub mode: u32,
    pub phase: f32,
    pub server: [u8; 128],
    pub profile: [u8; 64],
    pub addresses: [[u8; 256]; 3],
}
impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            version: ABI_VERSION,
            size: std::mem::size_of::<Self>() as u32,
            mode: SIMULATED,
            phase: 0.0,
            server: [0; 128],
            profile: [0; 64],
            addresses: [[0; 256]; 3],
        }
    }
}
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct Control {
    pub version: u32,
    pub size: u32,
    pub command: u32,
    pub result: u32,
    pub status: u32,
    /// 0 = no sample, 1 = available, 2 = temporarily unavailable, 3 = fatal.
    pub sample_state: u32,
    pub sent: u64,
    pub received: u64,
    pub read_failures: u64,
    pub config: ClientConfig,
    /// Last diagnostic as NUL-terminated UTF-8, truncated at a character boundary.
    pub message: [u8; 512],
}
impl Control {
    pub fn new(command: u32) -> Self {
        Self {
            version: ABI_VERSION,
            size: std::mem::size_of::<Self>() as u32,
            command,
            result: 0,
            status: 0,
            sample_state: 0,
            sent: 0,
            received: 0,
            read_failures: 0,
            config: ClientConfig::default(),
            message: [0; 512],
        }
    }
}
pub fn text(bytes: &[u8]) -> Result<&str, String> {
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or("unterminated ABI string")?;
    std::str::from_utf8(&bytes[..end]).map_err(|_| "invalid UTF-8 ABI string".into())
}
pub fn put(bytes: &mut [u8], value: &str) -> Result<(), String> {
    if value.len() >= bytes.len() || value.contains('\0') {
        return Err("ABI string too long or contains NUL".into());
    }
    bytes.fill(0);
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_layout_and_bounded_strings() {
        assert_eq!(std::mem::size_of::<EngineControl>(), 664);
        assert_eq!(std::mem::offset_of!(EngineControl, name), 280);
        assert_eq!(std::mem::size_of::<ClientConfig>(), 976);
        assert_eq!(std::mem::size_of::<Control>(), 1536);
        assert_eq!(std::mem::offset_of!(Control, sent), 24);
        assert_eq!(std::mem::offset_of!(Control, config), 48);
        assert_eq!(std::mem::offset_of!(Control, message), 1024);
        let mut field = [0; 8];
        put(&mut field, "olá").unwrap();
        assert_eq!(text(&field).unwrap(), "olá");
        assert!(put(&mut field, "12345678").is_err());
        assert!(text(&[1; 8]).is_err());
    }
}
