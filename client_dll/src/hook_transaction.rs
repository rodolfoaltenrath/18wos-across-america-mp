//! Recoverable descriptor patches. A failed operation may already have written
//! the callback or changed page protection, so its journal precedes the write.
//! No executable addresses or OS calls live in this module.

pub(crate) trait Memory {
    fn load(&mut self, address: usize) -> Result<usize, String>;
    fn writable(&mut self, address: usize) -> Result<u32, String>;
    fn exchange(&mut self, address: usize, expected: usize, desired: usize) -> Result<(), String>;
    fn protect(&mut self, address: usize, protection: u32) -> Result<(), String>;
}

pub(crate) struct Patch {
    address: usize,
    original: usize,
    replacement: usize,
    owned: bool,
    protection: Option<u32>,
}
impl Patch {
    pub(crate) fn new(address: usize, original: usize, replacement: usize) -> Self {
        Self {
            address,
            original,
            replacement,
            owned: false,
            protection: None,
        }
    }

    fn restore_protection(&mut self, memory: &mut impl Memory) -> Result<(), String> {
        if let Some(protection) = self.protection {
            memory.protect(self.address, protection)?;
            self.protection = None;
        }
        Ok(())
    }

    fn exchange(&mut self, memory: &mut impl Memory, installing: bool) -> Result<(), String> {
        // Never replace the original protection with the temporary RW value
        // when retrying after a failed restoration.
        if self.protection.is_none() {
            self.protection = Some(memory.writable(self.address)?);
        }
        let (expected, desired) = if installing {
            (self.original, self.replacement)
        } else {
            (self.replacement, self.original)
        };
        let exchange = memory.exchange(self.address, expected, desired);
        if exchange.is_ok() {
            self.owned = installing;
        }
        let protection = self.restore_protection(memory);
        match (exchange, protection) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(exchange), Err(protection)) => Err(format!("{exchange}; {protection}")),
        }
    }

    pub(crate) fn install(&mut self, memory: &mut impl Memory) -> Result<(), String> {
        self.exchange(memory, true)
    }

    pub(crate) fn restore(&mut self, memory: &mut impl Memory) -> Result<(), String> {
        if self.owned {
            match memory.load(self.address) {
                Ok(current) if current == self.replacement => return self.exchange(memory, false),
                Ok(current) if current == self.original => self.owned = false,
                result => {
                    // A foreign callback is never overwritten. Protection debt
                    // is still ours even if ownership of the word was lost.
                    let protection = self.restore_protection(memory);
                    return Err(format!(
                        "descriptor ownership unresolved: {result:?}; protection={protection:?}"
                    ));
                }
            }
        }
        self.restore_protection(memory)
    }
}

