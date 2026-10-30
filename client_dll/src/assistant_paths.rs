//! Resolve user-selected folders without a shell or creating an empty saves folder.
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

const SUFFIX: &str = "18 WoS Across America/saves";
const MAX_INPUT: usize = 32767;

/// One expansion pass: an environment value containing % is never reinterpreted.
pub fn expand(input: &str, mut env: impl FnMut(&str) -> Option<OsString>) -> Result<PathBuf> {
    let input = input.trim();
    ensure!(
        input.chars().count() <= MAX_INPUT && !input.contains('\0'),
        "caminho muito longo ou com NUL"
    );
    let input = if input.starts_with('"') || input.ends_with('"') {
        ensure!(
            input.len() >= 2 && input.starts_with('"') && input.ends_with('"'),
            "aspas incompletas no caminho"
        );
        &input[1..input.len() - 1]
    } else {
        input
    };
    ensure!(!input.is_empty(), "informe a pasta que contem os saves");
    let mut result = OsString::new();
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        result.push(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('%') else {
            result.push(&rest[start..]);
            rest = "";
            break;
        };
        let name = &tail[..end];
        ensure!(
            !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "variavel de ambiente invalida no caminho"
        );
        let value = env(name).with_context(|| {
            format!("variavel %{name}% nao definida; cole o endereco completo do Explorador")
        })?;
        ensure!(!value.is_empty(), "variavel %{name}% vazia");
        result.push(value);
        rest = &tail[end + 1..];
    }
    result.push(rest);
    ensure!(
        result.len() <= MAX_INPUT * 4,
        "caminho expandido muito longo"
    );
    Ok(PathBuf::from(result))
}

pub fn directory(input: &str) -> Result<PathBuf> {
    let path = expand(input, |name| std::env::var_os(name))?;
    ensure!(
        path.is_absolute(),
        "use o caminho completo da pasta de saves: {}",
        path.display()
    );
    match fs::metadata(&path) {
        Ok(meta) if meta.is_dir() => path
            .canonicalize()
            .with_context(|| format!("nao foi possivel resolver {}", path.display())),
        Ok(_) => bail!(
            "o caminho e um arquivo: {}. Informe a PASTA que contem os .sav",
            path.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "pasta inexistente: {}. Copie o endereco da barra do Explorador; Documents pode estar no OneDrive",
            path.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            bail!("acesso negado a pasta: {}", path.display())
        }
        Err(e) => {
            Err(e).with_context(|| format!("nao foi possivel consultar a pasta {}", path.display()))
        }
    }
}

/// Prefer an already verified saved choice; then the OS Documents folder.
/// If none exists, display the OS candidate, without inventing or creating it.
pub fn default_saves(
    remembered: Option<&Path>,
    documents: Option<&Path>,
    profile: &Path,
) -> PathBuf {
    let known = documents.map(|p| p.join(SUFFIX));
    let legacy = profile.join("Documents").join(SUFFIX);
    remembered
        .filter(|p| p.is_absolute() && p.is_dir())
        .map(Path::to_path_buf)
        .or_else(|| known.as_ref().filter(|p| p.is_dir()).cloned())
        .or_else(|| legacy.is_dir().then_some(legacy.clone()))
        .or(known)
        .unwrap_or(legacy)
}

#[cfg(windows)]
pub fn documents() -> Result<PathBuf> {
    use std::{os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath},
    };
    let mut raw = ptr::null_mut();
    // SAFETY: documented system ABI; current user, default flags, valid output.
    // This respects redirected Documents, including OneDrive. No COM init needed.
    let hr = unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, 0, ptr::null_mut(), &mut raw) };
    struct Owned(*mut u16);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CoTaskMemFree(self.0.cast()) };
        }
    }
    let owned = Owned(raw);
    ensure!(
        hr >= 0 && !raw.is_null(),
        "Documents indisponivel (HRESULT {hr:#x})"
    );
    // SAFETY: on success Windows returns an allocated NUL-terminated UTF-16 path;
    // memory remains owned until after the copy and is freed with CoTaskMemFree.
    let mut len = 0;
    while unsafe { *owned.0.add(len) } != 0 {
        ensure!(len < MAX_INPUT, "Documents retornou caminho muito longo");
        len += 1;
    }
    Ok(PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(owned.0, len)
    })))
}

/// Validated preferences only; never stores identity/tokens or modifies saves.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub server: std::net::SocketAddr,
    pub room: String,
    pub saves: PathBuf,
}
impl Preferences {
    pub fn validate(&self) -> Result<()> {
        crate::lan_kit::Settings {
            server: self.server,
            room: self.room.clone(),
        }
        .validate()?;
        ensure!(self.saves.is_absolute(), "caminho salvo deve ser absoluto");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expands_quoted_unicode_environment_once_and_rejects_unknown_variables() {
        let path = expand("  \"%userprofile%/Documentos/saves\"  ", |n| {
            n.eq_ignore_ascii_case("USERPROFILE")
                .then(|| OsString::from("/tmp/João%literal%"))
        })
        .unwrap();
        assert_eq!(path, PathBuf::from("/tmp/João%literal%/Documentos/saves"));
        assert!(expand("%ABSENT%/saves", |_| None).is_err());
        assert!(expand("\"/tmp/saves", |_| None).is_err());
        assert!(expand("/tmp/\0", |_| None).is_err());
        assert_eq!(
            expand("/tmp/100%", |_| None).unwrap(),
            PathBuf::from("/tmp/100%")
        );
    }
    #[test]
    fn redirected_documents_overrides_legacy_and_valid_remembered_choice_wins() {
        let t = tempfile::tempdir().unwrap();
        let docs = t.path().join("OneDrive/Documentos");
        let saves = docs.join(SUFFIX);
        fs::create_dir_all(&saves).unwrap();
        assert_eq!(default_saves(None, Some(&docs), t.path()), saves);
        let custom = t.path().join("meus saves");
        fs::create_dir(&custom).unwrap();
        assert_eq!(default_saves(Some(&custom), Some(&docs), t.path()), custom);
        fs::remove_dir(&custom).unwrap();
        assert_eq!(default_saves(Some(&custom), Some(&docs), t.path()), saves);
    }
    #[test]
    fn missing_folder_is_not_created_and_file_has_a_distinct_error() {
        let t = tempfile::tempdir().unwrap();
        let missing = t.path().join("not-created");
        assert!(
            directory(missing.to_str().unwrap())
                .unwrap_err()
                .to_string()
                .contains("inexistente")
        );
        assert!(!missing.exists());
        let file = t.path().join("own-fixture.sav");
        fs::write(&file, b"own bytes").unwrap();
        assert!(
            directory(file.to_str().unwrap())
                .unwrap_err()
                .to_string()
                .contains("arquivo")
        );
        assert_eq!(
            directory(t.path().to_str().unwrap()).unwrap(),
            t.path().canonicalize().unwrap()
        );
    }
    #[cfg(windows)]
    #[test]
    fn system_documents_is_absolute_without_reading_game_data() {
        assert!(documents().unwrap().is_absolute());
    }
}
