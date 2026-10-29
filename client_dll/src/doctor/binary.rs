//! File-only PE/ELF metadata. No mapping, loader calls or dependency execution.
use std::collections::BTreeSet;

#[derive(Debug)]
pub(super) struct Binary {
    pub format: &'static str,
    pub machine: &'static str,
    pub imports: Vec<String>,
    pub exports: BTreeSet<String>,
}
fn word(bytes: &[u8], at: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at.checked_add(2).ok_or("offset overflow")?)
            .ok_or("truncated word")?
            .try_into()
            .unwrap(),
    ))
}
fn dword(bytes: &[u8], at: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at.checked_add(4).ok_or("offset overflow")?)
            .ok_or("truncated dword")?
            .try_into()
            .unwrap(),
    ))
}
struct Section {
    va: u32,
    raw: u32,
    bytes: u32,
}
struct Pe<'a> {
    bytes: &'a [u8],
    headers: u32,
    image_size: u32,
    sections: Vec<Section>,
}
impl Pe<'_> {
    fn at(&self, rva: u32, size: usize) -> Result<usize, &'static str> {
        let size32 = u32::try_from(size).map_err(|_| "RVA size overflow")?;
        let end = rva.checked_add(size32).ok_or("RVA overflow")?;
        if end > self.image_size {
            return Err("RVA outside image");
        }
        let raw = if end <= self.headers {
            Some(rva)
        } else {
            let mut found = None;
            for s in &self.sections {
                if rva >= s.va && end <= s.va.checked_add(s.bytes).ok_or("section overflow")? {
                    if found.is_some() {
                        return Err("ambiguous RVA");
                    }
                    found = Some(s.raw.checked_add(rva - s.va).ok_or("raw offset overflow")?);
                }
            }
            found
        }
        .ok_or("unmapped RVA")? as usize;
        self.bytes
            .get(raw..raw.checked_add(size).ok_or("raw size overflow")?)
            .ok_or("truncated RVA")?;
        Ok(raw)
    }
    fn string(&self, rva: u32) -> Result<String, &'static str> {
        let mut text = Vec::new();
        for offset in 0..512 {
            let at = self.at(rva.checked_add(offset).ok_or("string overflow")?, 1)?;
            let c = self.bytes[at];
            if c == 0 {
                return String::from_utf8(text).map_err(|_| "non-UTF8 symbol");
            }
            if !(0x20..=0x7e).contains(&c) {
                return Err("non-ASCII symbol");
            }
            text.push(c);
        }
        Err("unterminated symbol")
    }
}

