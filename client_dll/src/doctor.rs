//! Static installation checks. Never opens a process, maps a DLL, executes a
//! dependency, installs a runtime, changes game files or reads saves/identities.
mod binary;
use crate::profile;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILE: u64 = 64 * 1024 * 1024;
const EXPORTS: &[&str] = &[
    "client_start",
    "client_stop",
    "client_control",
    "client_engine_control",
    "client_get_remote",
];

pub struct Options {
    pub game: PathBuf,
    pub client_dir: PathBuf,
    pub native_dir: PathBuf,
    pub check_server: bool,
    pub proton_prefix: Option<PathBuf>,
    pub include_paths: bool,
}
#[derive(Debug, Serialize)]
pub struct FileCheck {
    pub role: &'static str,
    pub name: String,
    pub passed: bool,
    pub bytes: Option<u64>,
    pub sha256: Option<String>,
    pub expected_sha256: Option<String>,
    pub format: Option<&'static str>,
    pub machine: Option<&'static str>,
    pub checks: Vec<&'static str>,
    pub issues: Vec<String>,
}
#[derive(Serialize)]
pub struct Dependency {
    pub role: &'static str,
    pub name: String,
    pub observation: &'static str,
    pub location_group: Option<&'static str>,
}
#[derive(Serialize)]
pub struct PrefixCheck {
    pub structure_present: bool,
    pub missing: Vec<&'static str>,
    pub runtime_tested: bool,
}
#[derive(Serialize)]
pub struct LocalPaths {
    pub game: String,
    pub client_dir: String,
    pub native_dir: String,
    pub proton_prefix: Option<String>,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: u8,
    pub tool_version: &'static str,
    pub host_os: &'static str,
    pub doctor_target_arch: &'static str,
    pub profile: &'static str,
    pub read_only: bool,
    pub static_checks_passed: bool,
    pub game_tested: bool,
    pub runtime_tested: bool,
    pub lan_tested: bool,
    pub pair_build_compatibility_verified: bool,
    pub world_assets_compatibility_verified: bool,
    pub dependency_resolution_verified: bool,
    pub files: Vec<FileCheck>,
    pub dependencies: Vec<Dependency>,
    pub proton_prefix: Option<PrefixCheck>,
    pub limitations: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_paths: Option<LocalPaths>,
}

struct Spec<'a> {
    name: &'a str,
    role: &'static str,
    sha: Option<&'a str>,
    native: bool,
    exports: bool,
}

fn inspect_file(root: &Path, spec: Spec<'_>) -> (FileCheck, Vec<String>) {
    let mut check = FileCheck {
        role: spec.role,
        name: spec.name.into(),
        passed: false,
        bytes: None,
        sha256: None,
        expected_sha256: spec.sha.map(str::to_owned),
        format: None,
        machine: None,
        checks: Vec::new(),
        issues: Vec::new(),
    };
    let result = (|| -> Result<binary::Binary, String> {
        let path = root.join(spec.name);
        let meta = fs::metadata(&path).map_err(|e| format!("metadata_{:?}", e.kind()))?;
        check.bytes = Some(meta.len());
        if !meta.is_file() || meta.len() > MAX_FILE {
            return Err("requires_regular_file_at_most_64_mib".into());
        }
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|e| format!("open_{:?}", e.kind()))?
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read_{:?}", e.kind()))?;
        if bytes.len() as u64 != meta.len() || bytes.len() as u64 > MAX_FILE {
            return Err("size_changed_or_limit_exceeded".into());
        }
        check.checks.push("bounded_regular_file");
        let sha = format!("{:x}", Sha256::digest(&bytes));
        if spec.sha.is_some_and(|expected| sha != expected) {
            check.issues.push("profile_sha256_mismatch".into());
        }
        check.sha256 = Some(sha);
        let info = binary::inspect(&bytes, spec.exports).map_err(str::to_owned)?;
        check.format = Some(info.format);
        check.machine = Some(info.machine);
        if spec.native {
            let format = if cfg!(windows) {
                info.format.starts_with("pe")
            } else if cfg!(target_os = "linux") {
                info.format == "elf"
            } else {
                false
            };
            if !format || info.machine != std::env::consts::ARCH {
                check
                    .issues
                    .push("native_binary_does_not_match_doctor_target".into());
            }
            check
                .checks
                .push("native_os_and_doctor_target_architecture");
        } else {
            if info.format != "pe32" || info.machine != "x86" {
                check.issues.push("requires_windows_pe32_x86".into());
            }
            check.checks.push("windows_pe32_x86");
        }
        if spec.sha.is_some() {
            check.checks.push("profile_sha256");
        }
        if spec.exports {
            for name in EXPORTS {
                if !info.exports.contains(*name) {
                    check.issues.push(format!("missing_export_{name}"));
                }
            }
            check.checks.push("required_export_names_not_live_abi");
        }
        Ok(info)
    })();
    match result {
        Ok(binary) => {
            check.passed = check.issues.is_empty();
            (check, binary.imports)
        }
        Err(reason) => {
            check.issues.push(reason);
            (check, Vec::new())
        }
    }
}

