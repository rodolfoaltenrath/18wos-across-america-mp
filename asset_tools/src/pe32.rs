//! Bounded, read-only PE32 analysis. Never maps or executes a commercial image.
use anyhow::{Context, Result, ensure};
use iced_x86::{Decoder, DecoderOptions, Formatter, IntelFormatter, OpKind, Register};
use serde::Serialize;
use std::{fs::File, io::Read, path::Path};
const MAX_FILE: u64 = 64 * 1024 * 1024;
#[derive(Clone, Debug, Serialize)]
pub struct Section {
    pub name: String,
    pub rva: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}
impl Section {
    pub fn executable(&self) -> bool {
        self.characteristics & 0x20000000 != 0
    }
}
pub struct Pe32 {
    bytes: Vec<u8>,
    pub base: u32,
    pub size: u32,
    headers: u32,
    pub sections: Vec<Section>,
    export: (u32, u32),
    import: (u32, u32),
}
#[derive(Debug, Serialize)]
pub struct Symbol {
    pub name: String,
    pub rva: u32,
}
#[derive(Debug, Serialize)]
pub struct Import {
    pub dll: String,
    pub name: String,
    pub iat_rva: u32,
}
#[derive(Debug, Serialize)]
pub struct RecordCandidate {
    pub name: String,
    pub rva: u32,
    pub words: [u32; 8],
    pub references: Vec<u32>,
}
#[derive(Debug, Serialize)]
pub struct Instruction {
    pub rva: u32,
    pub text: String,
}
fn u16at(b: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(offset..offset.checked_add(2).context("offset overflow")?)
            .context("truncated PE word")?
            .try_into()?,
    ))
}
fn u32at(b: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(offset..offset.checked_add(4).context("offset overflow")?)
            .context("truncated PE dword")?
            .try_into()?,
    ))
}
impl Pe32 {
    pub fn open(path: &Path) -> Result<Self> {
        let f = File::open(path)?;
        let size = f.metadata()?.len();
        ensure!(size <= MAX_FILE, "PE exceeds 64 MiB limit");
        let mut bytes = Vec::with_capacity(size as usize);
        f.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
        Self::parse(bytes)
    }
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            bytes.len() as u64 <= MAX_FILE && bytes.get(..2) == Some(b"MZ"),
            "not a bounded MZ file"
        );
        let nt = u32at(&bytes, 60)? as usize;
        let signature = bytes.get(nt..nt.checked_add(4).context("NT offset overflow")?);
        ensure!(
            signature == Some(b"PE\0\0") && u16at(&bytes, nt + 4)? == 0x14c,
            "requires PE32 x86"
        );
        let count = u16at(&bytes, nt + 6)? as usize;
        let optional = u16at(&bytes, nt + 20)? as usize;
        ensure!(
            (1..=96).contains(&count) && optional >= 112 && u16at(&bytes, nt + 24)? == 0x10b,
            "invalid PE32 section/optional header"
        );
        let base = u32at(&bytes, nt + 52)?;
        let size = u32at(&bytes, nt + 80)?;
        let headers = u32at(&bytes, nt + 84)?;
        ensure!(
            size > 0 && base.checked_add(size - 1).is_some() && headers as usize <= bytes.len(),
            "invalid image range"
        );
        let directories = u32at(&bytes, nt + 116)?;
        ensure!(directories >= 2, "PE32 missing data directories");
        let export = (u32at(&bytes, nt + 120)?, u32at(&bytes, nt + 124)?);
        let import = (u32at(&bytes, nt + 128)?, u32at(&bytes, nt + 132)?);
        let table = nt
            .checked_add(24 + optional)
            .context("section table overflow")?;
        let mut sections = Vec::new();
        for i in 0..count {
            let at = table + i * 40;
            let name = bytes.get(at..at + 8).context("truncated section")?;
            let name =
                String::from_utf8_lossy(&name[..name.iter().position(|b| *b == 0).unwrap_or(8)])
                    .into_owned();
            let section = Section {
                name,
                virtual_size: u32at(&bytes, at + 8)?,
                rva: u32at(&bytes, at + 12)?,
                raw_size: u32at(&bytes, at + 16)?,
                raw_offset: u32at(&bytes, at + 20)?,
                characteristics: u32at(&bytes, at + 36)?,
            };
            ensure!(
                u64::from(section.raw_offset) + u64::from(section.raw_size) <= bytes.len() as u64,
                "section raw bytes outside file"
            );
            ensure!(
                u64::from(section.rva) + u64::from(section.virtual_size.max(section.raw_size))
                    <= u64::from(size),
                "section outside image"
            );
            sections.push(section);
        }
        let pe = Self {
            bytes,
            base,
            size,
            headers,
            sections,
            export,
            import,
        };
        for (at, len) in [export, import] {
            ensure!(
                at == 0 || u64::from(at) + u64::from(len) <= u64::from(size),
                "directory outside image"
            );
        }
        Ok(pe)
    }
    pub fn bytes(&self, rva: u32, len: usize) -> Result<&[u8]> {
        ensure!(
            u64::from(rva) + len as u64 <= u64::from(self.size),
            "RVA exceeds image"
        );
        if u64::from(rva) + len as u64 <= u64::from(self.headers) {
            return self
                .bytes
                .get(rva as usize..rva as usize + len)
                .context("header range outside file");
        }
        let mut result = None;
        for s in &self.sections {
            if rva >= s.rva && u64::from(rva - s.rva) + len as u64 <= u64::from(s.raw_size) {
                ensure!(result.is_none(), "ambiguous overlapping sections");
                let offset = (s.raw_offset as usize)
                    .checked_add((rva - s.rva) as usize)
                    .context("file offset overflow")?;
                result = self.bytes.get(offset..offset + len);
            }
        }
        result.context("RVA not backed by file bytes (possibly BSS)")
    }
    pub fn word(&self, rva: u32) -> Result<u32> {
        u32at(self.bytes(rva, 4)?, 0)
    }
    pub fn rva(&self, va: u32) -> Result<u32> {
        let rva = va.checked_sub(self.base).context("VA below image")?;
        ensure!(rva < self.size, "VA outside image");
        Ok(rva)
    }
    pub fn string(&self, rva: u32) -> Result<String> {
        let mut b = Vec::new();
        for offset in 0..256 {
            let byte = self.bytes(rva.checked_add(offset).context("string RVA overflow")?, 1)?[0];
            if byte == 0 {
                return String::from_utf8(b).context("invalid UTF-8 string");
            }
            b.push(byte);
        }
        anyhow::bail!("unterminated string")
    }
    pub fn executable_va(&self, va: u32) -> bool {
        self.rva(va).is_ok_and(|rva| {
            self.sections
                .iter()
                .any(|s| s.executable() && rva >= s.rva && rva - s.rva < s.virtual_size)
        })
    }
    pub fn exports(&self) -> Result<Vec<Symbol>> {
        let (rva, size) = self.export;
        if rva == 0 {
            return Ok(Vec::new());
        }
        ensure!(size >= 40, "truncated export directory");
        let dir = self.bytes(rva, 40)?;
        let functions = u32at(dir, 20)?;
        let names = u32at(dir, 24)?;
        ensure!(functions <= 65536 && names <= 65536, "too many exports");
        let addresses = self.bytes(u32at(dir, 28)?, functions as usize * 4)?;
        let name_table = self.bytes(u32at(dir, 32)?, names as usize * 4)?;
        let ordinals = self.bytes(u32at(dir, 36)?, names as usize * 2)?;
        let mut symbols = Vec::new();
        for index in 0..names as usize {
            let ordinal = u16at(ordinals, index * 2)? as usize;
            ensure!(ordinal < functions as usize, "bad export ordinal");
            symbols.push(Symbol {
                name: self.string(u32at(name_table, index * 4)?)?,
                rva: u32at(addresses, ordinal * 4)?,
            });
        }
        Ok(symbols)
    }
    pub fn imports(&self) -> Result<Vec<Import>> {
        let (rva, size) = self.import;
        if rva == 0 {
            return Ok(Vec::new());
        }
        ensure!(size <= 1024 * 1024, "import directory too large");
        let mut imports = Vec::new();
        for index in 0..(size / 20).min(4096) {
            let dir = self.bytes(rva + index * 20, 20)?;
            let thunk = u32at(dir, 0)?;
            let name = u32at(dir, 12)?;
            let iat = u32at(dir, 16)?;
            if name == 0 && thunk == 0 && iat == 0 {
                return Ok(imports);
            }
            let dll = self.string(name)?;
            let lookup = if thunk == 0 { iat } else { thunk };
            for i in 0..65536u32 {
                let value = self.word(lookup.checked_add(i * 4).context("thunk overflow")?)?;
                if value == 0 {
                    break;
                }
                ensure!(imports.len() < 65536, "import count limit");
                imports.push(Import {
                    dll: dll.clone(),
                    name: if value & 0x80000000 != 0 {
                        format!("#{}", value & 0xffff)
                    } else {
                        self.string(value.checked_add(2).context("import name overflow")?)?
                    },
                    iat_rva: iat.checked_add(i * 4).context("IAT overflow")?,
                });
            }
        }
        anyhow::bail!("unterminated import directory")
    }
    fn instructions(&self) -> impl Iterator<Item = iced_x86::Instruction> + '_ {
        self.sections
            .iter()
            .filter(|s| s.executable())
            .flat_map(|s| {
                let bytes =
                    &self.bytes[s.raw_offset as usize..(s.raw_offset + s.raw_size) as usize];
                Decoder::with_ip(
                    32,
                    bytes,
                    u64::from(self.base) + u64::from(s.rva),
                    DecoderOptions::NONE,
                )
                .into_iter()
            })
    }
    pub fn disassemble(&self, rva: u32, length: usize) -> Result<Vec<Instruction>> {
        ensure!(length <= 65536, "disassembly limited to 64 KiB");
        let bytes = self.bytes(rva, length)?;
        let mut formatter = IntelFormatter::new();
        Ok(Decoder::with_ip(
            32,
            bytes,
            u64::from(self.base) + u64::from(rva),
            DecoderOptions::NONE,
        )
        .into_iter()
        .map(|i| {
            let mut text = String::new();
            formatter.format(&i, &mut text);
            Instruction {
                rva: (i.ip() - u64::from(self.base)) as u32,
                text,
            }
        })
        .collect())
    }
    pub fn references(&self, rva: u32) -> Result<Vec<u32>> {
        let va = self
            .base
            .checked_add(rva)
            .context("reference address overflow")?;
        Ok(self
            .instructions()
            .filter(|i| {
                (0..i.op_count()).any(|n| match i.op_kind(n) {
                    OpKind::Immediate32 => i.immediate32() == va,
                    OpKind::NearBranch32 => i.near_branch32() == va,
                    _ => false,
                }) || (i.memory_base() == Register::None
                    && i.memory_index() == Register::None
                    && i.memory_displacement32() == va)
            })
            .map(|i| (i.ip() - u64::from(self.base)) as u32)
            .collect())
    }
    /// Candidate accesses to a member offset through a register. This does not
    /// prove the register points to a particular type or that the code is reached.
    pub fn field_references(&self, offset: u32) -> Vec<Instruction> {
        let mut formatter = IntelFormatter::new();
        self.instructions()
            .filter(|i| {
                i.memory_base() != Register::None
                    && i.memory_displacement32() == offset
                    && (0..i.op_count()).any(|n| i.op_kind(n) == OpKind::Memory)
            })
            .map(|i| {
                let mut text = String::new();
                formatter.format(&i, &mut text);
                Instruction {
                    rva: (i.ip() - u64::from(self.base)) as u32,
                    text,
                }
            })
            .collect()
    }
    /// Structural candidates, not permission to call the constructors. Layout
    /// follows inspected entity_create/get_struct and must be checked live too.
    pub fn entity_candidates(&self) -> Result<Vec<RecordCandidate>> {
        let mut records = Vec::new();
        for s in self.sections.iter().filter(|s| !s.executable()) {
            for offset in (0..s.raw_size.saturating_sub(31)).step_by(4) {
                let rva = s.rva + offset;
                let bytes = self.bytes(rva, 32)?;
                let words: [u32; 8] = std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                let branch_valid = |size: u32, ctor: u32, dtor: u32| {
                    size > 0
                        && size <= 16 * 1024 * 1024
                        && self.executable_va(ctor)
                        && self.executable_va(dtor)
                };
                let normal = branch_valid(words[2], words[3], words[4]);
                let editor = branch_valid(words[5], words[6], words[7]);
                if !(normal || editor)
                    || (!normal && words[2..5] != [0; 3])
                    || (!editor && words[5..8] != [0; 3])
                {
                    continue;
                }
                let Ok(name) = self.rva(words[0]).and_then(|r| self.string(r)) else {
                    continue;
                };
                if name.is_empty()
                    || name.len() > 96
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.".contains(&b))
                {
                    continue;
                }
                records.push(RecordCandidate {
                    name,
                    rva,
                    words,
                    references: Vec::new(),
                });
            }
        }
        for record in &mut records {
            record.references = self.references(record.rva)?;
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 0x600];
        b[..2].copy_from_slice(b"MZ");
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        for (at, val) in [(0x84, 0x14cu16), (0x86, 2), (0x94, 224), (0x98, 0x10b)] {
            b[at..at + 2].copy_from_slice(&val.to_le_bytes());
        }
        for (at, val) in [
            (60, 0x80u32),
            (0xb4, 0x10000000),
            (0xd0, 0x3000),
            (0xd4, 0x200),
            (0xf4, 16),
            (0x180, 0x100),
            (0x184, 0x1000),
            (0x188, 0x200),
            (0x18c, 0x200),
            (0x19c, 0x60000020),
            (0x1a8, 0x200),
            (0x1ac, 0x2000),
            (0x1b0, 0x200),
            (0x1b4, 0x400),
            (0x1c4, 0x40000040),
        ] {
            b[at..at + 4].copy_from_slice(&val.to_le_bytes());
        }
        b[0x178..0x17d].copy_from_slice(b".text");
        b[0x1a0..0x1a6].copy_from_slice(b".rdata");
        b[0x200..0x206].copy_from_slice(&[0x68, 0x20, 0x20, 0, 0x10, 0xc3]);
        b[0x400..0x406].copy_from_slice(b"truck\0");
        for (i, value) in [
            0x10002000u32,
            7,
            64,
            0x10001010,
            0x10001020,
            64,
            0x10001030,
            0x10001040,
        ]
        .into_iter()
        .enumerate()
        {
            b[0x420 + i * 4..0x424 + i * 4].copy_from_slice(&value.to_le_bytes());
        }
        b
    }
    #[test]
    fn maps_rvas_disassembles_and_finds_own_entity_record() {
        let pe = Pe32::parse(fixture()).unwrap();
        assert_eq!(pe.string(0x2000).unwrap(), "truck");
        assert_eq!(pe.references(0x2020).unwrap(), [0x1000]);
        assert!(pe.disassemble(0x1000, 6).unwrap()[0].text.contains("push"));
        let candidates = pe.entity_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "truck");
        assert!(pe.bytes(0x3000, 1).is_err());
        assert!(pe.bytes(u32::MAX, 4).is_err());
    }
    #[test]
    fn member_candidates_exclude_immediates_and_absolute_addresses() {
        let mut bytes = fixture();
        // LEA eax,[ecx+0x12c8], PUSH 0x12c8, MOV eax,[0x12c8].
        let code = [
            0x8d, 0x81, 0xc8, 0x12, 0, 0, 0x68, 0xc8, 0x12, 0, 0, 0xa1, 0xc8, 0x12, 0, 0,
        ];
        bytes[0x210..0x210 + code.len()].copy_from_slice(&code);
        let pe = Pe32::parse(bytes).unwrap();
        let hits = pe.field_references(0x12c8);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rva, 0x1010);
        assert!(hits[0].text.contains("ecx+12C8h"));
    }
    #[test]
    fn rejects_truncated_architecture_and_ambiguous_sections() {
        assert!(Pe32::parse(vec![0; 64]).is_err());
        let mut b = fixture();
        b[0x84] = 0x64;
        assert!(Pe32::parse(b).is_err());
        let mut b = fixture();
        b[0x1ac..0x1b0].copy_from_slice(&0x1000u32.to_le_bytes());
        let pe = Pe32::parse(b).unwrap();
        assert!(pe.bytes(0x1000, 1).is_err());
        let mut b = fixture();
        b.truncate(0x500);
        assert!(Pe32::parse(b).is_err());
    }
    #[test]
    fn parses_own_exports_imports_and_one_branch_entity_record() {
        let mut b = fixture();
        for (at, value) in [
            (0xf8, 0x2100u32),
            (0xfc, 0x80),
            (0x100, 0x2080),
            (0x104, 40),
            (0x480, 0x20b0),
            (0x48c, 0x20e0),
            (0x490, 0x20c0),
            (0x4b0, 0x21c0),
            (0x514, 1),
            (0x518, 1),
            (0x51c, 0x2140),
            (0x520, 0x2144),
            (0x524, 0x2148),
            (0x540, 0x1000),
            (0x544, 0x2150),
        ] {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        b[0x4e0..0x4e8].copy_from_slice(b"own.dll\0");
        b[0x5c2..0x5c8].copy_from_slice(b"entry\0");
        b[0x550..0x555].copy_from_slice(b"call\0");
        b[0x428..0x434].fill(0); // false branch only, like actual actors.* records
        let pe = Pe32::parse(b).unwrap();
        let imports = pe.imports().unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].name, "entry");
        assert_eq!(imports[0].iat_rva, 0x20c0);
        let exports = pe.exports().unwrap();
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].name, "call");
        assert_eq!(exports[0].rva, 0x1000);
        assert_eq!(pe.entity_candidates().unwrap().len(), 1);
    }
}