pub(super) fn inspect(bytes: &[u8], want_exports: bool) -> Result<Binary, &'static str> {
    if bytes.starts_with(b"\x7fELF") {
        if bytes.len() < 64
            || bytes[5] != 1
            || bytes[6] != 1
            || dword(bytes, 20)? != 1
            || !matches!(bytes[4], 1 | 2)
            || !matches!(word(bytes, 16)?, 2 | 3)
        {
            return Err("unsupported/truncated ELF header");
        }
        if word(bytes, if bytes[4] == 2 { 52 } else { 40 })? != if bytes[4] == 2 { 64 } else { 52 }
        {
            return Err("invalid ELF header size");
        }
        let machine = match (bytes[4], word(bytes, 18)?) {
            (1, 3) => "x86",
            (2, 62) => "x86_64",
            (2, 183) => "aarch64",
            _ => "unknown",
        };
        return Ok(Binary {
            format: "elf",
            machine,
            imports: Vec::new(),
            exports: BTreeSet::new(),
        });
    }
    if bytes.get(..2) != Some(b"MZ") {
        return Err("not PE/ELF");
    }
    let nt = dword(bytes, 60)? as usize;
    if nt > 1024 * 1024 || bytes.get(nt..nt + 4) != Some(b"PE\0\0") {
        return Err("invalid PE signature");
    }
    let optional = nt + 24;
    let optional_size = usize::from(word(bytes, nt + 20)?);
    let (format, dir, count_at, machine) = match (word(bytes, optional)?, word(bytes, nt + 4)?) {
        (0x10b, 0x14c) => ("pe32", optional + 96, optional + 92, "x86"),
        (0x20b, 0x8664) => ("pe64", optional + 112, optional + 108, "x86_64"),
        (0x20b, 0xaa64) => ("pe64", optional + 112, optional + 108, "aarch64"),
        _ => return Err("unsupported PE architecture"),
    };
    if optional_size < dir + 16 - optional || dword(bytes, count_at)? < 2 {
        return Err("missing PE directories");
    }
    let table = optional
        .checked_add(optional_size)
        .ok_or("section table overflow")?;
    let count = usize::from(word(bytes, nt + 6)?);
    if !(1..=96).contains(&count) {
        return Err("invalid section count");
    }
    let mut pe = Pe {
        bytes,
        headers: dword(bytes, optional + 60)?,
        image_size: dword(bytes, optional + 56)?,
        sections: Vec::new(),
    };
    if pe.headers as usize > bytes.len() || pe.headers == 0 || pe.image_size < pe.headers {
        return Err("invalid image/header size");
    }
    for i in 0..count {
        let at = table + i * 40;
        let s = Section {
            va: dword(bytes, at + 12)?,
            raw: dword(bytes, at + 20)?,
            bytes: dword(bytes, at + 16)?,
        };
        if s.raw
            .checked_add(s.bytes)
            .is_none_or(|end| end as usize > bytes.len())
            || s.va
                .checked_add(s.bytes)
                .is_none_or(|end| end > pe.image_size)
        {
            return Err("invalid section range");
        }
        pe.sections.push(s);
    }
    let mut imports = BTreeSet::new();
    let import_rva = dword(bytes, dir + 8)?;
    let import_size = dword(bytes, dir + 12)?;
    if import_rva != 0 {
        if !(20..=1024 * 1024).contains(&import_size) {
            return Err("invalid import directory size");
        }
        let mut terminated = false;
        for index in 0..4096u32 {
            let offset = index * 20;
            if offset + 20 > import_size {
                break;
            }
            let at = pe.at(import_rva.checked_add(offset).ok_or("import overflow")?, 20)?;
            if bytes[at..at + 20].iter().all(|c| *c == 0) {
                terminated = true;
                break;
            }
            let name = pe.string(dword(bytes, at + 12)?)?;
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            {
                return Err("unsafe import basename");
            }
            imports.insert(name.to_ascii_lowercase());
        }
        if !terminated {
            return Err("unterminated import directory");
        }
    }
    let mut exports = BTreeSet::new();
    if want_exports {
        let export_rva = dword(bytes, dir)?;
        if export_rva != 0 {
            let at = pe.at(export_rva, 40)?;
            let functions = dword(bytes, at + 20)? as usize;
            let names = dword(bytes, at + 24)? as usize;
            if functions > 65536 || names > 65536 {
                return Err("export count exceeds limit");
            }
            let addresses = pe.at(dword(bytes, at + 28)?, functions * 4)?;
            let named = pe.at(dword(bytes, at + 32)?, names * 4)?;
            let ordinals = pe.at(dword(bytes, at + 36)?, names * 2)?;
            for i in 0..names {
                let ordinal = usize::from(word(bytes, ordinals + i * 2)?);
                if ordinal >= functions {
                    return Err("export ordinal outside table");
                }
                let rva = dword(bytes, addresses + ordinal * 4)?;
                if rva == 0 || rva >= pe.image_size {
                    return Err("invalid export target");
                }
                let name = pe.string(dword(bytes, named + i * 4)?)?;
                if !exports.insert(name) {
                    return Err("duplicate export name");
                }
            }
        }
    }
    Ok(Binary {
        format,
        machine,
        imports: imports.into_iter().collect(),
        exports,
    })
}

