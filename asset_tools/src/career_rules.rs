//! Data-only career rules for the mod's single scenario (M6).
//!
//! Static reading of game.dll (profile ccb327c2, `+0xbc180..+0xbc2fc`): the
//! New Game list enumerates `/defs/scenarios/`, loads each file and looks up a
//! `def` named `scenario`; a file without one is skipped silently. The win
//! trigger is the scenario's `win:` field (`money:N`, or `none:0` as the free
//! mode uses). Overriding the other scenario files with a differently named
//! def therefore hides them without touching originals. Both behaviours are
//! static evidence only until confirmed in the game (G14).
//!
//! After filling the list the dialog presets its selection to the hard-coded
//! `free_mode.def` (`+0xbc31b`, string `+0x152608`), used by New Game unless an
//! item is clicked. Hiding that file crashed the first real test (03/11) with
//! an invalid heap free. The career is therefore written INTO `free_mode.def`
//! (`SLOT`), from the visible `become_millionaire.def` (`CAREER`), and every
//! other file, including `become_millionaire.def` itself, is hidden.
use crate::localization::{Mounts, def};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path};

pub const DIR: &str = "defs/scenarios/";
/// Source of the career fields in the original mounts.
pub const CAREER: &str = "defs/scenarios/become_millionaire.def";
/// File the New Game dialog selects by default; carries the career.
pub const SLOT: &str = "defs/scenarios/free_mode.def";
pub const PACKAGE_ID: &str = "career-rules";
/// Kept well inside a signed 32-bit amount; the engine's integer width is unproven.
pub const MAX_DOLLARS: u64 = 2_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Goal {
    Dollars { amount: u64 },
    Unlimited,
}
impl Goal {
    pub fn dollars(amount: u64) -> Result<Self> {
        ensure!(
            (1..=MAX_DOLLARS).contains(&amount),
            "goal must be 1..={MAX_DOLLARS} dollars"
        );
        Ok(Self::Dollars { amount })
    }
    fn win(self) -> String {
        match self {
            Self::Dollars { amount } => format!("money:{amount}"),
            Self::Unlimited => "none:0".into(),
        }
    }
}
/// Reads the goal a career manifest already fixed (`mp_career host`), so the
/// rules package cannot diverge from what participants were admitted with.
pub fn goal_from_manifest(json: &[u8]) -> Result<Goal> {
    ensure!(json.len() <= 64 * 1024, "career manifest limit");
    let v: serde_json::Value = serde_json::from_slice(json).context("invalid career manifest")?;
    ensure!(
        v["scenario"] == "become_millionaire",
        "career manifest is not the millionaire scenario"
    );
    match v["goal"]["kind"].as_str() {
        Some("unlimited") => Ok(Goal::Unlimited),
        Some("financial") => {
            let minor = v["goal"]["minor_units"]
                .as_u64()
                .context("financial goal without minor_units")?;
            ensure!(
                minor.is_multiple_of(100),
                "goal must be whole dollars for the game"
            );
            Goal::dollars(minor / 100)
        }
        _ => bail!("unknown career goal"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    En,
    PtBr,
}
fn grouped(n: u64, sep: char) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}
fn texts(goal: Goal, language: Language) -> (String, String) {
    match (goal, language) {
        (Goal::Dollars { amount }, Language::En) => (
            "Become a millionaire".into(),
            format!(
                "Repay the loan and reach $ {}. Be faster than the other players !",
                grouped(amount, ',')
            ),
        ),
        (Goal::Dollars { amount }, Language::PtBr) => (
            "Torne-se milionário".into(),
            format!(
                "Pague o empréstimo e chegue a $ {}. Seja mais rápido que os outros jogadores !",
                grouped(amount, '.')
            ),
        ),
        (Goal::Unlimited, Language::En) => (
            "Career without limit".into(),
            "No money goal: repay the loan and grow your company.".into(),
        ),
        (Goal::Unlimited, Language::PtBr) => (
            "Carreira sem limite".into(),
            "Sem meta financeira: pague o empréstimo e faça sua empresa crescer.".into(),
        ),
    }
}

fn field(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let key = t.split(':').next()?;
    (key.len() < t.len() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some(key)
}
/// Lines of the single `def scenario` block: (header index, closing index).
fn block(lines: &[&str]) -> Result<(usize, usize)> {
    let headers: Vec<usize> = (0..lines.len())
        .filter(|&i| {
            let t = lines[i].trim();
            t.starts_with("def ")
                && t[4..].trim_start().starts_with("scenario")
                && !t[4..].trim_start().starts_with("scenario_")
        })
        .collect();
    ensure!(headers.len() == 1, "expected exactly one def scenario");
    let start = headers[0];
    let end = (start + 1..lines.len())
        .find(|&i| lines[i].trim() == "}")
        .context("unclosed def scenario")?;
    Ok((start, end))
}
const REWRITTEN: [&str; 3] = ["win", "name", "instructions"];

/// Sets `win`, `name` and `instructions` of the scenario; every other byte of
/// the visible definition is kept. A missing `win:` line is inserted.
pub fn rewrite(original: &[u8], goal: Goal, language: Language) -> Result<Vec<u8>> {
    ensure!(original.len() <= 64 * 1024, "scenario definition limit");
    let text = def::decode(original);
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let (start, end) = block(&lines)?;
    let (name, instructions) = texts(goal, language);
    let values = BTreeMap::from([
        ("win", goal.win()),
        ("name", name),
        ("instructions", instructions),
    ]);
    let mut seen = BTreeMap::new();
    let mut out = Vec::with_capacity(original.len() + 128);
    for (i, line) in lines.iter().enumerate() {
        let key = (i > start && i < end).then(|| field(line)).flatten();
        if let Some(key) = key.filter(|k| REWRITTEN.contains(k)) {
            ensure!(seen.insert(key, ()).is_none(), "duplicate {key} field");
            let indent = &line[..line.len() - line.trim_start().len()];
            out.extend_from_slice(indent.as_bytes());
            out.extend_from_slice(format!("{key}:\"").as_bytes());
            out.extend_from_slice(&def::encode(&values[key])?);
            out.extend_from_slice(format!("\"{newline}").as_bytes());
            continue;
        }
        if i == end {
            for key in REWRITTEN.iter().filter(|k| !seen.contains_key(*k)) {
                out.extend_from_slice(format!("\t{key}:\"").as_bytes());
                out.extend_from_slice(&def::encode(&values[key])?);
                out.extend_from_slice(format!("\"{newline}").as_bytes());
            }
        }
        out.extend_from_slice(&line.chars().map(|c| c as u32 as u8).collect::<Vec<_>>());
    }
    Ok(out)
}

/// Replacement for every other scenario: a def with another name, so the menu
/// lookup of `scenario` fails and the file is skipped. One name per file.
pub fn disabled(path: &str) -> Result<Vec<u8>> {
    let stem = path
        .strip_prefix(DIR)
        .and_then(|s| s.strip_suffix(".def"))
        .context("not a scenario definition")?;
    ensure!(
        !stem.is_empty()
            && stem.len() <= 64
            && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "unsupported scenario file name"
    );
    Ok(format!(
        "# across-america-mp: the multiplayer career is offered through free_mode.def only.\n\
         def mp_disabled_{stem} {{\n\tnote:\"disabled\"\n}}\n"
    )
    .into_bytes())
}

/// Install-time guard: `SLOT` differs from the visible original `CAREER` only
/// in the three rewritten fields; every other file is exactly `disabled()`.
pub fn verify_entry(path: &str, career: Option<&[u8]>, bytes: &[u8]) -> Result<()> {
    if path != SLOT {
        ensure!(
            bytes == disabled(path)?,
            "{path}: not our disabled definition"
        );
        return Ok(());
    }
    let original = career.context("career scenario missing in original mounts")?;
    let strip = |b: &[u8]| -> Result<Vec<String>> {
        let text = def::decode(b);
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let (start, end) = block(&lines)?;
        Ok(lines
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                !(*i > start && *i < end && field(l).is_some_and(|k| REWRITTEN.contains(&k)))
            })
            .map(|(_, l)| l.trim_end().to_string())
            .collect())
    };
    ensure!(
        strip(original)? == strip(bytes)?,
        "career scenario changes more than win/name/instructions"
    );
    let text = def::decode(bytes);
    let win = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("win:"))
        .context("career scenario without win")?
        .trim()
        .trim_matches('"');
    match win.split_once(':') {
        Some(("none", "0")) => {}
        Some(("money", n)) => {
            Goal::dollars(n.parse().context("win amount")?)?;
        }
        _ => bail!("unsupported win condition {win}"),
    }
    Ok(())
}

