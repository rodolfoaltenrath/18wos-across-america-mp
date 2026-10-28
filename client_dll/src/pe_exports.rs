//! Bounded PE32 export resolution from the TARGET's loaded image, including forwarders.
#[derive(Clone)]
pub struct Image {
    pub name: String,
    pub base: u64,
    pub size: u32,
}

pub fn resolve(
    images: &[Image],
    module: &str,
    symbol: &str,
    mut read: impl FnMut(u64, &mut [u8]) -> Result<(), String>,
) -> Result<u64, String> {
    let mut module = module.to_owned();
    let mut symbol = symbol.to_owned();
    for _ in 0..8 {
        let image = images
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(&module))
            .ok_or_else(|| format!("target module {module} not loaded"))?;
        let mut at = |rva: u32, len: usize| -> Result<Vec<u8>, String> {
            if u64::from(rva) + len as u64 > u64::from(image.size) {
                return Err("export RVA outside image".into());
            }
            let mut bytes = vec![0; len];
            read(
                image
                    .base
                    .checked_add(rva.into())
                    .ok_or("image base overflow")?,
                &mut bytes,
            )?;
            Ok(bytes)
        };
        let dos = at(0, 64)?;
        if &dos[..2] != b"MZ" {
            return Err("target module has no MZ signature".into());
        }
        let nt = u32::from_le_bytes(dos[60..64].try_into().unwrap());
        let header = at(nt, 120)?;
        if &header[..4] != b"PE\0\0" || word(&header, 4) != 0x14c || word(&header, 24) != 0x10b {
            return Err("remote calls require PE32 x86".into());
        }
        // PE32: export directory starts at optional-header+96 = NT+120.
        let directories = at(nt.checked_add(120).ok_or("NT overflow")?, 8)?;
        let export_rva = dword(&directories, 0);
        let export_size = dword(&directories, 4);
        if export_rva == 0 || export_size < 40 {
            return Err("target module has no exports".into());
        }
        let directory = at(export_rva, 40)?;
        let count = dword(&directory, 20);
        let names = dword(&directory, 24);
        if count > 65536 || names > 65536 {
            return Err("export table too large".into());
        }
        let functions = at(dword(&directory, 28), count as usize * 4)?;
        let index = if let Some(ordinal) = symbol.strip_prefix('#') {
            ordinal
                .parse::<u32>()
                .map_err(|_| "invalid forwarded ordinal")?
                .checked_sub(dword(&directory, 16))
                .ok_or("ordinal below export base")?
        } else {
            let name_table = at(dword(&directory, 32), names as usize * 4)?;
            let ordinals = at(dword(&directory, 36), names as usize * 2)?;
            let mut found = None;
            for index in 0..names as usize {
                if string(&mut at, dword(&name_table, index * 4))? == symbol {
                    found = Some(u32::from(word(&ordinals, index * 2)));
                    break;
                }
            }
            found.ok_or_else(|| format!("target export {module}!{symbol} missing"))?
        };
        if index >= count {
            return Err("export ordinal outside table".into());
        }
        let rva = dword(&functions, index as usize * 4);
        if rva == 0 {
            return Err("null export RVA".into());
        }
        let export_end = export_rva
            .checked_add(export_size)
            .ok_or("export directory overflow")?;
        if export_end > image.size {
            return Err("export directory outside image".into());
        }
        if (export_rva..export_end).contains(&rva) {
            let forwarder = string(&mut at, rva)?;
            let (next_module, next_symbol) = forwarder
                .rsplit_once('.')
                .ok_or("invalid export forwarder")?;
            module = if next_module.to_ascii_lowercase().ends_with(".dll") {
                next_module.into()
            } else {
                format!("{next_module}.dll")
            };
            symbol = next_symbol.into();
        } else {
            at(rva, 1)?;
            return image
                .base
                .checked_add(rva.into())
                .ok_or_else(|| "export address overflow".into());
        }
    }
    Err("export forwarder cycle or depth limit".into())
}
fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}
fn dword(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn string(
    read: &mut impl FnMut(u32, usize) -> Result<Vec<u8>, String>,
    rva: u32,
) -> Result<String, String> {
    let mut bytes = Vec::new();
    for i in 0..256 {
        let byte = read(rva.checked_add(i).ok_or("string RVA overflow")?, 1)?[0];
        if byte == 0 {
            return String::from_utf8(bytes).map_err(|_| "invalid export string".into());
        }
        bytes.push(byte);
    }
    Err("unterminated export string".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 1024];
        b[..2].copy_from_slice(b"MZ");
        b[60..64].copy_from_slice(&64u32.to_le_bytes());
        b[64..68].copy_from_slice(b"PE\0\0");
        b[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
        b[88..90].copy_from_slice(&0x10bu16.to_le_bytes());
        for (at, value) in [
            (184, 256u32),
            (188, 128),
            (272, 1),
            (276, 1),
            (280, 1),
            (284, 512),
            (288, 516),
            (292, 520),
            (512, 768),
            (516, 600),
        ] {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        b[600..605].copy_from_slice(b"call\0");
        b
    }
    #[test]
    fn resolves_target_base_forwarders_and_rejects_bad_rvas_and_cycles() {
        let images = [
            Image {
                name: "one.dll".into(),
                base: 4096,
                size: 1024,
            },
            Image {
                name: "two.dll".into(),
                base: 8192,
                size: 1024,
            },
        ];
        let mut first = fixture();
        let second = fixture();
        let check = |first: &[u8], symbol: &str| {
            resolve(&images, "one.dll", symbol, |address, out| {
                let (data, offset) = if address >= 8192 {
                    (&second[..], address - 8192)
                } else {
                    (first, address - 4096)
                };
                out.copy_from_slice(&data[offset as usize..offset as usize + out.len()]);
                Ok(())
            })
        };
        assert_eq!(check(&first, "call").unwrap(), 4096 + 768);
        first[512..516].copy_from_slice(&300u32.to_le_bytes());
        first[300..309].copy_from_slice(b"two.call\0");
        assert_eq!(check(&first, "call").unwrap(), 8192 + 768);
        first[300..309].copy_from_slice(b"one.call\0");
        assert!(check(&first, "call").unwrap_err().contains("cycle"));
        first[512..516].copy_from_slice(&2000u32.to_le_bytes());
        assert!(check(&first, "call").is_err());
        assert!(check(&first, "missing").is_err());
    }
}
