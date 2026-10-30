//! Own native UI projects, private translation inventory and reversible preview mounting.
//! No original assets are extracted into versioned sources. No game is launched.
pub mod inventory;
pub mod mount;
pub const PACKAGE_VERSION: &str = "0.2.0";
use crate::mod_package::{Input, Origin, Project};
use anyhow::{Result, ensure};
use mod_ui::{
    flow::{App, SCREENS},
    locale::Language,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};
pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let f = std::fs::File::open(path)?;
    ensure!(
        f.metadata()?.is_file() && f.metadata()?.len() <= limit as u64,
        "invalid file type/size"
    );
    let mut b = Vec::new();
    f.take(limit as u64 + 1).read_to_end(&mut b)?;
    ensure!(b.len() <= limit, "file expanded past limit");
    Ok(b)
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
pub fn project(output: &Path, main_preview: bool, language: Language) -> Result<()> {
    let output = crate::mod_package::checked(output)?;
    fs::create_dir(&output)?;
    let owned = output.join("inputs/owned");
    fs::create_dir_all(&owned)?;
    let mut files = Vec::new();
    let mut add = |path: String, bytes: String| -> Result<()> {
        let target = owned.join(&path);
        fs::create_dir_all(target.parent().unwrap())?;
        write_new(&target, bytes.as_bytes())?;
        files.push(Input {
            source: format!("inputs/owned/{path}"),
            path,
            origin: Origin::Owned,
            depends_on: vec![],
        });
        Ok(())
    };
    for &screen in SCREENS {
        add(
            format!("ui/mp/{}.esc", mod_ui::native::name(screen)),
            mod_ui::native::screen(&App::preview(screen, language)),
        )?;
    }
    if main_preview {
        add(
            "ui/mmain.esc".into(),
            mod_ui::native::main_preview(language),
        )?;
    }
    let p = Project {
        schema: 1,
        id: "mp-ui-foundation".into(),
        version: PACKAGE_VERSION.into(),
        files,
    };
    write_new(
        &output.join("project.json"),
        &serde_json::to_vec_pretty(&p)?,
    )?;
    write_new(&output.join("PREVIEW.txt"),b"Own experimental UI layouts. Main preview keeps legacy actions; Multiplayer is an inactive label. Other screens have NO engine handlers. Native fonts/accents unverified; ASCII fallback. No world/win-condition/network change.\n")?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_native_project_builds_without_commercial_inputs() {
        let t = tempfile::tempdir().unwrap();
        let project_dir = t.path().join("project");
        project(&project_dir, true, Language::PtBr).unwrap();
        let built = crate::mod_package::build(
            &project_dir.join("project.json"),
            &t.path().join("package"),
            true,
        )
        .unwrap();
        assert_eq!(built.manifest.files.len(), 10);
        assert!(built.manifest.distributable);
        assert!(
            built
                .manifest
                .files
                .iter()
                .all(|e| e.input.origin == Origin::Owned)
        );
        crate::mod_package::verify(&t.path().join("package"), true).unwrap();
    }
}
