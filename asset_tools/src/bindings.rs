//! Conservative GetProcAddress -> absolute EAX store candidates in one straight
//! line resolver routine. This is neither control-flow recovery nor an ABI proof.
use crate::pe32::Pe32;
use anyhow::{Context, Result, ensure};
use iced_x86::{Decoder, DecoderOptions, FlowControl, Mnemonic, OpKind, Register};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DynamicBinding {
    pub symbol: String,
    pub name_rva: u32,
    pub lookup_rva: u32,
    pub store_rva: u32,
    pub slot_rva: u32,
    pub resolver_iat_rva: u32,
}
fn absolute(i: &iced_x86::Instruction) -> bool {
    i.memory_base() == Register::None
        && i.memory_index() == Register::None
        && i.memory_displ_size() == 4
}

impl Pe32 {
    /// Decode from an explicit instruction boundary, stop at the first return or
    /// unsupported instruction after finding the resolver. Supports only PUSH
    /// name/PUSH module/CALL resolver/MOV [slot],EAX, including delayed stores.
    pub fn dynamic_bindings(&self, rva: u32, length: usize) -> Result<Vec<DynamicBinding>> {
        ensure!(
            (1..=16384).contains(&length),
            "resolver window must be 1..=16384 bytes"
        );
        let start = self
            .base
            .checked_add(rva)
            .context("resolver address overflow")?;
        ensure!(
            self.executable_va(start),
            "resolver must start in executable image bytes"
        );
        let imports = self.imports()?;
        let resolvers: Vec<_> = imports
            .iter()
            .filter(|i| i.dll.eq_ignore_ascii_case("kernel32.dll") && i.name == "GetProcAddress")
            .collect();
        ensure!(
            resolvers.len() == 1,
            "requires one named kernel32 GetProcAddress import"
        );
        let iat = resolvers[0].iat_rva;
        let iat_va = self.base.checked_add(iat).context("IAT address overflow")?;
        let mut resolver = None;
        let mut arguments = Vec::new();
        let mut pending = None;
        let mut rows = Vec::new();
        for i in Decoder::with_ip(
            32,
            self.bytes(rva, length)?,
            u64::from(self.base) + u64::from(rva),
            DecoderOptions::NONE,
        ) {
            let at = (i.ip() - u64::from(self.base)) as u32;
            if i.is_invalid() {
                break;
            }
            if !matches!(
                i.flow_control(),
                FlowControl::Next | FlowControl::IndirectCall
            ) {
                break;
            }
            if i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op1_kind() == OpKind::Memory
                && absolute(&i)
                && i.memory_displacement32() == iat_va
            {
                // Do not infer aliases, subregisters, or a resolver loaded by LEA.
                ensure!(
                    matches!(
                        i.op0_register(),
                        Register::EAX
                            | Register::EBX
                            | Register::ECX
                            | Register::EDX
                            | Register::ESI
                            | Register::EDI
                            | Register::EBP
                    ),
                    "resolver destination is not a supported x86 register"
                );
                resolver = Some(i.op0_register());
                arguments.clear();
                pending = None;
                continue;
            }
            if resolver.is_none() && pending.is_none() {
                continue;
            }
            match i.mnemonic() {
                Mnemonic::Push => {
                    if arguments.len() >= 2 {
                        break;
                    }
                    arguments.push((i.op0_kind() == OpKind::Immediate32).then(|| i.immediate32()));
                }
                Mnemonic::Call
                    if i.op0_kind() == OpKind::Register && Some(i.op0_register()) == resolver =>
                {
                    pending = None;
                    if arguments.len() == 2
                        && let Some(name) = arguments[0]
                        && let Ok(name_rva) = self.rva(name)
                        && let Ok(symbol) = self.string(name_rva)
                        && !symbol.is_empty()
                        && symbol.bytes().all(|b| b.is_ascii_graphic())
                    {
                        pending = Some((symbol, name_rva, at));
                    }
                    arguments.clear();
                    // GetProcAddress is stdcall; volatile registers are destroyed.
                    if matches!(
                        resolver,
                        Some(Register::EAX | Register::ECX | Register::EDX)
                    ) {
                        resolver = None;
                    }
                }
                Mnemonic::Mov
                    if i.op0_kind() == OpKind::Memory
                        && absolute(&i)
                        && i.op1_kind() == OpKind::Register
                        && i.op1_register() == Register::EAX =>
                {
                    if let Some((symbol, name_rva, lookup_rva)) = pending.take() {
                        let slot_rva = self.rva(i.memory_displacement32())?;
                        let writable = self.sections.iter().any(|s| {
                            s.characteristics & 0x80000000 != 0
                                && slot_rva >= s.rva
                                && u64::from(slot_rva - s.rva) + 4
                                    <= u64::from(s.virtual_size.max(s.raw_size))
                        });
                        ensure!(
                            writable && slot_rva.is_multiple_of(4),
                            "binding destination is not an aligned writable image slot"
                        );
                        rows.push(DynamicBinding {
                            symbol,
                            name_rva,
                            lookup_rva,
                            store_rva: at,
                            slot_rva,
                            resolver_iat_rva: iat,
                        });
                    }
                }
                Mnemonic::Pop if i.op0_kind() == OpKind::Register => {
                    // Saved nonvolatile registers may be restored before the
                    // last EAX store. They cannot serve as a resolver afterward.
                    if Some(i.op0_register()) == resolver {
                        resolver = None;
                    }
                    if i.op0_register() == Register::EAX {
                        pending = None;
                    }
                }
                Mnemonic::Nop => {}
                _ => break,
            }
        }
        Ok(rows)
    }
}
