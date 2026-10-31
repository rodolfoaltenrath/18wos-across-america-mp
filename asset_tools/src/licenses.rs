//! Third-party license inventory from `cargo metadata` (locked, offline).
//! Evaluates each declared SPDX expression against an explicit allowlist,
//! walks only non-dev dependencies of the shipped crates and builds a notice
//! with the license texts found in the local registry sources. Declarations
//! are the crates' own metadata: this is not legal review or file scanning.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

/// Permissive licenses accepted for binaries we hand to other people, in the
/// order preferred when a crate offers a choice.
pub const ALLOWED: [&str; 12] = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "0BSD",
    "Unlicense",
    "BSL-1.0",
    "CC0-1.0",
    "Unicode-3.0",
    "Unicode-DFS-2016",
];
/// `license WITH exception` pairs accepted as a unit.
pub const ALLOWED_EXCEPTIONS: [(&str, &str); 1] = [("Apache-2.0", "LLVM-exception")];
const MAX_EXPRESSION: usize = 256;
const MAX_TEXT: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    And,
    Or,
    With,
    Id(String),
}
fn tokens(expression: &str) -> Result<Vec<Token>> {
    ensure!(
        !expression.trim().is_empty() && expression.len() <= MAX_EXPRESSION,
        "empty or oversized license expression"
    );
    let spaced = expression
        .replace('(', " ( ")
        .replace(')', " ) ")
        .replace('/', " OR ");
    spaced
        .split_whitespace()
        .map(|t| {
            Ok(match t {
                "(" => Token::Open,
                ")" => Token::Close,
                "AND" => Token::And,
                "OR" => Token::Or,
                "WITH" => Token::With,
                id if id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-.+".contains(&b)) =>
                {
                    Token::Id(id.to_owned())
                }
                other => bail!("invalid license token {other:?}"),
            })
        })
        .collect()
}
/// Selected license text (e.g. "MIT", "Apache-2.0 AND Unicode-3.0") or None
/// when no branch satisfies the policy. Precedence: WITH > AND > OR.
struct Parser {
    tokens: Vec<Token>,
    at: usize,
}
impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }
    fn or(&mut self, depth: usize) -> Result<Option<String>> {
        ensure!(depth <= 16, "license expression nested too deeply");
        let mut choices = vec![self.and(depth)?];
        while self.peek() == Some(&Token::Or) {
            self.at += 1;
            choices.push(self.and(depth)?);
        }
        // Prefer the branch whose first license ranks best in ALLOWED.
        Ok(choices.into_iter().flatten().min_by_key(|s| {
            ALLOWED
                .iter()
                .position(|a| s.split_whitespace().next() == Some(a))
                .unwrap_or(ALLOWED.len())
        }))
    }
    fn and(&mut self, depth: usize) -> Result<Option<String>> {
        let mut parts = vec![self.with(depth)?];
        while self.peek() == Some(&Token::And) {
            self.at += 1;
            parts.push(self.with(depth)?);
        }
        Ok(parts
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .map(|p| p.join(" AND ")))
    }
    fn with(&mut self, depth: usize) -> Result<Option<String>> {
        let base = match self.tokens.get(self.at).cloned() {
            Some(Token::Open) => {
                self.at += 1;
                let inner = self.or(depth + 1)?;
                ensure!(self.peek() == Some(&Token::Close), "unbalanced parenthesis");
                self.at += 1;
                return Ok(inner);
            }
            Some(Token::Id(id)) => {
                self.at += 1;
                id
            }
            _ => bail!("expected a license identifier"),
        };
        if self.peek() == Some(&Token::With) {
            self.at += 1;
            let Some(Token::Id(exception)) = self.tokens.get(self.at).cloned() else {
                bail!("expected an exception after WITH");
            };
            self.at += 1;
            return Ok(ALLOWED_EXCEPTIONS
                .contains(&(base.as_str(), exception.as_str()))
                .then(|| format!("{base} WITH {exception}")));
        }
        Ok(ALLOWED.contains(&base.as_str()).then_some(base))
    }
}
pub fn evaluate(expression: &str) -> Result<Option<String>> {
    let mut p = Parser {
        tokens: tokens(expression)?,
        at: 0,
    };
    let selected = p.or(0)?;
    ensure!(
        p.at == p.tokens.len(),
        "trailing tokens in license expression"
    );
    Ok(selected)
}

