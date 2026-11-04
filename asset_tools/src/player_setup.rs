//! Portable local reconstruction and reversible installation. Embedded catalogues
//! are authored text only; original game data is read on the recipient's PC.
use crate::{
    localization as l10n, mod_package as package,
    ui_tools::{self as ui, mount},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
pub mod session;

const TABLE: &[u8] = include_bytes!("../../translations/pt-BR/local_def.json");
const LITERALS: &[u8] = include_bytes!("../../translations/pt-BR/literal_def.json");
const INPUTS: [&str; 5] = [
    "aa/init.cfg",
    "aa/lib/game.dll",
    "data.scs",
    "aa/base.scs",
    "aa/patch_1_10.scs",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    Translation,
    Menu,
}
impl Component {
    pub fn name(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Menu => "menu",
        }
    }
    pub fn overlay(self) -> mount::Overlay {
        match self {
            Self::Translation => mount::Overlay::PtBrText,
            Self::Menu => mount::Overlay::UiPreview,
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Translation => "ptbr-text",
            Self::Menu => "mp-ui-foundation",
        }
    }
    fn kind(self) -> &'static str {
        match self {
            Self::Translation => "own-ptbr-text",
            Self::Menu => "own-ui-preview",
        }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    name: String,
    bytes: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: u8,
    component: Component,
    root: PathBuf,
    inputs: Vec<Input>,
    package_sha256: String,
    typeface_sha256: Option<String>,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: u8,
    pub component: Component,
    pub files: usize,
    pub translated_table: usize,
    pub translated_literals: usize,
    pub smooth_font: bool,
    pub locally_rebuilt: bool,
    pub installed: bool,
    pub engine_validated: bool,
}
fn fingerprint(root: &Path, name: &str) -> Result<Input> {
    let path = package::checked(&root.join(name))?;
    let mut file = File::open(&path)?;
    let before = file.metadata()?;
    ensure!(
        before.is_file() && before.len() <= 4 * 1024 * 1024 * 1024,
        "source type/size differs"
    );
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= before.len(), "source grew while reading");
        hash.update(&buffer[..n]);
    }
    ensure!(
        total == before.len() && file.metadata()?.len() == total,
        "source changed while reading"
    );
    Ok(Input {
        name: name.into(),
        bytes: total,
        sha256: format!("{:x}", hash.finalize()),
    })
}
fn inputs(root: &Path) -> Result<Vec<Input>> {
    INPUTS.iter().map(|name| fingerprint(root, name)).collect()
}
fn generate(
    sources: &[PathBuf],
    component: Component,
    face: Option<&l10n::typeface::Typeface>,
    project: &Path,
) -> Result<(usize, usize)> {
    match component {
        Component::Translation => {
            let mounts = l10n::Mounts::open(sources)?;
            let table = l10n::catalog::load(TABLE)?;
            let literals = l10n::literals::load(LITERALS)?;
            let report =
                l10n::build_with_literals(&mounts, &table, project, face, Some(&literals))?;
            Ok((
                report.catalog.translated,
                report.literal_def.as_ref().map_or(0, |r| r.translated),
            ))
        }
        Component::Menu => {
            ui::project(project, true, mod_ui::locale::Language::PtBr)?;
            Ok((0, 0))
        }
    }
}
pub fn prepare(
    game: &Path,
    component: Component,
    typeface: Option<&Path>,
    output: &Path,
) -> Result<Report> {
    let (root, sources) = mount::setup_sources(game, component.overlay())?;
    let face = typeface.map(l10n::typeface::Typeface::load).transpose()?;
    ensure!(
        component == Component::Translation || face.is_none(),
        "menu does not replace fonts"
    );
    prepare_with(&root, &sources, component, face.as_ref(), output, generate)
}
fn prepare_with(
    root: &Path,
    sources: &[PathBuf],
    component: Component,
    face: Option<&l10n::typeface::Typeface>,
    output: &Path,
    generate: impl FnOnce(
        &[PathBuf],
        Component,
        Option<&l10n::typeface::Typeface>,
        &Path,
    ) -> Result<(usize, usize)>,
) -> Result<Report> {
    let output = package::checked(output)?;
    ensure!(!output.try_exists()?, "preparation directory must be NEW");
    let before = inputs(root)?;
    fs::create_dir(&output)?;
    ui::write_new(
        &output.join(".INCOMPLETE"),
        b"Local preparation is incomplete; do not install.\n",
    )?;
    let (table, literals) = generate(sources, component, face, &output.join("project"))?;
    let built = package::build(
        &output.join("project/project.json"),
        &output.join("package"),
        component == Component::Menu,
    )?;
    ensure!(
        built.manifest.id == component.id(),
        "generated component differs"
    );
    ensure!(
        inputs(root)? == before,
        "source changed during preparation; output remains incomplete"
    );
    let plan = Plan {
        schema: 1,
        component,
        root: root.to_owned(),
        inputs: before,
        package_sha256: built.manifest.payload_sha256.clone(),
        typeface_sha256: face.map(|f| f.sha256.clone()),
    };
    ui::write_new(
        &output.join("prepared.json"),
        &serde_json::to_vec_pretty(&plan)?,
    )?;
    fs::remove_file(output.join(".INCOMPLETE"))?;
    Ok(Report {
        schema: 1,
        component,
        files: built.manifest.files.len(),
        translated_table: table,
        translated_literals: literals,
        smooth_font: face.is_some(),
        locally_rebuilt: true,
        installed: false,
        engine_validated: false,
    })
}
fn load(game: &Path, work: &Path) -> Result<(PathBuf, Plan)> {
    let root = package::checked(game)?.canonicalize()?;
    let work = package::checked(work)?;
    ensure!(
        !work.join(".INCOMPLETE").try_exists()?,
        "preparation is incomplete"
    );
    let bytes = ui::read(&package::checked(&work.join("prepared.json"))?, 16 * 1024)?;
    let plan: Plan = serde_json::from_slice(&bytes)?;
    ensure!(
        plan.schema == 1
            && package::checked(&plan.root)?.canonicalize()? == root
            && plan.inputs.len() == INPUTS.len()
            && plan
                .inputs
                .iter()
                .zip(INPUTS)
                .all(|(input, name)| input.name == name
                    && multiplayer_server::career::hash_valid(&input.sha256)
                    && input.bytes <= 4 * 1024 * 1024 * 1024)
            && multiplayer_server::career::hash_valid(&plan.package_sha256)
            && plan
                .typeface_sha256
                .as_ref()
                .is_none_or(|h| multiplayer_server::career::hash_valid(h))
            && (plan.component == Component::Translation || plan.typeface_sha256.is_none()),
        "invalid prepared context"
    );
    Ok((work, plan))
}
pub fn check_prepared(game: &Path, work: &Path) -> Result<Component> {
    let (work, plan) = load(game, work)?;
    ensure!(
        inputs(&plan.root)? == plan.inputs,
        "game inputs changed since preparation"
    );
    let manifest = package::verify(&work.join("package"), plan.component == Component::Menu)?;
    ensure!(
        manifest.id == plan.component.id() && manifest.payload_sha256 == plan.package_sha256,
        "prepared package differs"
    );
    Ok(plan.component)
}
pub fn install(game: &Path, work: &Path) -> Result<mount::Status> {
    mount::ensure_game_closed()?;
    let component = check_prepared(game, work)?;
    mount::install_overlay(
        game,
        &work.join("package"),
        &work.join("backup"),
        component.overlay(),
    )
}
pub fn restore(game: &Path, work: &Path) -> Result<mount::Status> {
    let (work, plan) = load(game, work)?;
    ensure!(
        mount::status(game, &work.join("backup"))?.kind == plan.component.kind(),
        "backup component differs from preparation"
    );
    mount::restore(game, &work.join("backup"))
}
pub fn status(game: &Path, work: &Path) -> Result<Option<mount::Status>> {
    let (work, plan) = load(game, work)?;
    if work.join("backup").try_exists()? {
        let status = mount::status(game, &work.join("backup"))?;
        ensure!(
            status.kind == plan.component.kind(),
            "backup component differs from preparation"
        );
        Ok(Some(status))
    } else {
        check_prepared(game, &work).context("prepared input/package check failed")?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
