//! Font slot/metrics inspection. Slot presence does not verify glyph appearance.
//! Descriptors are single-byte text: comments may hold legacy code-page bytes.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Glyph {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub left: i32,
    pub top: i32,
    pub advance: i32,
}
#[derive(Serialize)]
pub struct Font {
    pub vert_span: i32,
    pub line_spacing: i32,
    pub images: Vec<String>,
    pub glyphs: BTreeMap<u8, Glyph>,
    pub kerns: usize,
}
fn slot(value: &str) -> Result<u8> {
    let digits = value.trim().strip_prefix('$').context("kern slot")?;
    Ok(u8::try_from(digits.parse::<u32>().context("kern slot")?)?)
}
pub fn parse(bytes: &[u8]) -> Result<Font> {
    ensure!(bytes.len() <= 1024 * 1024, "font descriptor limit");
    let mut result = Font {
        vert_span: 0,
        line_spacing: 0,
        images: vec![],
        glyphs: BTreeMap::new(),
        kerns: 0,
    };
    for raw in bytes.split(|b| *b == b'\n') {
        let raw = raw.split(|b| *b == b'#').next().unwrap_or_default();
        let line = std::str::from_utf8(raw)
            .context("non-ASCII outside a font comment")?
            .trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('$') {
            let nums: Vec<i32> = line
                .trim_start_matches('$')
                .split(',')
                .map(|v| v.trim().parse().context("font metric"))
                .collect::<Result<_>>()?;
            ensure!(nums.len() == 8, "glyph field count");
            let key = u8::try_from(nums[0])?;
            ensure!(
                nums[1..5].iter().all(|v| (0..=4096).contains(v))
                    && nums[5..].iter().all(|v| (-4096..=4096).contains(v)),
                "glyph metric bounds"
            );
            ensure!(
                result
                    .glyphs
                    .insert(
                        key,
                        Glyph {
                            x: nums[1],
                            y: nums[2],
                            w: nums[3],
                            h: nums[4],
                            left: nums[5],
                            top: nums[6],
                            advance: nums[7]
                        }
                    )
                    .is_none(),
                "duplicate glyph slot"
            );
        } else if let Some(v) = line.strip_prefix("kern:") {
            let parts: Vec<&str> = v.split(',').collect();
            ensure!(parts.len() == 3, "kern field count");
            slot(parts[0])?;
            slot(parts[1])?;
            let amount: i32 = parts[2].trim().parse().context("kern amount")?;
            ensure!((-64..=64).contains(&amount), "kern bounds");
            result.kerns += 1;
            ensure!(result.kerns <= 65536, "kern count");
        } else if let Some(v) = line.strip_prefix("vert_span:") {
            result.vert_span = v.trim().parse()?;
        } else if let Some(v) = line.strip_prefix("line_spacing:") {
            result.line_spacing = v.trim().parse()?;
        } else if let Some(v) = line.strip_prefix("image:") {
            let v = v.trim();
            ensure!(
                v.len() <= 240 && v.starts_with("/fonts/") && v.ends_with(".shd"),
                "font shader path"
            );
            crate::safe_relative(v.trim_start_matches('/'))?;
            ensure!(result.images.len() < 8, "font shader count");
            result.images.push(v.into());
        } else {
            anyhow::bail!("unsupported font directive");
        }
    }
    ensure!(
        (1..=512).contains(&result.vert_span)
            && (0..=128).contains(&result.line_spacing)
            && !result.images.is_empty()
            && !result.glyphs.is_empty(),
        "font metadata"
    );
    Ok(result)
}

/// Appends glyph lines to the original descriptor bytes; existing slots and
/// kerning stay byte-identical. Only valid for slots absent from `original`.
pub fn append(original: &[u8], added: &BTreeMap<u8, Glyph>) -> Result<Vec<u8>> {
    let font = parse(original)?;
    ensure!(
        added.keys().all(|k| !font.glyphs.contains_key(k)),
        "slot already present"
    );
    let newline: &[u8] = if original.windows(2).any(|w| w == b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    };
    let mut out = original.to_vec();
    if !out.ends_with(b"\n") {
        out.extend_from_slice(newline);
    }
    out.extend_from_slice(b"# PT-BR glyphs synthesized locally by l10n_tools");
    out.extend_from_slice(newline);
    for (k, g) in added {
        out.extend_from_slice(
            format!(
                "${:03}, {:3}, {:3}, {:2}, {:2}, {:2}, {:2}, {:2}",
                k, g.x, g.y, g.w, g.h, g.left, g.top, g.advance
            )
            .as_bytes(),
        );
        out.extend_from_slice(newline);
    }
    parse(&out)?;
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    const OWN: &[u8]=b"# own \xff comment\r\nvert_span:12\r\nline_spacing:2\r\nimage:/fonts/own.shd\r\n$065,1,2,3,4,0,1,5 # \xff\r\nkern: $065, $065, -1  # \xff\r\n";
    #[test]
    fn byte_comments_are_not_decoded_as_text_and_duplicate_slots_fail() {
        let f = parse(OWN).unwrap();
        assert_eq!(f.glyphs[&65].advance, 5);
        assert_eq!(f.kerns, 1);
        assert!(parse(&[OWN, b"$065,1,2,3,4,0,1,5"].concat()).is_err());
        assert!(parse(&[OWN, b"kern: $065, $300, -1"].concat()).is_err());
        assert!(parse(&[OWN, b"kerning: 1"].concat()).is_err());
    }
    #[test]
    fn append_keeps_original_bytes_and_line_endings() {
        let g = Glyph {
            x: 9,
            y: 64,
            w: 3,
            h: 6,
            left: 0,
            top: 1,
            advance: 4,
        };
        let out = append(OWN, &BTreeMap::from([(227u8, g.clone())])).unwrap();
        assert!(out.starts_with(OWN));
        assert!(out.ends_with(b"\r\n"));
        assert_eq!(parse(&out).unwrap().glyphs[&227], g);
        assert!(append(OWN, &BTreeMap::from([(65u8, g)])).is_err());
    }
}
