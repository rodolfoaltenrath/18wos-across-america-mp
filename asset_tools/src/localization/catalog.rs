//! Authored PT-BR catalogue. It holds only our translations, keyed by the game's
//! keys, plus a short hash of each source value to detect upstream changes.
use super::def::{self, Entry as Source};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const LANGUAGE: &str = "pt-BR";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// First 12 hex digits of SHA-256 of the source value bytes.
    pub src: String,
    pub text: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub language: String,
    pub source: String,
    pub entries: BTreeMap<String, Entry>,
}
pub fn source_hash(value: &str) -> String {
    let bytes: Vec<u8> = value.chars().map(|c| c as u32 as u8).collect();
    format!("{:x}", Sha256::digest(bytes))[..12].to_string()
}
pub fn load(bytes: &[u8]) -> Result<Catalog> {
    ensure!(bytes.len() <= 4 * 1024 * 1024, "catalogue limit");
    let c: Catalog = serde_json::from_slice(bytes).context("invalid catalogue")?;
    ensure!(
        c.schema == 1 && c.language == LANGUAGE && c.source == "scripts/local.def",
        "unsupported catalogue"
    );
    ensure!(c.entries.len() <= 16384, "too many catalogue entries");
    Ok(c)
}

/// Colour codes (`\\y`) and `<br>` must survive translation in the same order.
pub fn markup(text: &str) -> Vec<String> {
    let mut found = vec![];
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("\\\\") {
            let code: String = after
                .chars()
                .take_while(char::is_ascii_lowercase)
                .take(1)
                .collect();
            found.push(format!("\\\\{code}"));
            rest = &after[code.len()..];
        } else if rest
            .get(..4)
            .is_some_and(|s| s.eq_ignore_ascii_case("<br>"))
        {
            found.push("<br>".into());
            rest = &rest[4..];
        } else {
            let n = rest.chars().next().map_or(1, char::len_utf8);
            rest = &rest[n..];
        }
    }
    found
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub source_keys: usize,
    pub translatable_keys: usize,
    pub translated: usize,
    pub untranslated: Vec<String>,
    pub stale: Vec<String>,
    pub repaired_source_lines: Vec<String>,
    pub long: Vec<String>,
    pub errors: Vec<String>,
    pub complete: bool,
}
fn translatable(key: &str) -> bool {
    !key.starts_with("__")
}
pub fn check(catalog: &Catalog, source: &[Source]) -> Report {
    let mut by_key: BTreeMap<&str, &Source> = BTreeMap::new();
    let mut r = Report::default();
    for e in source {
        if let Some(previous) = by_key.insert(&e.key, e)
            && previous.value != e.value
        {
            r.errors.push(format!(
                "{}: duplicate source key with different values",
                e.key
            ));
        }
        if e.repaired {
            r.repaired_source_lines.push(e.key.clone());
        }
    }
    r.source_keys = by_key.len();
    let keys: BTreeSet<&str> = by_key.keys().copied().filter(|k| translatable(k)).collect();
    r.translatable_keys = keys.len();
    for (key, entry) in &catalog.entries {
        let Some(src) = by_key.get(key.as_str()) else {
            r.errors.push(format!("{key}: not in source table"));
            continue;
        };
        if !translatable(key) {
            r.errors
                .push(format!("{key}: engine setup key must not be translated"));
            continue;
        }
        if entry.src != source_hash(&src.value) {
            r.stale.push(key.clone());
        }
        if let Err(e) = def::encode(&entry.text) {
            r.errors.push(format!("{key}: {e}"));
        }
        if markup(&entry.text) != markup(&src.value) {
            r.errors
                .push(format!("{key}: colour codes/<br> differ from source"));
        }
        if entry.text.trim().is_empty() != src.value.trim().is_empty() {
            r.errors
                .push(format!("{key}: blank/non-blank differs from source"));
        }
        let (n, m) = (entry.text.chars().count(), src.value.chars().count());
        if n > m + 12 && n * 2 > m * 3 {
            r.long.push(key.clone());
        }
        r.translated += 1;
    }
    r.untranslated = keys
        .iter()
        .filter(|k| !catalog.entries.contains_key(**k))
        .map(|k| k.to_string())
        .collect();
    r.complete = r.errors.is_empty() && r.stale.is_empty() && r.untranslated.is_empty();
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    const OWN: &[u8] = b"def translate {\n__fnt:\"face=own.font\"\nown_a:\"\\\\yOwn \\\\wtext<br>more\"\nown_b:\"Short\"\nown_c:Fixed\"\n}\n";
    fn entry(src: &str, text: &str) -> Entry {
        Entry {
            src: source_hash(src),
            text: text.into(),
        }
    }
    fn catalog(entries: &[(&str, Entry)]) -> Catalog {
        Catalog {
            schema: 1,
            language: LANGUAGE.into(),
            source: "scripts/local.def".into(),
            entries: entries
                .iter()
                .map(|(k, e)| (k.to_string(), e.clone()))
                .collect(),
        }
    }
    #[test]
    fn complete_catalogue_preserves_markup_and_flags_problems() {
        let source = def::parse(OWN).unwrap();
        let a = entry(
            "\\\\yOwn \\\\wtext<br>more",
            "\\\\yPróprio \\\\wtexto<br>mais",
        );
        let ok = catalog(&[
            ("own_a", a.clone()),
            ("own_b", entry("Short", "Curto")),
            ("own_c", entry("Fixed", "Corrigido")),
        ]);
        let r = check(&ok, &source);
        assert!(r.complete, "{r:?}");
        assert_eq!((r.translatable_keys, r.translated), (3, 3));
        assert_eq!(r.repaired_source_lines, ["own_c"]);

        let bad = catalog(&[
            (
                "own_a",
                entry("\\\\yOwn \\\\wtext<br>more", "\\\\wPróprio texto"),
            ),
            (
                "own_b",
                entry(
                    "Changed upstream",
                    "Um texto muito mais longo que o original",
                ),
            ),
            ("__fnt", entry("face=own.font", "x")),
            ("ghost", entry("x", "y")),
        ]);
        let r = check(&bad, &source);
        assert!(!r.complete);
        assert_eq!(r.stale, ["own_b"]);
        assert_eq!(r.long, ["own_b"]);
        assert_eq!(r.untranslated, ["own_c"]);
        assert_eq!(r.errors.len(), 3, "{:?}", r.errors);
        assert_eq!(markup("\\\\ya<BR>\\\\g"), ["\\\\y", "<br>", "\\\\g"]);
        assert_eq!(markup("ção<br>ã"), ["<br>"]);
    }
    #[test]
    fn catalogue_schema_is_strict() {
        let good = br#"{"schema":1,"language":"pt-BR","source":"scripts/local.def","entries":{"k":{"src":"0","text":"t"}}}"#;
        assert!(load(good).is_ok());
        assert!(
            load(
                &String::from_utf8_lossy(good)
                    .replace("pt-BR", "en")
                    .into_bytes()
            )
            .is_err()
        );
        assert!(
            load(
                &String::from_utf8_lossy(good)
                    .replace("\"t\"}", "\"t\",\"x\":1}")
                    .into_bytes()
            )
            .is_err()
        );
    }
}
