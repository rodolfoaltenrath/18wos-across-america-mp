//! Local, allowlisted Windows client kit. No game files or user identities.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, net::SocketAddr, path::Path};

pub const PAYLOAD: [&str; 5] = [
    "client_dll.dll",
    "mp_launcher.exe",
    "mp_lan.exe",
    "mp_doctor.exe",
    "memory_tool.exe",
];
pub const CONFIG: &str = "mp-lan.json";
pub const MANIFEST: &str = "mp-kit-manifest.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub server: SocketAddr,
    pub room: String,
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.server.port() != 0 && !self.server.ip().is_unspecified(),
            "informe o IP do servidor e uma porta diferente de zero"
        );
        ensure!(
            multiplayer_server::session_protocol::room_valid(&self.room),
            "nome de sala invalido"
        );
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u8,
    /// Supplied by the builder; hashes identify the actual files independently.
    pub source_revision: String,
    pub target: String,
    pub game_profile: String,
    pub lan_tested: bool,
    pub files: Vec<Entry>,
}

fn binary(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::metadata(path).with_context(|| path.display().to_string())?;
    ensure!(
        meta.is_file() && meta.len() <= 64 * 1024 * 1024,
        "invalid binary size/type"
    );
    let bytes = fs::read(path)?;
    ensure!(
        bytes.get(..2) == Some(b"MZ"),
        "missing MZ: {}",
        path.display()
    );
    let offset = bytes.get(60..64).context("missing PE offset")?;
    let nt = u32::from_le_bytes(offset.try_into()?) as usize;
    let header = bytes
        .get(nt..nt.saturating_add(26))
        .context("missing PE header")?;
    ensure!(
        &header[..4] == b"PE\0\0" && header[4..6] == [0x4c, 1] && header[24..26] == [0x0b, 1],
        "expected Windows x86 PE32: {}",
        path.display()
    );
    let is_dll = u16::from_le_bytes(header[22..24].try_into()?) & 0x2000 != 0;
    ensure!(
        is_dll == (path.extension().is_some_and(|e| e == "dll")),
        "wrong DLL/EXE role"
    );
    Ok(bytes)
}
fn entry(name: &str, bytes: &[u8]) -> Entry {
    Entry {
        name: name.into(),
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}

pub fn verify(root: &Path) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(root.join(MANIFEST))?)?;
    ensure!(
        manifest.schema == 1
            && manifest.target == "i686-pc-windows-gnu"
            && manifest.game_profile == crate::profile::ID,
        "unsupported kit manifest"
    );
    ensure!(
        manifest.files.len() == PAYLOAD.len(),
        "incomplete kit manifest"
    );
    for name in PAYLOAD {
        let matches: Vec<_> = manifest.files.iter().filter(|e| e.name == name).collect();
        ensure!(matches.len() == 1, "missing/duplicate kit file: {name}");
        let actual = entry(name, &binary(&root.join(name))?);
        ensure!(
            matches[0].bytes == actual.bytes && matches[0].sha256 == actual.sha256,
            "kit file differs: {name}; recopy the entire kit"
        );
    }
    Ok(manifest)
}

pub fn create(
    build: &Path,
    output: &Path,
    revision: &str,
    settings: &Settings,
) -> Result<Manifest> {
    settings.validate()?;
    ensure!(
        !revision.is_empty() && revision.len() <= 128,
        "missing source revision"
    );
    // Validate all inputs BEFORE creating a new output directory. Never overwrite.
    let binaries = PAYLOAD
        .iter()
        .map(|name| Ok((*name, binary(&build.join(name))?)))
        .collect::<Result<Vec<_>>>()?;
    let manifest = Manifest {
        schema: 1,
        source_revision: revision.into(),
        target: "i686-pc-windows-gnu".into(),
        game_profile: crate::profile::ID.into(),
        lan_tested: false,
        files: binaries
            .iter()
            .map(|(name, bytes)| entry(name, bytes))
            .collect(),
    };
    fs::create_dir(output).context("kit output must be a NEW directory with an existing parent")?;
    for (name, bytes) in binaries {
        fs::write(output.join(name), bytes)?;
    }
    fs::write(output.join(CONFIG), serde_json::to_vec_pretty(settings)?)?;
    fs::write(output.join(MANIFEST), serde_json::to_vec_pretty(&manifest)?)?;
    fs::write(output.join("LICENSE.txt"), include_str!("../../LICENSE"))?;
    fs::write(output.join("LEIA-ME.txt"), instructions(settings))?;
    verify(output)
}

