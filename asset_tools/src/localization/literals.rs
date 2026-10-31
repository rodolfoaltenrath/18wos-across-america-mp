//! T2 plain display strings only. Hashes/locators may be versioned; English
//! prose is available only through an explicit private inventory. Rebuilding
//! copies every byte outside the chosen quoted value ranges unchanged.
use super::{Mounts, def, sha};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ENTRIES: usize = 16384;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    Editable,
    ProperName,
    TableReference,
    RichText,
    Escaped,
    Empty,
    UnsupportedResource,
    DeferredEscEncoding,
}
#[derive(Clone, Debug, Serialize)]
pub struct Literal {
    pub key: String,
    pub attribute: String,
    pub line: usize,
    pub src: String,
    pub bytes: usize,
    pub policy: Policy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip)]
    start: usize,
    #[serde(skip)]
    end: usize,
}
impl Literal {
    /// Only used with the same buffer returned by the bounded lexer.
    pub(super) fn value<'a>(&self, source: &'a [u8]) -> &'a [u8] {
        &source[self.start..self.end]
    }
}
#[derive(Serialize)]
pub struct Resource {
    pub path: String,
    pub src: String,
    pub entries: Vec<Literal>,
}
#[derive(Serialize)]
pub struct Inventory {
    pub schema: u32,
    pub resources: Vec<Resource>,
    pub eligible: usize,
    pub protected: usize,
    pub unreadable_resources: Vec<String>,
    pub contains_private_source: bool,
    pub game_tested: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub src: String,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileCatalog {
    pub src: String,
    pub entries: BTreeMap<String, Entry>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub language: String,
    pub source: String,
    pub files: BTreeMap<String, FileCatalog>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub translated: usize,
    pub files: usize,
    pub errors: Vec<String>,
    pub long: Vec<String>,
    pub game_tested: bool,
    pub all_t2_translated: bool,
}
pub struct Compiled {
    pub outputs: BTreeMap<String, Vec<u8>>,
    pub report: Report,
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn scan_path(path: &str) -> bool {
    if path.starts_with("defs/seg_server/") || path.starts_with("defs/prefabs/") {
        return false;
    }
    (path.starts_with("makes/") || path.starts_with("defs/")) && path.ends_with(".def")
        || path.starts_with("ui/") && path.ends_with(".esc")
}
/// Narrow writable surface shared by compiler and installer. Neither editor,
/// audio, UI scripts nor arbitrary gameplay definitions can become translations.
pub fn writable_path(path: &str) -> bool {
    if crate::safe_relative(path).is_err() {
        return false;
    }
    let p: Vec<_> = path.split('/').collect();
    (p.len() == 6
        && p[0] == "makes"
        && matches!(p[1], "truck" | "trailer")
        && p[3] == "acc"
        && p[5] == "!desc.def")
        || (p.len() == 4
            && p[0] == "makes"
            && matches!(p[1], "truck" | "trailer")
            && p[3] == "description.def")
        || (p.len() == 3 && p[0] == "defs" && p[1] == "scenarios" && p[2].ends_with(".def"))
}
fn policy(path: &str, attribute: &str, value: &[u8]) -> Policy {
    if path.ends_with(".esc") {
        return Policy::DeferredEscEncoding;
    }
    if attribute.ends_with("_pml") || value.contains(&b'<') || value.contains(&b'>') {
        return Policy::RichText;
    }
    if value.windows(2).any(|w| w == b"@@") {
        return Policy::TableReference;
    }
    if value.contains(&b'\\') || value.iter().any(|b| b.is_ascii_control()) {
        return Policy::Escaped;
    }
    if value.iter().all(u8::is_ascii_whitespace) {
        return Policy::Empty;
    }
    if attribute == "name" && path.ends_with("/description.def") {
        return Policy::ProperName;
    }
    if attribute == "name"
        && path.starts_with("makes/truck/")
        && path.split('/').nth(4).is_some_and(|p| {
            p.strip_prefix("engine").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        })
    {
        return Policy::ProperName;
    }
    if !writable_path(path) {
        return if attribute == "name" {
            Policy::ProperName
        } else {
            Policy::UnsupportedResource
        };
    }
    if path.starts_with("defs/scenarios/") && !matches!(attribute, "name" | "instructions")
        || path.starts_with("makes/") && !matches!(attribute, "name" | "text" | "description")
    {
        return Policy::UnsupportedResource;
    }
    Policy::Editable
}
enum Token {
    Word(String),
    Colon,
    String(usize, usize),
    Other,
}
fn tokenize(bytes: &[u8]) -> Result<Vec<Token>> {
    ensure!(bytes.len() <= 1024 * 1024, "literal resource exceeds limit");
    let mut at = 0;
    let mut tokens = vec![];
    while at < bytes.len() {
        let b = bytes[at];
        if b.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if b == b'#' || bytes.get(at..at + 2) == Some(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if bytes.get(at..at + 2) == Some(b"/*") {
            at += 2;
            let n = bytes[at..]
                .windows(2)
                .position(|w| w == b"*/")
                .context("unterminated block comment")?;
            at += n + 2;
            continue;
        }
        if b == b'"' {
            at += 1;
            let start = at;
            while at < bytes.len() && bytes[at] != b'"' {
                if bytes[at] == b'\\' {
                    at += 1;
                    ensure!(at < bytes.len(), "truncated string escape");
                }
                at += 1;
            }
            ensure!(at < bytes.len(), "unterminated quoted string");
            tokens.push(Token::String(start, at));
            at += 1;
        } else if b.is_ascii_alphabetic() || b == b'_' {
            let start = at;
            at += 1;
            while bytes
                .get(at)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                at += 1;
            }
            ensure!(at - start <= 128, "literal identifier too long");
            tokens.push(Token::Word(String::from_utf8(bytes[start..at].to_vec())?));
        } else {
            tokens.push(if b == b':' {
                Token::Colon
            } else {
                Token::Other
            });
            at += 1;
        }
        ensure!(tokens.len() <= 262144, "literal token limit");
    }
    Ok(tokens)
}
pub fn parse(path: &str, bytes: &[u8], include_source: bool) -> Result<Vec<Literal>> {
    let tokens = tokenize(bytes)?;
    let mut counters = BTreeMap::<String, usize>::new();
    let mut entries = vec![];
    let lines: Vec<_> = bytes
        .iter()
        .enumerate()
        .filter_map(|(i, b)| (*b == b'\n').then_some(i))
        .collect();
    for window in tokens.windows(3) {
        let (attribute, start, end) = match window {
            [
                Token::Word(attribute),
                Token::Colon,
                Token::String(start, end),
            ] => (attribute.clone(), *start, *end),
            [Token::String(a, b), Token::Colon, Token::String(start, end)] => {
                let Ok(attribute) = std::str::from_utf8(&bytes[*a..*b]) else {
                    continue;
                };
                (attribute.to_owned(), *start, *end)
            }
            _ => continue,
        };
        if !matches!(
            attribute.as_str(),
            "name" | "text" | "instructions" | "description" | "value"
        ) && !attribute.ends_with("_pml")
        {
            continue;
        }
        let count = counters.entry(attribute.clone()).or_default();
        let value = &bytes[start..end];
        entries.push(Literal {
            key: format!("{attribute}#{count}"),
            attribute: attribute.clone(),
            line: lines.partition_point(|i| *i < start) + 1,
            src: sha(value),
            bytes: value.len(),
            policy: policy(path, &attribute, value),
            source: include_source.then(|| def::decode(value)),
            start,
            end,
        });
        *count += 1;
        ensure!(entries.len() <= 4096, "too many literal fields");
    }
    Ok(entries)
}
pub fn inventory(mounts: &Mounts, include_source: bool) -> Result<Inventory> {
    let files = mounts.read_matching(scan_path)?;
    let mut result = Inventory {
        schema: 1,
        resources: vec![],
        eligible: 0,
        protected: 0,
        unreadable_resources: vec![],
        contains_private_source: include_source,
        game_tested: false,
    };
    let mut count = 0;
    for (path, bytes) in files {
        let entries = match parse(&path, &bytes, include_source) {
            Ok(entries) => entries,
            Err(e) => {
                result.unreadable_resources.push(format!("{path}: {e:#}"));
                continue;
            }
        };
        count += entries.len();
        ensure!(count <= MAX_ENTRIES, "literal inventory entry limit");
        if entries.is_empty() {
            continue;
        }
        for entry in &entries {
            if entry.policy == Policy::Editable {
                result.eligible += 1;
            } else {
                result.protected += 1;
            }
        }
        result.resources.push(Resource {
            path,
            src: sha(&bytes),
            entries,
        });
    }
    Ok(result)
}
pub fn load(bytes: &[u8]) -> Result<Catalog> {
    ensure!(bytes.len() <= 4 * 1024 * 1024, "literal catalog size limit");
    let cat: Catalog = serde_json::from_slice(bytes).context("invalid literal catalog")?;
    ensure!(
        cat.schema == 1 && cat.language == "pt-BR" && cat.source == "literal-def-v1",
        "unsupported literal catalog schema"
    );
    ensure!(
        !cat.files.is_empty() && cat.files.len() <= 1024,
        "literal catalog file count"
    );
    let mut count = 0;
    for (path, file) in &cat.files {
        ensure!(
            writable_path(path) && valid_hash(&file.src),
            "invalid literal resource/hash: {path}"
        );
        ensure!(!file.entries.is_empty(), "empty literal file entries");
        count += file.entries.len();
        for entry in file.entries.values() {
            ensure!(valid_hash(&entry.src), "invalid value hash");
        }
    }
    ensure!(count <= MAX_ENTRIES, "literal catalog entry count");
    Ok(cat)
}
fn validate_text(original: &[u8], text: &str) -> Result<Vec<u8>> {
    ensure!(!text.trim().is_empty(), "blank translated literal");
    ensure!(
        !text.contains(['\\', '<', '>', '@']),
        "markup/references/escapes are outside this plain-text stage"
    );
    let encoded = def::encode(text)?;
    ensure!(encoded.len() <= 8192, "translated literal length limit");
    ensure!(
        encoded != original,
        "unchanged literal; omit it from the catalog"
    );
    // Conservative printf-like tokens are kept exact and ordered, including %%.
    fn formats(bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut out = vec![];
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] != b'%' {
                at += 1;
                continue;
            }
            let begin = at;
            at += 1;
            if bytes.get(at) == Some(&b'%') {
                out.push(b"%%".to_vec());
                at += 1;
                continue;
            }
            while bytes
                .get(at)
                .is_some_and(|b| b.is_ascii_digit() || b"-+.#$hlzjtL* ".contains(b))
            {
                at += 1;
            }
            if bytes
                .get(at)
                .is_some_and(|b| b"diuoxXfFeEgGaAcspn".contains(b))
            {
                at += 1;
                out.push(bytes[begin..at].to_vec());
            }
        }
        out
    }
    ensure!(
        formats(original) == formats(&encoded),
        "format placeholders differ"
    );
    Ok(encoded)
}
fn rebuild(path: &str, original: &[u8], values: &BTreeMap<String, Entry>) -> Result<Vec<u8>> {
    let fields = parse(path, original, false)?;
    let lookup: BTreeMap<_, _> = fields.iter().map(|f| (f.key.as_str(), f)).collect();
    let mut edits = vec![];
    for (key, entry) in values {
        let field = lookup
            .get(key.as_str())
            .with_context(|| format!("unknown field {key}"))?;
        ensure!(
            field.policy == Policy::Editable,
            "protected literal {key}: {:?}",
            field.policy
        );
        ensure!(field.src == entry.src, "stale literal {key}");
        edits.push((
            field.start,
            field.end,
            validate_text(&original[field.start..field.end], &entry.text)?,
        ));
    }
    edits.sort_by_key(|e| e.0);
    let mut out = vec![];
    let mut at = 0;
    for (start, end, replacement) in edits {
        ensure!(start >= at, "overlapping edits");
        out.extend_from_slice(&original[at..start]);
        out.extend_from_slice(&replacement);
        at = end;
    }
    out.extend_from_slice(&original[at..]);
    ensure!(out.len() <= 1024 * 1024, "rebuilt resource limit");
    Ok(out)
}
pub fn compile(mounts: &Mounts, catalog: &Catalog) -> Result<Compiled> {
    // Public callers cannot bypass schema/path validation with hand-built structs.
    let catalog = load(&serde_json::to_vec(catalog)?)?;
    let originals = mounts.read_matching(|p| catalog.files.contains_key(p))?;
    let mut result = Compiled {
        outputs: BTreeMap::new(),
        report: Report {
            schema: 1,
            translated: 0,
            files: 0,
            errors: vec![],
            long: vec![],
            game_tested: false,
            all_t2_translated: false,
        },
    };
    for (path, file) in &catalog.files {
        let checked = (|| -> Result<Vec<u8>> {
            let bytes = originals.get(path).context("source resource missing")?;
            ensure!(
                sha(bytes) == file.src,
                "source resource changed; refusing stale context"
            );
            let fields = parse(path, bytes, false)?;
            for field in &fields {
                if let Some(entry) = file.entries.get(&field.key)
                    && entry.text.len() > field.bytes + 12
                {
                    result.report.long.push(format!("{path}:{}", field.key));
                }
            }
            rebuild(path, bytes, &file.entries)
        })();
        match checked {
            Ok(bytes) => {
                result.report.translated += file.entries.len();
                result.outputs.insert(path.clone(), bytes);
            }
            Err(e) => result.report.errors.push(format!("{path}: {e:#}")),
        }
    }
    result.report.files = result.outputs.len();
    Ok(result)
}
/// Bind authored PT-BR text to source hashes without storing original strings.
/// A choice applies to matching editable values only; protected matches stay put.
pub fn compose(mounts: &Mounts, choices: &BTreeMap<String, String>) -> Result<Catalog> {
    ensure!(
        !choices.is_empty() && choices.len() <= 4096 && choices.keys().all(|h| valid_hash(h)),
        "invalid literal choices"
    );
    let mut result = Catalog {
        schema: 1,
        language: "pt-BR".into(),
        source: "literal-def-v1".into(),
        files: BTreeMap::new(),
    };
    let scan = inventory(mounts, true)?;
    let mut used = BTreeSet::new();
    for resource in scan.resources {
        let mut entries = BTreeMap::new();
        for field in resource.entries {
            if field.policy != Policy::Editable {
                continue;
            }
            if let Some(text) = choices.get(&field.src) {
                let original = field.source.context("missing private source")?;
                validate_text(
                    &original.chars().map(|c| c as u32 as u8).collect::<Vec<_>>(),
                    text,
                )?;
                used.insert(field.src.clone());
                entries.insert(
                    field.key,
                    Entry {
                        src: field.src,
                        text: text.clone(),
                    },
                );
            }
        }
        if !entries.is_empty() {
            result.files.insert(
                resource.path,
                FileCatalog {
                    src: resource.src,
                    entries,
                },
            );
        }
    }
    ensure!(
        used.len() == choices.len(),
        "choice hashes not found in editable fields"
    );
    Ok(result)
}
/// Installation guard: only authorized quoted display values may differ.
/// All mechanical fields, references, comments, whitespace and proper names
/// must reconstruct byte-for-byte from the installed originals.
pub fn verify_translation(path: &str, original: &[u8], translated: &[u8]) -> Result<()> {
    ensure!(
        writable_path(path),
        "resource outside literal translation scope"
    );
    let old = parse(path, original, false)?;
    let new = parse(path, translated, true)?;
    ensure!(old.len() == new.len(), "literal structure changed");
    let mut changes = BTreeMap::new();
    for (a, b) in old.iter().zip(&new) {
        ensure!(
            a.key == b.key && a.attribute == b.attribute,
            "literal identities changed"
        );
        if a.src != b.src {
            changes.insert(
                a.key.clone(),
                Entry {
                    src: a.src.clone(),
                    text: b.source.clone().context("missing target")?,
                },
            );
        }
    }
    ensure!(
        !changes.is_empty(),
        "literal resource has no translated values"
    );
    ensure!(
        rebuild(path, original, &changes)? == translated,
        "non-text bytes changed in literal resource"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, io::Write, path::Path};
    const PATH: &str = "makes/trailer/own/acc/blights/!desc.def";
    const OWN:&[u8]=b"# own fixture\r\ndef own {\r\n name: \"Own lamp\" # retain\r\n price:123\r\n model:\"own.psm\"\r\n}\r\n";
    fn entries(bytes: &[u8], text: &str) -> BTreeMap<String, Entry> {
        BTreeMap::from([(
            "name#0".into(),
            Entry {
                src: parse(PATH, bytes, false).unwrap()[0].src.clone(),
                text: text.into(),
            },
        )])
    }
    fn archive(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    #[test]
    fn lexer_preserves_comments_line_endings_duplicates_and_protects_rich_fields() {
        let source=b"// name:\"ignored\"\r\n/* text:\"ignored\" */\r\nname:\"Own\"\r\nname:\"Next\"\r\ntext:\"@@own\"\r\ndescription:\"\\nOwn\"\r\np_pml:\"<own>\"\r\nvalue:\" \"\r\n";
        let e = parse(PATH, source, false).unwrap();
        assert_eq!(e.len(), 6);
        assert_eq!((&e[0].key, e[0].line), (&"name#0".to_string(), 3));
        assert_eq!(e[1].key, "name#1");
        assert_eq!(
            e.iter().map(|f| f.policy.clone()).collect::<Vec<_>>(),
            vec![
                Policy::Editable,
                Policy::Editable,
                Policy::TableReference,
                Policy::Escaped,
                Policy::RichText,
                Policy::Empty
            ]
        );
        assert!(e.iter().all(|f| f.source.is_none()));
        assert!(!serde_json::to_string(&e).unwrap().contains("Next"));
        assert_eq!(
            parse(PATH, source, true).unwrap()[1].source.as_deref(),
            Some("Next")
        );
        let escaped = parse(PATH, b"name:\"Own \\\"lamp\\\"\"", false).unwrap();
        assert_eq!(escaped[0].policy, Policy::Escaped);
    }
    #[test]
    fn explicit_surface_preserves_names_ui_and_other_gameplay_definitions() {
        for path in [
            "makes/truck/own/description.def",
            "makes/truck/own/acc/engine0/!desc.def",
            "defs/cities/own.def",
        ] {
            assert_eq!(
                parse(path, b"name:\"Own\"", false).unwrap()[0].policy,
                Policy::ProperName
            );
        }
        assert_eq!(
            parse("ui/own.esc", b"\"text\":\"Own\"", false).unwrap()[0].policy,
            Policy::DeferredEscEncoding
        );
        assert_eq!(
            parse("defs/scenarios/own.def", b"value:\"Own\"", false).unwrap()[0].policy,
            Policy::UnsupportedResource
        );
        assert!(!writable_path("../makes/truck/own/description.def"));
        assert!(!writable_path("defs/own.def"));
        assert!(!scan_path("defs/seg_server/own.def"));
    }
    #[test]
    fn rebuilding_changes_only_display_bytes_and_installer_refuses_mechanical_tampering() {
        let out = rebuild(PATH, OWN, &entries(OWN, "Luz de freio")).unwrap();
        assert!(rebuild(PATH, OWN, &entries(OWN, "Own lamp")).is_err());
        assert_eq!(
            out,
            def::decode(OWN)
                .replace("Own lamp", "Luz de freio")
                .as_bytes()
        );
        verify_translation(PATH, OWN, &out).unwrap();
        for changed in [
            def::decode(&out).replace("price:123", "price:456"),
            def::decode(&out).replace("own.psm", "other.psm"),
            def::decode(&out).replace("# retain", "# changed"),
        ] {
            assert!(verify_translation(PATH, OWN, changed.as_bytes()).is_err());
        }
        assert!(verify_translation(PATH, OWN, OWN).is_err());
        assert!(verify_translation("makes/truck/own/description.def", OWN, &out).is_err());
        let mut stale = entries(OWN, "Luz");
        stale.get_mut("name#0").unwrap().src = sha(b"different");
        assert!(rebuild(PATH, OWN, &stale).is_err());
        stale.insert("missing#0".into(), stale["name#0"].clone());
        assert!(rebuild(PATH, OWN, &stale).is_err());
    }
    #[test]
    fn single_byte_accents_and_exact_placeholders_refuse_injection() {
        assert_eq!(
            validate_text(b"Own %02d %% %s", "Ação %02d %% %s").unwrap(),
            b"A\xe7\xe3o %02d %% %s"
        );
        for text in [
            "",
            "\n",
            "Texto\"\nprice:0",
            "@@own",
            "<own>",
            "\\n",
            "Texto 😀",
            "Texto %d",
            "Texto %s %02d %%",
        ] {
            assert!(validate_text(b"Own %02d %% %s", text).is_err(), "{text:?}");
        }
        assert!(validate_text(b"Own", "Texto %n").is_err());
    }
    #[test]
    fn malformed_and_excessive_inputs_are_refused_without_repairs() {
        for bytes in [
            b"name:\"broken".as_slice(),
            b"/*broken",
            b"name:\"truncated\\",
        ] {
            assert!(parse(PATH, bytes, false).is_err());
        }
        assert!(parse(PATH, &vec![b' '; 1024 * 1024 + 1], false).is_err());
        assert!(parse(PATH, "name:\"Own\"\n".repeat(4097).as_bytes(), false).is_err());
    }
    #[test]
    fn compose_and_compile_pin_full_context_mount_priority_and_never_copy_source_prose() {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().join("base.scs");
        let patch = t.path().join("patch.scs");
        archive(&base, &[(PATH, b"name:\"Lower own\"\nprice:1")]);
        archive(
            &patch,
            &[
                (PATH, OWN),
                ("makes/truck/own/description.def", OWN),
                (
                    "makes/trailer/own/acc/wheel/!desc.def",
                    b"name:\"Own lamp\"\nmodel:\"broken",
                ),
            ],
        );
        let mounts = Mounts::open(&[base.clone(), patch.clone()]).unwrap();
        let choices = BTreeMap::from([(sha(b"Own lamp"), "Luz de freio".into())]);
        let inv = inventory(&mounts, false).unwrap();
        assert_eq!(
            (inv.eligible, inv.protected, inv.unreadable_resources.len()),
            (1, 1, 1)
        );
        let cat = compose(&mounts, &choices).unwrap();
        assert_eq!(cat.files.len(), 1);
        let json = serde_json::to_vec(&cat).unwrap();
        assert!(!String::from_utf8_lossy(&json).contains("Own lamp"));
        let compiled = compile(&mounts, &load(&json).unwrap()).unwrap();
        assert_eq!((compiled.report.translated, compiled.report.files), (1, 1));
        assert!(compiled.report.errors.is_empty());
        verify_translation(PATH, OWN, &compiled.outputs[PATH]).unwrap();
        // A purely mechanical source change still invalidates the pinned context.
        archive(
            &patch,
            &[(
                PATH,
                &OWN.iter()
                    .copied()
                    .chain(b"# changed".iter().copied())
                    .collect::<Vec<_>>(),
            )],
        );
        let bad = compile(&mounts, &cat).unwrap();
        assert!(bad.outputs.is_empty());
        assert_eq!(bad.report.errors.len(), 1);
        assert!(
            compose(
                &mounts,
                &BTreeMap::from([(sha(b"not in any field"), "Outro".into())])
            )
            .is_err()
        );
    }
    #[test]
    fn batch_reader_rejects_selected_duplicates_and_cross_mount_case_collisions() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a.scs");
        let b = t.path().join("b.scs");
        archive(&a, &[(PATH, OWN), (&PATH.to_ascii_uppercase(), OWN)]);
        let mounts = Mounts::open(std::slice::from_ref(&a)).unwrap();
        assert!(mounts.read_matching(|p| p == PATH).is_err());
        archive(&a, &[(PATH, OWN)]);
        archive(&b, &[(&PATH.to_ascii_uppercase(), OWN)]);
        assert!(
            Mounts::open(&[a, b])
                .unwrap()
                .read_matching(|_| true)
                .is_err()
        );
    }
    #[test]
    fn schema_is_strict_and_hand_built_catalogs_cannot_bypass_guards() {
        let mut cat = Catalog {
            schema: 1,
            language: "pt-BR".into(),
            source: "literal-def-v1".into(),
            files: BTreeMap::from([(
                PATH.into(),
                FileCatalog {
                    src: sha(OWN),
                    entries: entries(OWN, "Luz"),
                },
            )]),
        };
        let mut json = serde_json::to_value(&cat).unwrap();
        json["source_prose"] = "own unwanted export".into();
        assert!(load(&serde_json::to_vec(&json).unwrap()).is_err());
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("own.scs");
        archive(&a, &[(PATH, OWN)]);
        cat.files
            .insert("ui/own.esc".into(), cat.files[PATH].clone());
        assert!(compile(&Mounts::open(&[a]).unwrap(), &cat).is_err());
    }
}
