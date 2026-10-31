//! Read-only PML/reference audit. No original prose, attributes or rendering
//! assumptions are exported. Recognizing syntax does not validate the engine.
use super::{Mounts, TABLE, catalog, def, literals};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Debug, Serialize)]
pub struct Syntax {
    pub references: BTreeSet<String>,
    pub parameters: Vec<u16>,
    pub tags: BTreeMap<String, usize>,
    pub colors: BTreeSet<char>,
    pub literal_letter_bytes: usize,
    /// Codes and byte offsets only: never original text or tag attributes.
    pub issues: Vec<String>,
}
fn reference(bytes: &[u8], at: &mut usize, result: &mut Syntax) {
    let begin = *at;
    *at += 2;
    let end = bytes[*at..]
        .windows(2)
        .position(|p| p == b"@@")
        .map(|n| *at + n);
    if let Some(end) = end {
        let key = &bytes[*at..end];
        if (1..=128).contains(&key.len())
            && key
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(c))
        {
            result
                .references
                .insert(String::from_utf8_lossy(key).into_owned());
        } else {
            result.issues.push(format!("invalid_reference@{begin}"));
        }
        *at = end + 2;
    } else {
        result.issues.push(format!("unclosed_reference@{begin}"));
        *at = bytes.len();
    }
}
fn parameter(bytes: &[u8], at: &mut usize, result: &mut Syntax) {
    let begin = *at;
    *at += 1;
    if bytes.get(*at) == Some(&b'%') {
        *at += 1;
        return;
    }
    let start = *at;
    while bytes.get(*at).is_some_and(|b| b.is_ascii_digit()) {
        *at += 1;
    }
    if *at > start && *at - start <= 3 {
        result.parameters.push(
            std::str::from_utf8(&bytes[start..*at])
                .unwrap()
                .parse()
                .unwrap(),
        );
    } else {
        result.issues.push(format!("unsupported_parameter@{begin}"));
    }
}
fn markers(bytes: &[u8], result: &mut Syntax) -> Result<()> {
    let mut at = 0;
    while at < bytes.len() {
        if bytes.get(at..at + 2) == Some(b"@@") {
            reference(bytes, &mut at, result);
        } else if bytes[at] == b'%' {
            parameter(bytes, &mut at, result);
        } else {
            at += 1;
        }
        ensure!(
            result.issues.len() <= 1024
                && result.references.len() <= 4096
                && result.parameters.len() <= 4096,
            "PML marker limit"
        );
    }
    Ok(())
}
pub fn analyze(bytes: &[u8]) -> Result<Syntax> {
    ensure!(bytes.len() <= 1024 * 1024, "PML field exceeds limit");
    let mut result = Syntax::default();
    let mut stack = vec![];
    let mut at = 0;
    let mut tokens = 0;
    while at < bytes.len() {
        tokens += 1;
        ensure!(tokens <= 65536, "PML token limit");
        match bytes[at] {
            b'<' => {
                let begin = at;
                at += 1;
                let start = at;
                let mut quote = None;
                while at < bytes.len() {
                    if let Some(q) = quote {
                        if bytes[at] == q {
                            quote = None;
                        }
                    } else if matches!(bytes[at], b'\'' | b'"') {
                        quote = Some(bytes[at]);
                    } else if bytes[at] == b'>' {
                        break;
                    }
                    at += 1;
                }
                if at == bytes.len() {
                    result.issues.push(format!("unclosed_tag@{begin}"));
                    break;
                }
                let body = &bytes[start..at];
                markers(body, &mut result)?;
                let closing = body.first() == Some(&b'/');
                let body = if closing { &body[1..] } else { body };
                let n = body
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_')
                    .count();
                let tag = String::from_utf8_lossy(&body[..n]).to_ascii_lowercase();
                if n == 0 || n > 64 {
                    result.issues.push(format!("invalid_tag@{begin}"));
                } else {
                    *result.tags.entry(tag.clone()).or_default() += 1;
                    if matches!(tag.as_str(), "font" | "align") {
                        if closing {
                            if stack.last() != Some(&tag) {
                                result.issues.push(format!("container_mismatch@{begin}"));
                            } else {
                                stack.pop();
                            }
                        } else {
                            ensure!(stack.len() < 64, "PML nesting limit");
                            stack.push(tag);
                        }
                    } else if !matches!(tag.as_str(), "img" | "offset" | "ret" | "br" | "color")
                        || closing
                    {
                        result.issues.push(format!("unknown_tag@{begin}"));
                    }
                }
                at += 1;
            }
            b'@' if bytes.get(at..at + 2) == Some(b"@@") => reference(bytes, &mut at, &mut result),
            b'%' => parameter(bytes, &mut at, &mut result),
            b'\\' => {
                let begin = at;
                while bytes.get(at) == Some(&b'\\') {
                    at += 1;
                }
                let count = at - begin;
                if matches!(count, 2 | 4) && bytes.get(at).is_some_and(|c| b"ywgrmbc".contains(c)) {
                    result.colors.insert(bytes[at] as char);
                    at += 1;
                } else {
                    result.issues.push(format!("unknown_escape@{begin}"));
                }
            }
            b'>' => {
                result.issues.push(format!("unmatched_close@{at}"));
                at += 1;
            }
            c => {
                if c.is_ascii_alphabetic() || c >= 128 {
                    result.literal_letter_bytes += 1;
                }
                at += 1;
            }
        }
        ensure!(
            result.issues.len() <= 1024
                && result.references.len() <= 4096
                && result.parameters.len() <= 4096,
            "PML diagnostic limit"
        );
    }
    if !stack.is_empty() {
        result.issues.push("unclosed_container".into());
    }
    Ok(result)
}
#[derive(Serialize)]
pub struct Field {
    pub path: String,
    pub key: String,
    pub src: String,
    pub syntax: Syntax,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: u32,
    pub fields: Vec<Field>,
    pub referenced_keys: usize,
    pub translated_keys: usize,
    pub preserved_config_keys: usize,
    /// Candidate selector families: variants exist; runtime selection unproven.
    pub font_selectors: BTreeMap<String, Vec<String>>,
    pub missing_keys: Vec<String>,
    pub untranslated_keys: Vec<String>,
    pub catalog_errors: Vec<String>,
    pub unreadable_resources: Vec<String>,
    pub fields_with_literal_text: usize,
    pub structural_issues: usize,
    pub game_tested: bool,
    pub full_translation_verified: bool,
    pub runtime_selection_verified: bool,
}
impl Report {
    pub fn checks_passed(&self) -> bool {
        self.missing_keys.is_empty()
            && self.untranslated_keys.is_empty()
            && self.catalog_errors.is_empty()
            && self.unreadable_resources.is_empty()
            && self.structural_issues == 0
    }
}
pub fn audit(mounts: &Mounts, cat: &catalog::Catalog) -> Result<Report> {
    let table = def::parse(&mounts.read(TABLE)?)?;
    let checked = catalog::check(cat, &table);
    let keys: BTreeSet<_> = table.iter().map(|e| e.key.as_str()).collect();
    let mut errors = checked.errors;
    errors.extend(checked.stale.into_iter().map(|key| format!("stale:{key}")));
    let mut r = Report {
        schema: 1,
        fields: vec![],
        referenced_keys: 0,
        translated_keys: 0,
        preserved_config_keys: 0,
        font_selectors: BTreeMap::new(),
        missing_keys: vec![],
        untranslated_keys: vec![],
        catalog_errors: errors,
        unreadable_resources: vec![],
        fields_with_literal_text: 0,
        structural_issues: 0,
        game_tested: false,
        full_translation_verified: false,
        runtime_selection_verified: false,
    };
    let files = mounts.read_matching(|p| {
        p.starts_with("ui/") && !p.starts_with("ui/editor") && p.ends_with(".esc")
            || p.starts_with("defs/ui/") && p.ends_with(".def")
    })?;
    let mut referenced = BTreeSet::new();
    for (path, bytes) in files {
        let fields = match literals::parse(&path, &bytes, false) {
            Ok(f) => f,
            Err(e) => {
                r.unreadable_resources.push(format!("{path}: {e:#}"));
                continue;
            }
        };
        for field in fields {
            // A typed boolean flag, not a string containing PML.
            if field.attribute == "p2p_pml" {
                continue;
            }
            let value = field.value(&bytes);
            if !field.attribute.ends_with("_pml")
                && !value.contains(&b'<')
                && !value.windows(2).any(|p| p == b"@@")
            {
                continue;
            }
            let syntax = analyze(value)?;
            referenced.extend(syntax.references.iter().cloned());
            r.structural_issues += syntax.issues.len();
            r.fields_with_literal_text += usize::from(syntax.literal_letter_bytes > 0);
            r.fields.push(Field {
                path: path.clone(),
                key: field.key,
                src: field.src,
                syntax,
            });
            ensure!(r.fields.len() <= 16384, "PML field count");
        }
    }
    r.referenced_keys = referenced.len();
    for key in referenced {
        if !keys.contains(key.as_str()) {
            let variants: Vec<_> = (0..4).map(|i| format!("{key}_{i}")).collect();
            if matches!(key.as_str(), "__fnt_sml" | "__fnt_mid" | "__fnt_big")
                && variants.iter().all(|v| keys.contains(v.as_str()))
            {
                r.font_selectors.insert(key, variants);
            } else {
                r.missing_keys.push(key);
            }
        } else if !catalog::translatable(&key) {
            r.preserved_config_keys += 1;
        } else if cat.entries.contains_key(&key) {
            r.translated_keys += 1;
        } else {
            r.untranslated_keys.push(key);
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn syntax_tracks_attributes_parameters_colors_and_references_without_exporting_prose() {
        let own=b"<align hstyle=%1><font @@__fnt_own@@>\\\\y@@own@@ %0 %0 Own prose<ret><img src='private>own.shd'></font></align>";
        let r = analyze(own).unwrap();
        assert!(r.issues.is_empty());
        assert_eq!(r.parameters, [1, 0, 0]);
        assert_eq!(
            r.references,
            BTreeSet::from(["__fnt_own".into(), "own".into()])
        );
        assert_eq!(r.colors, BTreeSet::from(['y']));
        assert_eq!(r.literal_letter_bytes, 8);
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("Own prose") && !json.contains("private>own.shd"));
        assert!(
            analyze(b"<align><color value=FF707070><font>%0</font></align>")
                .unwrap()
                .issues
                .is_empty()
        );
    }
    #[test]
    fn malformed_and_unknown_constructs_are_metadata_only_and_bounded() {
        for source in [
            "<font>own</align>",
            "<script src=private>",
            "@@broken",
            "@@bad key@@",
            "%s",
            "<font",
            "\\q",
            ">",
            "</img>",
        ] {
            assert!(
                !analyze(source.as_bytes()).unwrap().issues.is_empty(),
                "{source}"
            );
        }
        assert!(analyze("<font>".repeat(65).as_bytes()).is_err());
        assert!(analyze(&vec![b' '; 1024 * 1024 + 1]).is_err());
        assert!(
            analyze(b"<font>@@own@@ %0</font>")
                .unwrap()
                .literal_letter_bytes
                == 0
        );
    }
    #[test]
    fn mount_audit_detects_drift_missing_translations_and_keeps_sources_private() {
        use std::{fs::File, io::Write};
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("own.scs");
        let mut z = zip::ZipWriter::new(File::create(&path).unwrap());
        for (p, b) in [
            (
                TABLE,
                "def translate {\nown:\"Own\"\n__fnt_own:\"face=own\"\n__fnt_big_0:\"face=a\"\n__fnt_big_1:\"face=b\"\n__fnt_big_2:\"face=c\"\n__fnt_big_3:\"face=d\"\n}\n",
            ),
            (
                "defs/ui/own.def",
                "def own {n_pml:\"<font @@__fnt_own@@>@@own@@ %0</font><font @@__fnt_big@@>%1</font>\"}",
            ),
            ("ui/own.esc", "\"text\":\"@@missing@@\""),
        ] {
            z.start_file(p, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let m = Mounts::open(&[path]).unwrap();
        let mut cat = catalog::Catalog {
            schema: 1,
            language: "pt-BR".into(),
            source: TABLE.into(),
            entries: BTreeMap::new(),
        };
        let r = audit(&m, &cat).unwrap();
        assert_eq!(r.missing_keys, ["missing"]);
        assert_eq!(r.untranslated_keys, ["own"]);
        assert_eq!(r.preserved_config_keys, 1);
        assert_eq!(r.font_selectors["__fnt_big"].len(), 4);
        assert!(!r.checks_passed());
        cat.entries.insert(
            "own".into(),
            catalog::Entry {
                src: catalog::source_hash("Own"),
                text: "Próprio".into(),
            },
        );
        let r = audit(&m, &cat).unwrap();
        assert_eq!(r.translated_keys, 1);
        assert!(r.untranslated_keys.is_empty());
        cat.entries.get_mut("own").unwrap().src = catalog::source_hash("Other");
        assert!(!audit(&m, &cat).unwrap().catalog_errors.is_empty());
        assert!(!serde_json::to_string(&r).unwrap().contains("face=own"));
    }
}
