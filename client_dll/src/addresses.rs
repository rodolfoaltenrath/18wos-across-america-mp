//! Address notation shared by the DLL and the external inspection tool.
#[derive(Clone, Debug, PartialEq)]
pub enum Base {
    Absolute(u64),
    Module { name: String, offset: u64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Address {
    pub base: Base,
    /// Each step means: read a pointer at the current address, then add offset.
    pub chain: Vec<u64>,
}

pub fn hex(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let text = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u64::from_str_radix(text, 16).map_err(|_| "expected a hexadecimal address/offset".into())
}

impl std::str::FromStr for Address {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let mut parts = text.trim().split("->");
        let first = parts.next().ok_or("empty address")?;
        let base = if let Some((name, offset)) = first.split_once('+') {
            if name.trim().is_empty() || name.contains(['/', '\\', '\0']) {
                return Err("module must be a filename, e.g. prism3d.exe+0x1234".into());
            }
            Base::Module {
                name: name.trim().into(),
                offset: hex(offset)?,
            }
        } else {
            let value = hex(first)?;
            if value == 0 {
                return Err("address must be nonzero".into());
            }
            Base::Absolute(value)
        };
        let chain: Vec<_> = parts.map(hex).collect::<Result<_, _>>()?;
        if chain.len() > 16 {
            return Err("at most 16 pointer steps are supported".into());
        }
        Ok(Self { base, chain })
    }
}

impl Address {
    pub fn resolve(
        &self,
        max_address: u64,
        mut module: impl FnMut(&str) -> Result<u64, String>,
        mut pointer: impl FnMut(u64) -> Result<u64, String>,
    ) -> Result<u64, String> {
        let checked = |value: u64| -> Result<u64, String> {
            if value == 0 || value > max_address {
                Err("address is null or outside target address space".into())
            } else {
                Ok(value)
            }
        };
        let mut address = checked(match &self.base {
            Base::Absolute(value) => *value,
            Base::Module { name, offset } => module(name)?
                .checked_add(*offset)
                .ok_or("module offset overflow")?,
        })?;
        for offset in &self.chain {
            let base = checked(pointer(address)?)?;
            address = checked(base.checked_add(*offset).ok_or("pointer offset overflow")?)?;
        }
        Ok(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_modules_and_pointer_chains_and_checks_overflow() {
        let address: Address = "prism3d.exe+0x100->0x20->0x4".parse().unwrap();
        let result = address.resolve(
            u32::MAX.into(),
            |name| {
                assert_eq!(name, "prism3d.exe");
                Ok(0x400000)
            },
            |at| match at {
                0x400100 => Ok(0x1000),
                0x1020 => Ok(0x2000),
                _ => Err("unexpected pointer".into()),
            },
        );
        assert_eq!(result.unwrap(), 0x2004);
        assert!("0".parse::<Address>().is_err());
        assert!("module.exe+garbage".parse::<Address>().is_err());
        assert!(
            "ffffffff->1"
                .parse::<Address>()
                .unwrap()
                .resolve(u32::MAX.into(), |_| Ok(0), |_| Ok(u32::MAX.into()))
                .is_err()
        );
        assert!(
            "100->4"
                .parse::<Address>()
                .unwrap()
                .resolve(u32::MAX.into(), |_| Ok(0), |_| Ok(0))
                .is_err()
        );
    }
}
