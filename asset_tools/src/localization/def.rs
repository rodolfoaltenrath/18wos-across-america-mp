//! `scripts/local.def` structure: `key:"value"` lines inside `def translate { }`.
//! Values are single-byte text; only the quoted part of a line is ever replaced.
use anyhow::{Result, bail, ensure};
use std::collections::BTreeMap;

/// Accented letters added to the ASCII range for PT-BR. Their Windows-1252
/// bytes equal the Unicode scalar, and the same values index the font slots.
pub const PT_LETTERS: &str = "ÀÁÂÃÇÉÊÍÓÔÕÚÜàáâãçéêíóôõúü";

pub fn encode(text: &str) -> Result<Vec<u8>> {
    text.chars()
        .map(|c| match c {
            ' '..='~' if c != '"' => Ok(c as u8),
            _ if PT_LETTERS.contains(c) => Ok(c as u32 as u8),
            _ => bail!("character {c:?} has no PT-BR glyph slot"),
        })
        .collect()
}
pub fn decode(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    pub line: usize,
    /// Original line lacks its opening quote; rebuilding repairs it.
    pub repaired: bool,
}
struct Span {
    key: (usize, usize),
    value: (usize, usize),
}
fn span(line: &[u8]) -> Option<Span> {
    let start = line.iter().position(|b| !b" \t".contains(b))?;
    let len = line[start..]
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
        .count();
    let mut at = start + len;
    if len == 0 || line.get(at) != Some(&b':') {
        return None;
    }
    at += 1;
    while line.get(at).is_some_and(|b| b" \t".contains(b)) {
        at += 1;
    }
    let open = line.get(at) == Some(&b'"');
    let begin = at + usize::from(open);
    let close = begin + line[begin..].iter().rposition(|b| *b == b'"')?;
    Some(Span {
        key: (start, start + len),
        value: (begin, close),
    })
}
fn lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split_inclusive(|b| *b == b'\n').collect()
}
fn body(line: &[u8]) -> &[u8] {
    let end = line.len()
        - line
            .iter()
            .rev()
            .take_while(|b| b"\r\n".contains(b))
            .count();
    &line[..end]
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>> {
    ensure!(bytes.len() <= 1024 * 1024, "local.def limit");
    let mut inside = false;
    let mut entries = vec![];
    for (n, line) in lines(bytes).into_iter().enumerate() {
        let text = body(line);
        let trimmed = decode(text);
        let trimmed = trimmed.trim();
        if trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }
        if !inside {
            inside = trimmed.starts_with("def translate") && trimmed.ends_with('{');
            continue;
        }
        if trimmed == "}" {
            inside = false;
            continue;
        }
        let Some(s) = span(text) else {
            bail!("line {}: unrecognized localization entry", n + 1)
        };
        entries.push(Entry {
            key: decode(&text[s.key.0..s.key.1]),
            value: decode(&text[s.value.0..s.value.1]),
            line: n,
            repaired: text[s.value.0 - 1] != b'"',
        });
        ensure!(entries.len() <= 16384, "too many localization entries");
    }
    ensure!(
        !entries.is_empty() && !inside,
        "translate block missing/unclosed"
    );
    Ok(entries)
}

/// Replaces values of the given keys (every duplicate line of a key) and keeps
/// all other bytes, comments and line endings unchanged.
pub fn rebuild(bytes: &[u8], values: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    let entries = parse(bytes)?;
    let by_line: BTreeMap<usize, &Entry> = entries.iter().map(|e| (e.line, e)).collect();
    for key in values.keys() {
        ensure!(
            entries.iter().any(|e| &e.key == key),
            "unknown localization key {key}"
        );
    }
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 4);
    for (n, line) in lines(bytes).into_iter().enumerate() {
        let replacement = by_line
            .get(&n)
            .and_then(|e| values.get(&e.key).map(|v| (e, v)));
        let Some((entry, value)) = replacement else {
            out.extend_from_slice(line);
            continue;
        };
        let s = span(body(line)).expect("parsed entry line");
        out.extend_from_slice(&line[..s.value.0 - usize::from(!entry.repaired)]);
        out.push(b'"');
        out.extend_from_slice(&encode(value)?);
        out.extend_from_slice(&line[s.value.1..]);
    }
    let check = parse(&out)?;
    ensure!(
        check.len() == entries.len()
            && check
                .iter()
                .all(|e| !e.repaired || !values.contains_key(&e.key)),
        "rebuilt table changed its structure"
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    const OWN: &[u8] = b"# own fixture\r\ndef translate {\r\n__fnt:\"face=own.font\"\r\nown_a:\"Own \\\\ytext\" # c\r\nown_b:Broken\"\r\n// note\r\nown_a:\"Own \\\\ytext\"\r\n}\r\n";
    #[test]
    fn parse_finds_entries_duplicates_and_missing_open_quote() {
        let e = parse(OWN).unwrap();
        assert_eq!(e.len(), 4);
        assert_eq!(e[1].value, "Own \\\\ytext");
        assert!(e[2].repaired && !e[1].repaired);
        assert_eq!(e[2].value, "Broken");
        assert!(parse(b"def translate {\nbad line\n}\n").is_err());
        assert!(parse(b"def translate {\nown:\"x\"\n").is_err());
    }
    #[test]
    fn rebuild_replaces_only_quoted_values_in_single_byte_text() {
        let values = BTreeMap::from([
            ("own_a".to_string(), "Ação \\\\ytexto".to_string()),
            ("own_b".to_string(), "Reparado".to_string()),
        ]);
        let out = rebuild(OWN, &values).unwrap();
        let text = decode(&out);
        assert!(text.contains("own_a:\"A\u{e7}\u{e3}o \\\\ytexto\" # c\r\n"));
        assert!(text.contains("own_b:\"Reparado\"\r\n"));
        assert_eq!(text.matches("A\u{e7}\u{e3}o").count(), 2);
        assert!(text.starts_with("# own fixture\r\n") && text.contains("__fnt:\"face=own.font\""));
        assert!(out.contains(&0xe7) && !out.contains(&0xc3));
        let bad = BTreeMap::from([("own_a".to_string(), "aspas \"".to_string())]);
        assert!(rebuild(OWN, &bad).is_err());
        let euro = BTreeMap::from([("own_a".to_string(), "€".to_string())]);
        assert!(rebuild(OWN, &euro).is_err());
        let unknown = BTreeMap::from([("nope".to_string(), "x".to_string())]);
        assert!(rebuild(OWN, &unknown).is_err());
    }
}
