//! Finds the Steam app of this installation from its own library manifest, so the
//! assistant can open the game the way the player would. Read-only and portable.
use std::path::Path;

/// `installdir` must equal the game folder name: another game's manifest in the
/// same library never matches. Returns None rather than guessing.
pub fn app_id(manifest: &str, folder: &str) -> Option<u32> {
    let field = |key: &str| {
        manifest.lines().find_map(|line| {
            let mut parts = line.split('"').skip(1).step_by(2);
            (parts.next()? == key).then(|| parts.next().map(str::to_string))?
        })
    };
    if !field("installdir")?.eq_ignore_ascii_case(folder) {
        return None;
    }
    field("appid")?.parse().ok().filter(|id| *id != 0)
}
/// `<library>/steamapps/common/<folder>`: manifests sit two levels above the game.
pub fn find_app_id(root: &Path) -> Option<u32> {
    let folder = root.file_name()?.to_str()?;
    let steamapps = root.parent()?.parent()?;
    for entry in std::fs::read_dir(steamapps).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
            continue;
        }
        if entry.metadata().ok()?.len() > 64 * 1024 {
            continue;
        }
        if let Some(id) = std::fs::read_to_string(entry.path())
            .ok()
            .and_then(|text| app_id(&text, folder))
        {
            return Some(id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    const OWN: &str = "\"AppState\"\n{\n\t\"appid\"\t\t\"4242\"\n\t\"name\"\t\t\"Own Game\"\n\t\"installdir\"\t\t\"Own Game\"\n}\n";
    #[test]
    fn manifest_must_name_this_folder() {
        assert_eq!(app_id(OWN, "Own Game"), Some(4242));
        assert_eq!(app_id(OWN, "own game"), Some(4242));
        assert_eq!(app_id(OWN, "Other Game"), None);
        assert_eq!(app_id("garbage", "Own Game"), None);
        assert_eq!(app_id(&OWN.replace("4242", "x"), "Own Game"), None);
        assert_eq!(app_id(&OWN.replace("4242", "0"), "Own Game"), None);
    }
    #[test]
    fn library_layout_is_searched_without_guessing() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("steamapps/common/Own Game");
        std::fs::create_dir_all(&root).unwrap();
        let apps = t.path().join("steamapps");
        assert_eq!(find_app_id(&root), None);
        std::fs::write(
            apps.join("appmanifest_1.acf"),
            OWN.replace("Own Game", "Other"),
        )
        .unwrap();
        assert_eq!(find_app_id(&root), None);
        std::fs::write(apps.join("appmanifest_4242.acf"), OWN).unwrap();
        assert_eq!(find_app_id(&root), Some(4242));
    }
}
