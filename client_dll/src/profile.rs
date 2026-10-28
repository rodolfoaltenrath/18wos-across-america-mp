//! Only the locally investigated Steam module is supported. No game bytes included.
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub const ID: &str = "steam-aa-1.10-ccb327c2";
pub const SHA256: &str = "ccb327c2a5c2f7d01d74d94629cf65847ee1535ca9020b3a5fc5d412a8ffbc7d";
pub const ENGINE_SHA256: &str = "7df43b55e6b34e3ac2017af9566665bd935895ff02a2a916b099bb34045d30a3";
pub const ENTITY_REGISTRY_HEAD: u32 = 0x49a7c;
pub const EXECUTABLE_SHA256: &str =
    "949be316b38eaf4d80d4f7f6a3e7ae040e6c6072c92267b3d10066140058f61e";
pub const PEEK_MESSAGE_IAT: u32 = 0x22188;
pub const PUMP_RETURN_SITES: [u32; 3] = [0x1473b, 0x1476e, 0x147a4];
pub const ENGINE_CONTEXT: u32 = 0x51314;
pub const GAME_CONTEXT: u32 = 0x160a24;
pub const CREATE_FACTORY: u32 = 0xcba0;
pub const DESTROY_FACTORY: u32 = 0xcc40;
pub const STRING_SETTER: u32 = 0x1b10;
pub const CAR_RECORD: u32 = 0x127a34;
pub const CAR_CONSTRUCTOR: u32 = 0xcef70;
pub const CAR_DESTRUCTOR: u32 = 0xd1820;
pub const CAR_SIZE: u32 = 0x2118;
pub const CAR_VTABLE: u32 = 0x1257a0;
pub const CAR_VALUES: u32 = 0x15fdb8;
#[derive(Clone, Copy)]
pub struct EntityType {
    pub name: &'static str,
    pub record: u32,
    pub size: u32,
    pub constructor: u32,
    pub destructor: u32,
    pub vtable: u32,
    pub values: u32,
}
pub const CAR: EntityType = EntityType {
    name: "actors.car",
    record: CAR_RECORD,
    size: CAR_SIZE,
    constructor: CAR_CONSTRUCTOR,
    destructor: CAR_DESTRUCTOR,
    vtable: CAR_VTABLE,
    values: CAR_VALUES,
};
/// Candidate visual type: only private lifetime is enabled; no init/render ABI.
pub const PMD: EntityType = EntityType {
    name: "decals.pmd",
    record: 0x12a318,
    size: 0xa8c,
    constructor: 0xf9c70,
    destructor: 0xf9980,
    vtable: 0x12ac68,
    values: 0x15bd00,
};
pub fn entity_type(name: &str) -> Result<EntityType, String> {
    match name {
        "" | "actors.car" => Ok(CAR),
        "decals.pmd" => Ok(PMD),
        _ => Err("entity type is not profiled".into()),
    }
}
pub const POSITION: &str = "game.dll+0x160df0->0x14c->0x4610->0x8->0x8->0x1d0->0x3e4->0x30";

pub fn verify_module(path: &Path) -> Result<(), String> {
    verify(path, SHA256)
}
pub fn verify_engine(path: &Path) -> Result<(), String> {
    verify(path, ENGINE_SHA256)
}
pub fn verify_executable(path: &Path) -> Result<(), String> {
    verify(path, EXECUTABLE_SHA256)
}
fn verify(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut header = [0; 64];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[..2] != b"MZ" {
        return Err("module has no MZ signature".into());
    }
    let pe = u32::from_le_bytes(header[60..64].try_into().unwrap());
    file.seek(SeekFrom::Start(pe.into()))
        .map_err(|e| e.to_string())?;
    let mut coff = [0; 6];
    file.read_exact(&mut coff).map_err(|e| e.to_string())?;
    if &coff[..4] != b"PE\0\0" || u16::from_le_bytes(coff[4..].try_into().unwrap()) != 0x14c {
        return Err("profile requires a PE x86 module".into());
    }
    file.rewind().map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hash.finalize());
    if actual != expected {
        return Err(format!(
            "incompatible module SHA-256: {actual}; expected {expected}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_other_versions_and_architectures_using_own_fixture() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("own.dll");
        let mut data = [0; 70];
        data[..2].copy_from_slice(b"MZ");
        data[60..64].copy_from_slice(&64u32.to_le_bytes());
        data[64..68].copy_from_slice(b"PE\0\0");
        data[68..].copy_from_slice(&0x14cu16.to_le_bytes());
        std::fs::write(&path, data).unwrap();
        let hash = format!("{:x}", Sha256::digest(data));
        verify(&path, &hash).unwrap();
        assert!(verify_module(&path).unwrap_err().contains("incompatible"));
        data[68..].copy_from_slice(&0x8664u16.to_le_bytes());
        std::fs::write(&path, data).unwrap();
        assert!(verify(&path, &hash).unwrap_err().contains("x86"));
    }
}