fn prefix(path: &Path) -> PrefixCheck {
    let mut missing = Vec::new();
    if !path.join("drive_c").is_dir() {
        missing.push("drive_c");
    }
    for name in ["system.reg", "user.reg"] {
        if !path.join(name).is_file() {
            missing.push(name);
        }
    }
    PrefixCheck {
        structure_present: missing.is_empty(),
        missing,
        runtime_tested: false,
    }
}

fn index(directories: &[(PathBuf, &'static str)]) -> BTreeMap<String, &'static str> {
    let mut found = BTreeMap::new();
    for (root, group) in directories {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.take(8192).flatten() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                // Metadata only, including links used by Proton's builtin DLLs.
                if entry.path().is_file() {
                    found.entry(name).or_insert(*group);
                }
            }
        }
    }
    found
}

fn dependencies(
    options: &Options,
    files: Vec<(&'static str, bool, Vec<String>)>,
) -> Vec<Dependency> {
    let mut base = vec![
        (options.game.clone(), "game"),
        (options.game.join("aa/lib"), "game"),
        (options.game.join("lib"), "game"),
        (options.client_dir.clone(), "client"),
    ];
    let mut native = base.clone();
    native.push((options.native_dir.clone(), "native"));
    if cfg!(windows) {
        if let Some(windir) = std::env::var_os("WINDIR") {
            let windir = PathBuf::from(windir);
            let wow = windir.join("SysWOW64");
            base.push((
                if wow.is_dir() {
                    wow
                } else {
                    windir.join("System32")
                },
                "system",
            ));
            native.push((windir.join("System32"), "system"));
        }
    } else if let Some(root) = &options.proton_prefix {
        base.push((root.join("drive_c/windows/syswow64"), "prefix"));
        base.push((root.join("drive_c/windows/system32"), "prefix"));
    }
    let (base, native) = (index(&base), index(&native));
    let mut output = Vec::new();
    for (role, is_native, names) in files {
        for name in names {
            let group = if is_native { &native } else { &base }.get(&name).copied();
            let contract = name.starts_with("api-ms-") || name.starts_with("ext-ms-");
            output.push(Dependency {
                role,
                name,
                location_group: group,
                observation: if contract {
                    "loader_contract_not_a_disk_presence_test"
                } else if group.is_some() {
                    "present_in_known_location"
                } else {
                    "not_found_in_bounded_known_locations_not_a_loader_verdict"
                },
            });
        }
    }
    output
}

pub fn inspect(options: &Options) -> Report {
    let specs = [
        ("prism3d.exe", profile::EXECUTABLE_SHA256),
        ("aa/lib/game.dll", profile::SHA256),
        ("p3shared.dll", profile::ENGINE_SHA256),
        ("gfxshared.dll", profile::GFX_SHA256),
        ("lib/psm.dll", profile::PSM_SHA256),
    ];
    let (mut files, mut imports) = (Vec::new(), Vec::new());
    for (name, sha) in specs {
        let (check, names) = inspect_file(
            &options.game,
            Spec {
                name,
                role: "game",
                sha: Some(sha),
                native: false,
                exports: false,
            },
        );
        files.push(check);
        imports.push(("game", false, names));
    }
    for (name, exports) in [("client_dll.dll", true), ("mp_launcher.exe", false)] {
        let (check, names) = inspect_file(
            &options.client_dir,
            Spec {
                name,
                role: "client",
                sha: None,
                native: false,
                exports,
            },
        );
        files.push(check);
        imports.push(("client", false, names));
    }
    if options.check_server {
        let name = if cfg!(windows) {
            "multiplayer_server.exe"
        } else {
            "multiplayer_server"
        };
        let (check, names) = inspect_file(
            &options.native_dir,
            Spec {
                name,
                role: "native_server",
                sha: None,
                native: true,
                exports: false,
            },
        );
        files.push(check);
        imports.push(("native_server", true, names));
    }
    let proton_prefix = options.proton_prefix.as_deref().map(prefix);
    Report {
        schema: 1,
        tool_version: env!("CARGO_PKG_VERSION"),
        host_os: std::env::consts::OS,
        doctor_target_arch: std::env::consts::ARCH,
        profile: profile::ID,
        read_only: true,
        static_checks_passed: files.iter().all(|f| f.passed)
            && proton_prefix.as_ref().is_none_or(|p| p.structure_present),
        game_tested: false,
        runtime_tested: false,
        lan_tested: false,
        pair_build_compatibility_verified: false,
        world_assets_compatibility_verified: false,
        dependency_resolution_verified: false,
        files,
        dependencies: dependencies(options, imports),
        proton_prefix,
        limitations: vec![
            "static_metadata_only_no_process_or_loader_calls",
            "required_export_names_do_not_prove_live_abi_or_matching_builds",
            "dll_presence_is_not_loader_resolution_sxs_and_wine_builtins_can_differ",
            "elf_header_only_no_native_shared_library_resolution",
            "directx_dotnet_graphics_and_proton_runtime_not_exercised",
            "game_archives_map_mods_saves_and_identity_files_not_read",
        ],
        local_paths: options.include_paths.then(|| LocalPaths {
            game: options.game.display().to_string(),
            client_dir: options.client_dir.display().to_string(),
            native_dir: options.native_dir.display().to_string(),
            proton_prefix: options
                .proton_prefix
                .as_ref()
                .map(|p| p.display().to_string()),
        }),
    }
}

/// Limited discovery: explicit path, AA_GAME_DIR, then current-directory
/// ancestors. Never guesses a different install or scans the entire Steam tree.
pub fn locate_game(explicit: Option<PathBuf>, current: &Path) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(path) = std::env::var_os("AA_GAME_DIR") {
        return path.into();
    }
    current
        .ancestors()
        .take(8)
        .find(|p| p.join("prism3d.exe").is_file())
        .unwrap_or(current)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_profile_and_exports_are_checked_without_loading_and_changes_are_detected() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = binary::fixture(EXPORTS);
        fs::write(temp.path().join("own.dll"), &bytes).unwrap();
        let sha = format!("{:x}", Sha256::digest(&bytes));
        let spec = || Spec {
            name: "own.dll",
            role: "fixture",
            sha: Some(&sha),
            native: false,
            exports: true,
        };
        assert!(inspect_file(temp.path(), spec()).0.passed);
        let mut changed = bytes;
        changed[100] ^= 1;
        fs::write(temp.path().join("own.dll"), changed).unwrap();
        assert!(
            inspect_file(temp.path(), spec())
                .0
                .issues
                .iter()
                .any(|s| s == "profile_sha256_mismatch")
        );
        fs::write(
            temp.path().join("own.dll"),
            binary::fixture(&["client_start"]),
        )
        .unwrap();
        assert!(
            inspect_file(temp.path(), spec())
                .0
                .issues
                .iter()
                .any(|s| s == "missing_export_client_control")
        );
    }
    #[test]
    fn missing_nonregular_and_oversized_files_are_reported_before_reading() {
        let temp = tempfile::tempdir().unwrap();
        let spec = || Spec {
            name: "own.dll",
            role: "fixture",
            sha: None,
            native: false,
            exports: false,
        };
        assert!(!inspect_file(temp.path(), spec()).0.passed);
        fs::create_dir(temp.path().join("own.dll")).unwrap();
        assert!(!inspect_file(temp.path(), spec()).0.passed);
        fs::remove_dir(temp.path().join("own.dll")).unwrap();
        File::create(temp.path().join("own.dll"))
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        assert!(inspect_file(temp.path(), spec()).0.sha256.is_none());
    }
    #[test]
    fn reports_are_redacted_by_default_and_do_not_claim_runtime_readiness() {
        let temp = tempfile::tempdir().unwrap();
        let mut options = Options {
            game: temp.path().to_owned(),
            client_dir: temp.path().to_owned(),
            native_dir: temp.path().to_owned(),
            check_server: true,
            proton_prefix: Some(temp.path().to_owned()),
            include_paths: false,
        };
        let report = inspect(&options);
        assert!(
            !report.static_checks_passed
                && !report.runtime_tested
                && !report.pair_build_compatibility_verified
        );
        let json = serde_json::to_string(&report).unwrap();
        assert!(
            !json.contains(&temp.path().to_string_lossy().to_string())
                && !json.contains("local_paths")
        );
        assert!(!report.proton_prefix.unwrap().structure_present);
        fs::create_dir(temp.path().join("drive_c")).unwrap();
        fs::write(temp.path().join("user.reg"), b"own fixture").unwrap();
        fs::write(temp.path().join("system.reg"), b"own fixture").unwrap();
        assert!(prefix(temp.path()).structure_present);
        options.include_paths = true;
        assert!(inspect(&options).local_paths.is_some());
    }
    #[test]
    fn import_presence_is_informational_and_api_sets_are_not_missing_disk_files() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("OWN.DLL"), b"not executed").unwrap();
        let options = Options {
            game: temp.path().to_owned(),
            client_dir: temp.path().to_owned(),
            native_dir: temp.path().to_owned(),
            check_server: false,
            proton_prefix: None,
            include_paths: false,
        };
        let found = dependencies(
            &options,
            vec![(
                "fixture",
                false,
                vec![
                    "own.dll".into(),
                    "api-ms-fixture.dll".into(),
                    "definitely-absent-fixture.dll".into(),
                ],
            )],
        );
        assert_eq!(found[0].observation, "present_in_known_location");
        assert!(found[1].observation.starts_with("loader_contract"));
        assert!(found[2].observation.starts_with("not_found"));
        assert_eq!(
            fs::read(temp.path().join("OWN.DLL")).unwrap(),
            b"not executed"
        );
    }
}
