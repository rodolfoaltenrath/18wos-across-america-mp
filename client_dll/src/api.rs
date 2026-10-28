//! Versioned, pointer-free control ABI, also usable as a remote thread payload.
pub const ABI_VERSION: u32 = 1;
pub const START: u32 = 1;
pub const STATUS: u32 = 2;
pub const STOP: u32 = 3;
pub const SIMULATED: u32 = 1;
pub const GAME: u32 = 2;
pub const MEMORY: u32 = 3;

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