#[derive(Serialize)]
pub struct Report {
    pub schema: u32,
    pub goal: Goal,
    pub language: Language,
    pub disabled: Vec<String>,
    pub files: usize,
    pub engine_validated: bool,
}
#[derive(Serialize)]
struct ProjectFile {
    source: String,
    path: String,
    origin: &'static str,
    depends_on: Vec<String>,
}
#[derive(Serialize)]
struct Project {
    schema: u32,
    id: &'static str,
    version: &'static str,
    files: Vec<ProjectFile>,
}

/// Writes a NEW derived_local project from the visible scenario definitions.
pub fn build(mounts: &Mounts, goal: Goal, language: Language, output: &Path) -> Result<Report> {
    ensure!(!output.exists(), "output must be a NEW directory");
    let names = mounts.names(DIR, ".def");
    ensure!(
        names.iter().any(|n| n == CAREER) && names.iter().any(|n| n == SLOT),
        "career or default scenario not found"
    );
    ensure!(names.len() <= 64, "too many scenario definitions");
    let career = mounts.read(CAREER)?;
    let mut outputs = BTreeMap::new();
    let mut disabled_list = vec![];
    for name in &names {
        let bytes = if name == SLOT {
            rewrite(&career, goal, language)?
        } else {
            disabled_list.push(name.clone());
            disabled(name)?
        };
        verify_entry(name, Some(&career), &bytes)?;
        outputs.insert(name.clone(), bytes);
    }
    fs::create_dir(output).context("creating output directory")?;
    let mut files = vec![];
    for (path, bytes) in &outputs {
        let source = format!("inputs/local/{path}");
        let file = output.join(&source);
        fs::create_dir_all(file.parent().context("output parent")?)?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .and_then(|mut f| std::io::Write::write_all(&mut f, bytes))?;
        files.push(ProjectFile {
            source,
            path: path.clone(),
            origin: "derived_local",
            depends_on: vec![],
        });
    }
    let project = Project {
        schema: 1,
        id: PACKAGE_ID,
        version: "0.1.0",
        files,
    };
    fs::write(
        output.join("project.json"),
        serde_json::to_vec_pretty(&project)?,
    )?;
    let report = Report {
        schema: 1,
        goal,
        language,
        disabled: disabled_list,
        files: outputs.len(),
        engine_validated: false,
    };
    fs::write(
        output.join("career-rules-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    const OWN: &[u8] = b"extdefs \"/defs/own_base.def\"\r\n\r\ndef scenario:/own_base {\r\n\tname:\"Own career\"\r\n\tinstructions:\"Own text\"\r\n\ttruck:\"own1\"\r\n\tmoney:\"3000\"\r\n\twin:\"money:1000000\"\r\n}\r\n";
    #[test]
    fn rewrite_changes_only_goal_and_texts_and_keeps_line_endings() {
        let out = rewrite(OWN, Goal::dollars(5000).unwrap(), Language::En).unwrap();
        let text = def::decode(&out);
        assert!(text.contains("\twin:\"money:5000\"\r\n"));
        assert!(text.contains("reach $ 5,000."));
        assert!(text.contains("\tmoney:\"3000\"\r\n") && text.contains("truck:\"own1\""));
        verify_entry(SLOT, Some(OWN), &out).unwrap();

        let pt = rewrite(OWN, Goal::Unlimited, Language::PtBr).unwrap();
        let text = def::decode(&pt);
        assert!(text.contains("win:\"none:0\"") && text.contains("Carreira sem limite"));
        assert!(
            pt.contains(&0xe7) && !pt.contains(&0xc3),
            "single-byte text"
        );
        verify_entry(SLOT, Some(OWN), &pt).unwrap();

        // A missing win line is inserted before the closing brace.
        let no_win = def::decode(OWN).replace("\twin:\"money:1000000\"\r\n", "");
        let out = rewrite(no_win.as_bytes(), Goal::Unlimited, Language::En).unwrap();
        assert!(def::decode(&out).ends_with("\twin:\"none:0\"\r\n}\r\n"));
    }
    #[test]
    fn guards_refuse_other_changes_and_unexpected_shapes() {
        let out = rewrite(OWN, Goal::dollars(5000).unwrap(), Language::En).unwrap();
        let tampered = def::decode(&out).replace("money:\"3000\"", "money:\"900000\"");
        assert!(verify_entry(SLOT, Some(OWN), tampered.as_bytes()).is_err());
        let bad_win = def::decode(&out).replace("money:5000", "delivered:own_city");
        assert!(verify_entry(SLOT, Some(OWN), bad_win.as_bytes()).is_err());
        assert!(rewrite(b"def other {\n}\n", Goal::Unlimited, Language::En).is_err());
        let twice = [OWN, OWN].concat();
        assert!(rewrite(&twice, Goal::Unlimited, Language::En).is_err());
        assert!(Goal::dollars(0).is_err() && Goal::dollars(MAX_DOLLARS + 1).is_err());

        let off = disabled("defs/scenarios/own_mode.def").unwrap();
        assert!(String::from_utf8_lossy(&off).contains("def mp_disabled_own_mode {"));
        verify_entry("defs/scenarios/own_mode.def", None, &off).unwrap();
        assert!(verify_entry("defs/scenarios/other.def", None, &off).is_err());
        assert!(disabled("defs/scenarios/../x.def").is_err());
    }
    #[test]
    fn manifest_goal_is_whole_dollars_of_the_millionaire_scenario() {
        let fin = br#"{"scenario":"become_millionaire","goal":{"kind":"financial","minor_units":500000}}"#;
        assert_eq!(
            goal_from_manifest(fin).unwrap(),
            Goal::Dollars { amount: 5000 }
        );
        let unl = br#"{"scenario":"become_millionaire","goal":{"kind":"unlimited"}}"#;
        assert_eq!(goal_from_manifest(unl).unwrap(), Goal::Unlimited);
        let cents =
            br#"{"scenario":"become_millionaire","goal":{"kind":"financial","minor_units":150}}"#;
        assert!(goal_from_manifest(cents).is_err());
        let other = br#"{"scenario":"free_mode","goal":{"kind":"unlimited"}}"#;
        assert!(goal_from_manifest(other).is_err());
    }
    #[test]
    fn build_hides_every_other_scenario_from_the_visible_mounts() {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().join("base.scs");
        let patch = t.path().join("patch.scs");
        let mut z = zip::ZipWriter::new(fs::File::create(&base).unwrap());
        for (name, data) in [
            (CAREER, OWN),
            (SLOT, b"def scenario:/own_base {\n}\n".as_slice()),
            ("defs/own_base.def", b"def own_base {\n}\n"),
        ] {
            z.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        let mut z = zip::ZipWriter::new(fs::File::create(&patch).unwrap());
        z.start_file(
            "defs/scenarios/own_fast.def",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        z.write_all(b"def scenario:/own_base {\n}\n").unwrap();
        z.finish().unwrap();
        let mounts = Mounts::open(&[base, patch]).unwrap();
        let out = t.path().join("project");
        let r = build(&mounts, Goal::dollars(5000).unwrap(), Language::PtBr, &out).unwrap();
        // The default selection carries the career; the original career file hides.
        assert_eq!(r.disabled, [CAREER, "defs/scenarios/own_fast.def"]);
        let slot = fs::read(out.join("inputs/local").join(SLOT)).unwrap();
        assert!(def::decode(&slot).contains("win:\"money:5000\""));
        assert_eq!(
            fs::read(out.join("inputs/local").join(CAREER)).unwrap(),
            disabled(CAREER).unwrap()
        );
        assert!(!r.engine_validated);
        let pkg = t.path().join("package");
        crate::mod_package::build(&out.join("project.json"), &pkg, false).unwrap();
        assert!(
            crate::mod_package::build(&out.join("project.json"), &t.path().join("d"), true)
                .is_err()
        );
        assert!(build(&mounts, Goal::Unlimited, Language::En, &out).is_err());
    }
}