#[derive(Debug, Serialize, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub license: Option<String>,
    pub selected: Option<String>,
    pub repository: Option<String>,
    /// License/notice file names found next to the crate's manifest.
    pub texts: Vec<String>,
    #[serde(skip)]
    pub directory: PathBuf,
}
#[derive(Debug, Serialize)]
pub struct Inventory {
    pub schema: u32,
    pub targets: Vec<String>,
    pub roots: Vec<String>,
    pub packages: Vec<Package>,
    /// name@version: reason. Empty means every declaration passed the policy.
    pub violations: Vec<String>,
    pub missing_texts: Vec<String>,
    pub passed: bool,
    pub legal_review: bool,
}
fn license_file(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "LICENSE",
        "LICENCE",
        "COPYING",
        "UNLICENSE",
        "NOTICE",
        "COPYRIGHT",
    ]
    .iter()
    .any(|p| upper.starts_with(p))
}
/// Packages reachable from `roots` without dev edges, from one metadata
/// document per target. Workspace crates are traversed but not listed.
pub fn inventory(documents: &[(String, Value)], roots: &[&str]) -> Result<Inventory> {
    ensure!(!roots.is_empty(), "no root crates");
    let mut found: BTreeMap<String, Package> = BTreeMap::new();
    for (target, metadata) in documents {
        let packages = metadata["packages"]
            .as_array()
            .context("metadata without packages")?;
        let by_id: BTreeMap<&str, &Value> = packages
            .iter()
            .filter_map(|p| Some((p["id"].as_str()?, p)))
            .collect();
        let nodes: BTreeMap<&str, &Value> = metadata["resolve"]["nodes"]
            .as_array()
            .context("metadata without resolve graph")?
            .iter()
            .filter_map(|n| Some((n["id"].as_str()?, n)))
            .collect();
        let mut queue: VecDeque<&str> = VecDeque::new();
        for root in roots {
            let id = packages
                .iter()
                .find(|p| p["name"] == *root && p["source"].is_null())
                .and_then(|p| p["id"].as_str())
                .with_context(|| format!("workspace crate {root} not in metadata for {target}"))?;
            queue.push_back(id);
        }
        let mut seen = BTreeSet::new();
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id) {
                continue;
            }
            let package = by_id
                .get(id)
                .with_context(|| format!("unknown package {id}"))?;
            if !package["source"].is_null() {
                let name = package["name"].as_str().context("package name")?;
                let version = package["version"].as_str().context("package version")?;
                let license = package["license"].as_str().map(str::to_owned);
                let selected = match &license {
                    Some(l) => evaluate(l).ok().flatten(),
                    None => None,
                };
                let directory = package["manifest_path"]
                    .as_str()
                    .map(|p| Path::new(p).parent().unwrap_or(Path::new("")).to_path_buf())
                    .unwrap_or_default();
                found
                    .entry(format!("{name}@{version}"))
                    .or_insert_with(|| Package {
                        name: name.into(),
                        version: version.into(),
                        license,
                        selected,
                        repository: package["repository"].as_str().map(str::to_owned),
                        texts: Vec::new(),
                        directory,
                    });
            }
            let node = nodes
                .get(id)
                .with_context(|| format!("no graph node for {id}"))?;
            for dep in node["deps"].as_array().into_iter().flatten() {
                let shipped = dep["dep_kinds"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|k| k["kind"].as_str() != Some("dev")));
                if shipped && let Some(next) = dep["pkg"].as_str() {
                    queue.push_back(next);
                }
            }
        }
    }
    let mut violations = Vec::new();
    for (key, p) in &found {
        match (&p.license, &p.selected) {
            (None, _) => violations.push(format!("{key}: no SPDX license declared")),
            (Some(l), None) => match evaluate(l) {
                Err(e) => violations.push(format!("{key}: unparsable {l:?}: {e}")),
                Ok(_) => violations.push(format!("{key}: {l} is outside the allowlist")),
            },
            _ => {}
        }
    }
    Ok(Inventory {
        schema: 1,
        targets: documents.iter().map(|(t, _)| t.clone()).collect(),
        roots: roots.iter().map(|r| r.to_string()).collect(),
        passed: violations.is_empty(),
        violations,
        missing_texts: Vec::new(),
        packages: found.into_values().collect(),
        legal_review: false,
    })
}
/// Attach license file names and render the notice. Identical texts are
/// written once and referenced afterwards to keep the file small.
pub fn notice(inventory: &mut Inventory, project: &str) -> String {
    let mut out = format!(
        "AVISOS DE TERCEIROS / THIRD-PARTY NOTICES\r\n\r\n\
{project} inclui as bibliotecas Rust abaixo, conforme a licença declarada por\r\n\
cada crate (seleção entre as opções oferecidas, quando houver). Gerado a partir\r\n\
do Cargo.lock; não é revisão jurídica.\r\n\r\n"
    );
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut missing = Vec::new();
    for p in &mut inventory.packages {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&p.directory)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(license_file)
            })
            .collect();
        files.sort();
        out += &format!(
            "== {} {} ==\r\nLicença declarada: {}\r\nUsada como: {}\r\nFonte: {}\r\n",
            p.name,
            p.version,
            p.license.as_deref().unwrap_or("(nenhuma)"),
            p.selected.as_deref().unwrap_or("(fora da política)"),
            p.repository.as_deref().unwrap_or("crates.io")
        );
        if files.is_empty() {
            missing.push(format!("{}@{}", p.name, p.version));
            out += "Texto: o pacote não traz arquivo de licença; vale o texto padrão da licença declarada.\r\n\r\n";
            continue;
        }
        for file in files {
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_owned();
            p.texts.push(name.clone());
            let text = std::fs::metadata(&file)
                .ok()
                .filter(|m| m.len() <= MAX_TEXT)
                .and_then(|_| std::fs::read_to_string(&file).ok())
                .unwrap_or_else(|| "(arquivo grande ou não UTF-8 omitido)".into());
            let label = format!("{}@{} {}", p.name, p.version, name);
            match texts.iter().find(|(_, t)| **t == text) {
                Some((first, _)) => out += &format!("--- {name}: idêntico a {first}\r\n"),
                None => {
                    out += &format!(
                        "--- {name} ---\r\n{}\r\n",
                        text.replace("\r\n", "\n").replace('\n', "\r\n")
                    );
                    texts.insert(label, text);
                }
            }
        }
        out += "\r\n";
    }
    inventory.missing_texts = missing;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn spdx_expressions_follow_precedence_preference_and_exceptions() {
        for (expression, expected) in [
            ("MIT", Some("MIT")),
            ("Apache-2.0 OR MIT", Some("MIT")),
            ("MIT/Apache-2.0", Some("MIT")),
            ("Unlicense OR MIT", Some("MIT")),
            (
                "(MIT OR Apache-2.0) AND Unicode-3.0",
                Some("MIT AND Unicode-3.0"),
            ),
            (
                "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
                Some("MIT"),
            ),
            (
                "Apache-2.0 WITH LLVM-exception",
                Some("Apache-2.0 WITH LLVM-exception"),
            ),
            ("GPL-3.0-only", None),
            ("GPL-3.0-only OR MIT", Some("MIT")),
            ("GPL-3.0-only AND MIT", None),
            ("MIT WITH Classpath-exception-2.0", None),
            ("MPL-2.0", None),
        ] {
            assert_eq!(
                evaluate(expression).unwrap().as_deref(),
                expected,
                "{expression}"
            );
        }
        for bad in [
            "",
            "MIT OR",
            "(MIT",
            "MIT)",
            "MIT AND AND ISC",
            "MIT;rm",
            "WITH MIT",
        ] {
            assert!(evaluate(bad).is_err(), "{bad:?}");
        }
        assert!(evaluate(&"(".repeat(40)).is_err());
    }
    fn metadata() -> Value {
        let pkg = |id: &str, name: &str, license: Option<&str>, source: Option<&str>| {
            json!({"id": id, "name": name, "version": "1.0.0", "license": license,
                "source": source, "repository": null, "manifest_path": format!("/nonexistent/{name}/Cargo.toml")})
        };
        let dep = |pkg: &str, kind: Option<&str>| json!({"pkg": pkg, "dep_kinds": [{"kind": kind, "target": null}]});
        json!({
            "packages": [
                pkg("root", "client_dll", Some("Apache-2.0"), None),
                pkg("a", "alpha", Some("MIT OR Apache-2.0"), Some("registry")),
                pkg("b", "beta", Some("GPL-3.0-only"), Some("registry")),
                pkg("c", "gamma", None, Some("registry")),
                pkg("d", "delta", Some("MIT"), Some("registry")),
                pkg("e", "epsilon", Some("Zlib"), Some("registry")),
            ],
            "resolve": {"nodes": [
                {"id": "root", "deps": [dep("a", None), dep("d", Some("dev")), dep("e", Some("build"))]},
                {"id": "a", "deps": [dep("b", None)]},
                {"id": "b", "deps": [dep("c", None)]},
                {"id": "c", "deps": []},
                {"id": "d", "deps": []},
                {"id": "e", "deps": []},
            ]}
        })
    }
    #[test]
    fn walks_shipped_edges_only_and_reports_every_policy_violation() {
        let mut inv = inventory(&[("t".into(), metadata())], &["client_dll"]).unwrap();
        let names: Vec<_> = inv.packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["alpha", "beta", "epsilon", "gamma"],
            "dev-only delta excluded"
        );
        assert!(!inv.passed);
        assert_eq!(inv.violations.len(), 2, "{:?}", inv.violations);
        assert!(
            inv.violations
                .iter()
                .any(|v| v.contains("beta") && v.contains("allowlist"))
        );
        assert!(
            inv.violations
                .iter()
                .any(|v| v.contains("gamma") && v.contains("no SPDX"))
        );
        assert_eq!(inv.packages[0].selected.as_deref(), Some("MIT"));
        let text = notice(&mut inv, "Kit");
        assert!(text.contains("== alpha 1.0.0 ==") && text.contains("Usada como: MIT"));
        assert_eq!(inv.missing_texts.len(), 4);
        assert!(!inv.legal_review);
        assert!(inventory(&[("t".into(), metadata())], &["missing"]).is_err());
        assert!(inventory(&[("t".into(), json!({}))], &["client_dll"]).is_err());
    }
}