fn instructions(settings: &Settings) -> String {
    format!(
        "18 WOS ACROSS AMERICA - KIT LOCAL DE TESTE LAN\r\n\r\n\
1. Copie TODO o conteudo desta pasta para a raiz do jogo no Windows, ao lado\r\n\
   de prism3d.exe. Nao precisa copiar o repositorio nem instalar Rust/Proton.\r\n\
   Este kit contem somente arquivos proprios do mod; nao substitui originais.\r\n\
2. Abra o jogo pela Steam e carregue um save de teste.\r\n\
3. Abra mp_lan.exe com dois cliques. Se o jogo estiver como administrador,\r\n\
   use o mesmo nivel de permissao para o assistente.\r\n\
4. Escolha 1 (iniciar), confirme o IP/sala e a pasta de saves.\r\n\
   Servidor preparado na configuracao: {}\r\n\
   Sala: {}\r\n\
   O servidor precisa estar rodando no Fedora, sem --echo, UDP {}.\r\n\
5. Quando solicitado, volte ao jogo em ate 5 segundos e mantenha-o em foco.\r\n\
   O assistente usa o protocolo v2, offset zero e cria uma identidade local.\r\n\
   Nao copie a identidade entre PCs. Cada PC usa seu proprio save de teste.\r\n\
6. Para sair, volte ao assistente, escolha 3 (parar), retorne ao jogo durante\r\n\
   a limpeza e aguarde o resultado. SO DEPOIS feche o jogo normalmente.\r\n\
   Nao use Alt+F4/menu de saida durante a sessao deste primeiro teste.\r\n\
7. A opcao 2 salva status; 4 confere os hashes do ultimo backup, inclusive\r\n\
   depois que o jogo fechar. Se o assistente fechar, reabra para recuperar\r\n\
   a parada pendente. Nao substitua a DLL com o jogo/mod ainda aberto.\r\n\r\n\
Dados privados, backups e logs: %LOCALAPPDATA%\\18wos-mp\\<instalacao>\\\r\n\
Saves padrao: %USERPROFILE%\\Documents\\18 WoS Across America\\saves\r\n\
Se estiverem no OneDrive/outra pasta, informe o caminho real no assistente.\r\n\
Nao compartilhe a pasta de dados: ela contem identidade e copias de saves.\r\n\r\n\
O visual remoto ainda nao tem colisao, rodas/reboque ou economia sincronizada.\r\n\
A LAN entre dois PCs sera validada neste teste; hashes e bancada nao sao\r\n\
essa prova. A janela sem foco pode pausar render/envio de posicoes.\r\n\
Comecem na mesma cidade/local proximo; o kit nao sincroniza saves/teleporta.\r\n\r\n\
Diagnostico manual (terminal na raiz do jogo):\r\n\
  .\\mp_doctor.exe --game . --client-dir .\r\n\
  .\\memory_tool.exe processes\r\n\
O manifesto mp-kit-manifest.json identifica os cinco binarios por SHA-256.\r\n\
Ele verifica integridade local, nao e assinatura de publicacao.\r\n",
        settings.server,
        settings.room,
        settings.server.port()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixtures(root: &Path) {
        for name in PAYLOAD {
            let mut bytes = vec![0; 160];
            bytes[..2].copy_from_slice(b"MZ");
            bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
            bytes[64..68].copy_from_slice(b"PE\0\0");
            bytes[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
            bytes[88..90].copy_from_slice(&0x10bu16.to_le_bytes());
            if name.ends_with(".dll") {
                bytes[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
            }
            fs::write(root.join(name), bytes).unwrap();
        }
    }
    fn settings() -> Settings {
        Settings {
            server: "192.0.2.1:39000".parse().unwrap(),
            room: "lan".into(),
        }
    }
    #[test]
    fn rejects_incomplete_wrong_architecture_and_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("kit");
        assert!(create(temp.path(), &out, "test", &settings()).is_err());
        assert!(!out.exists());
        fixtures(temp.path());
        let bad = temp.path().join("mp_lan.exe");
        let mut bytes = fs::read(&bad).unwrap();
        bytes[68] = 0x64;
        fs::write(&bad, bytes).unwrap();
        assert!(create(temp.path(), &out, "test", &settings()).is_err());
        assert!(!out.exists());
        fixtures(temp.path());
        create(temp.path(), &out, "test", &settings()).unwrap();
        fs::write(out.join("preserve.txt"), "owned by user").unwrap();
        assert!(create(temp.path(), &out, "test", &settings()).is_err());
        assert_eq!(
            fs::read_to_string(out.join("preserve.txt")).unwrap(),
            "owned by user"
        );
    }
    #[test]
    fn detects_tampering_duplicate_and_traversal_entries() {
        let temp = tempfile::tempdir().unwrap();
        fixtures(temp.path());
        let out = temp.path().join("kit");
        let mut manifest = create(temp.path(), &out, "test", &settings()).unwrap();
        let original = fs::read(out.join("mp_lan.exe")).unwrap();
        let mut modified = original.clone();
        modified[150] = 1;
        fs::write(out.join("mp_lan.exe"), modified).unwrap();
        assert!(verify(&out).is_err());
        fs::write(out.join("mp_lan.exe"), original).unwrap();
        manifest.files[0].name = "../client_dll.dll".into();
        fs::write(out.join(MANIFEST), serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(verify(&out).is_err());
        manifest.files[0].name = "mp_lan.exe".into();
        fs::write(out.join(MANIFEST), serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(verify(&out).is_err());
    }
}