#[cfg(test)]
pub(super) fn fixture(exports: &[&str]) -> Vec<u8> {
    fn w(b: &mut [u8], at: usize, n: u16) {
        b[at..at + 2].copy_from_slice(&n.to_le_bytes());
    }
    fn d(b: &mut [u8], at: usize, n: u32) {
        b[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    let mut b = vec![0; 4096];
    b[..2].copy_from_slice(b"MZ");
    d(&mut b, 60, 128);
    b[128..132].copy_from_slice(b"PE\0\0");
    w(&mut b, 132, 0x14c);
    w(&mut b, 134, 1);
    w(&mut b, 148, 224);
    let optional = 152;
    w(&mut b, optional, 0x10b);
    d(&mut b, optional + 56, 8192);
    d(&mut b, optional + 60, 512);
    d(&mut b, optional + 92, 16);
    let section = optional + 224;
    d(&mut b, section + 12, 4096);
    d(&mut b, section + 16, 3584);
    d(&mut b, section + 20, 512);
    if !exports.is_empty() {
        d(&mut b, optional + 96, 4096);
        d(&mut b, optional + 100, 512);
        d(&mut b, 512 + 20, exports.len() as u32);
        d(&mut b, 512 + 24, exports.len() as u32);
        d(&mut b, 512 + 28, 4096 + 64);
        d(&mut b, 512 + 32, 4096 + 128);
        d(&mut b, 512 + 36, 4096 + 192);
        let mut name_at = 512 + 256;
        for (i, name) in exports.iter().enumerate() {
            d(&mut b, 512 + 64 + i * 4, 7000);
            d(&mut b, 512 + 128 + i * 4, (name_at - 512 + 4096) as u32);
            w(&mut b, 512 + 192 + i * 2, i as u16);
            b[name_at..name_at + name.len()].copy_from_slice(name.as_bytes());
            name_at += name.len() + 1;
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_headers_exports_and_elf_architecture_are_read_without_execution() {
        let value = inspect(&fixture(&["client_control", "client_stop"]), true).unwrap();
        assert_eq!((value.format, value.machine), ("pe32", "x86"));
        assert!(value.exports.contains("client_control"));
        let mut elf = vec![0; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[6] = 1;
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[52..54].copy_from_slice(&64u16.to_le_bytes());
        elf[16..18].copy_from_slice(&3u16.to_le_bytes());
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        assert_eq!(inspect(&elf, false).unwrap().machine, "x86_64");
    }
    #[test]
    fn truncation_bad_ranges_counts_and_architecture_are_refused() {
        let original = fixture(&["client_control"]);
        for end in [0, 2, 63, 128, 160, 380, 800] {
            assert!(inspect(&original[..end], true).is_err());
        }
        for (at, bytes) in [
            (60, u32::MAX.to_le_bytes().to_vec()),
            (134, 97u16.to_le_bytes().to_vec()),
            (152, 0x20bu16.to_le_bytes().to_vec()),
            (512 + 24, u32::MAX.to_le_bytes().to_vec()),
            (512 + 64, 0u32.to_le_bytes().to_vec()),
        ] {
            let mut changed = original.clone();
            changed[at..at + bytes.len()].copy_from_slice(&bytes);
            assert!(
                inspect(&changed, true).is_err(),
                "accepted changed offset {at}"
            );
        }
    }
    #[test]
    fn imports_use_bounded_mapped_basenames_and_require_termination() {
        let mut bytes = fixture(&[]);
        bytes[256..260].copy_from_slice(&4096u32.to_le_bytes());
        bytes[260..264].copy_from_slice(&60u32.to_le_bytes());
        bytes[524..528].copy_from_slice(&4224u32.to_le_bytes());
        bytes[544..548].copy_from_slice(&4256u32.to_le_bytes());
        bytes[640..653].copy_from_slice(b"KERNEL32.dll\0");
        bytes[672..682].copy_from_slice(b"OWN32.dll\0");
        assert_eq!(
            inspect(&bytes, false).unwrap().imports,
            ["kernel32.dll", "own32.dll"]
        );
        let mut unsafe_name = bytes.clone();
        unsafe_name[640..653].copy_from_slice(b"../badxx.dll\0");
        assert!(inspect(&unsafe_name, false).is_err());
        bytes[260..264].copy_from_slice(&20u32.to_le_bytes());
        assert!(inspect(&bytes, false).is_err());
    }
}
