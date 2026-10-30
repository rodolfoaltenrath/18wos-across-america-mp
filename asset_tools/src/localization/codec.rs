//! 3nK v1 XOR envelope, independently checked against p3shared's reader.
//! Interoperability background: https://github.com/Wilps93/scs_tools/blob/main/FORMAT.md#91-3nk
use anyhow::{Result, ensure};
pub fn decode(input: &[u8]) -> Result<Vec<u8>> {
    ensure!(input.len() <= 1024 * 1024, "text envelope limit");
    if !input.starts_with(b"3nK") {
        return Ok(input.to_vec());
    }
    ensure!(
        input.len() >= 6 && input[3..5] == [1, 0],
        "unsupported/truncated 3nK envelope"
    );
    let mut state = input[5];
    Ok(input[6..]
        .iter()
        .map(|&b| {
            let mask = ((state.wrapping_shl(2) ^ !state).wrapping_shl(3)) ^ state;
            state = state.wrapping_add(1);
            b ^ mask
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_byte_all_seeds_and_wrap_are_checked_without_game_bytes() {
        assert_eq!(decode(b"3nK\x01\x00\x58\x43").unwrap(), b"#");
        for seed in 0..=255u8 {
            let plain = b"def translate { own_key: \"Own fixture\" }\n".repeat(20);
            let mut encoded = vec![b'3', b'n', b'K', 1, 0, seed];
            for (i, &b) in plain.iter().enumerate() {
                let n = (usize::from(seed) + i) & 255;
                let mask = ((((n << 2) ^ (!n)) << 3) ^ n) & 255;
                encoded.push(b ^ mask as u8);
            }
            assert_eq!(decode(&encoded).unwrap(), plain);
        }
        assert!(decode(b"3nK\x02\x00\x00").is_err());
        assert!(decode(b"3nK").is_err());
    }
}