/// Attempt every rollback, including earlier patches when a later one failed.
/// Keep this journal on error: callers must allow removal to be retried.
pub(crate) fn restore_all(patches: &mut [Patch], memory: &mut impl Memory) -> Result<(), String> {
    let mut errors = Vec::new();
    for patch in patches.iter_mut().rev() {
        if let Err(error) = patch.restore(memory) {
            errors.push(format!("0x{:x}: {error}", patch.address));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ORIGINAL: usize = 10;
    const HOOK: usize = 20;
    const FOREIGN: usize = 30;
    const READ_ONLY: u32 = 2;
    const READ_WRITE: u32 = 4;

    struct Host {
        words: [usize; 6],
        protections: [u32; 6],
        unlock_failure: Option<usize>,
        protect_failure: Option<usize>,
        foreign_on_exchange: Option<usize>,
        read_failure: Option<usize>,
    }
    impl Default for Host {
        fn default() -> Self {
            Self {
                words: [ORIGINAL; 6],
                protections: [READ_ONLY; 6],
                unlock_failure: None,
                protect_failure: None,
                foreign_on_exchange: None,
                read_failure: None,
            }
        }
    }
    impl Memory for Host {
        fn load(&mut self, address: usize) -> Result<usize, String> {
            if self.read_failure == Some(address) {
                return Err("unreadable".into());
            }
            Ok(self.words[address])
        }
        fn writable(&mut self, address: usize) -> Result<u32, String> {
            if self.unlock_failure == Some(address) {
                return Err("unlock failed".into());
            }
            Ok(std::mem::replace(
                &mut self.protections[address],
                READ_WRITE,
            ))
        }
        fn exchange(
            &mut self,
            address: usize,
            expected: usize,
            desired: usize,
        ) -> Result<(), String> {
            assert_eq!(self.protections[address], READ_WRITE);
            if self.foreign_on_exchange == Some(address) {
                self.words[address] = FOREIGN;
            }
            if self.words[address] != expected {
                return Err("owner changed".into());
            }
            self.words[address] = desired;
            Ok(())
        }
        fn protect(&mut self, address: usize, protection: u32) -> Result<(), String> {
            if self.protect_failure == Some(address) {
                return Err("protect failed".into());
            }
            self.protections[address] = protection;
            Ok(())
        }
    }
    fn patches() -> Vec<Patch> {
        (0..6)
            .map(|address| Patch::new(address, ORIGINAL, HOOK))
            .collect()
    }

    #[test]
    fn each_partial_install_restores_all_six_words_and_their_protections() {
        for index in 0..6 {
            for protection_failure in [false, true] {
                let mut host = Host::default();
                if protection_failure {
                    host.protect_failure = Some(index);
                } else {
                    host.unlock_failure = Some(index);
                }
                let mut patches = patches();
                for (i, patch) in patches.iter_mut().enumerate() {
                    let result = patch.install(&mut host);
                    if i == index {
                        assert!(result.is_err());
                        break;
                    }
                    result.unwrap();
                }
                host.protect_failure = None;
                host.unlock_failure = None;
                restore_all(&mut patches, &mut host).unwrap();
                assert_eq!(host.words, [ORIGINAL; 6]);
                assert_eq!(host.protections, [READ_ONLY; 6]);
            }
        }
    }

    #[test]
    fn failed_rollback_keeps_protection_debt_and_allows_another_removal() {
        let mut host = Host::default();
        let mut patches = patches();
        for patch in &mut patches {
            patch.install(&mut host).unwrap();
        }
        host.protect_failure = Some(4);
        assert!(restore_all(&mut patches, &mut host).is_err());
        assert_eq!(host.words, [ORIGINAL; 6]);
        assert_eq!(host.protections[4], READ_WRITE);
        host.protect_failure = None;
        restore_all(&mut patches, &mut host).unwrap();
        assert_eq!(host.protections, [READ_ONLY; 6]);
        restore_all(&mut patches, &mut host).unwrap();
    }

    #[test]
    fn compare_exchange_conflict_never_overwrites_foreign_word_even_when_protect_fails() {
        let mut host = Host {
            foreign_on_exchange: Some(2),
            protect_failure: Some(2),
            ..Host::default()
        };
        let mut patches = patches();
        for patch in &mut patches[..2] {
            patch.install(&mut host).unwrap();
        }
        assert!(patches[2].install(&mut host).is_err());
        host.protect_failure = None;
        restore_all(&mut patches, &mut host).unwrap();
        assert_eq!(
            host.words,
            [ORIGINAL, ORIGINAL, FOREIGN, ORIGINAL, ORIGINAL, ORIGINAL]
        );
        assert_eq!(host.protections, [READ_ONLY; 6]);
    }

    #[test]
    fn lost_ownership_or_unreadable_word_does_not_skip_other_rollbacks() {
        for unreadable in [false, true] {
            let mut host = Host::default();
            let mut patches = patches();
            for patch in &mut patches {
                patch.install(&mut host).unwrap();
            }
            if unreadable {
                host.read_failure = Some(3);
            } else {
                host.words[3] = FOREIGN;
            }
            assert!(restore_all(&mut patches, &mut host).is_err());
            for index in [0, 1, 2, 4, 5] {
                assert_eq!(host.words[index], ORIGINAL);
            }
            assert_eq!(host.words[3], if unreadable { HOOK } else { FOREIGN });
            host.read_failure = None;
            host.words[3] = HOOK;
            restore_all(&mut patches, &mut host).unwrap();
            assert_eq!(host.words, [ORIGINAL; 6]);
        }
    }
}
