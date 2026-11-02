//! Local, allowlisted Windows client kit. No game files or user identities.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, net::SocketAddr, path::Path};

const LEGACY_PAYLOAD: [&str; 5] = [
    "client_dll.dll",
    "mp_launcher.exe",
    "mp_lan.exe",
    "mp_doctor.exe",
    "memory_tool.exe",
];
pub const PAYLOAD: [&str; 6] = [
    "client_dll.dll",
    "mp_launcher.exe",
    "mp_lan.exe",
    "mp_doctor.exe",
    "memory_tool.exe",
    "mp_career.exe",
];
pub const CONFIG: &str = "mp-lan.json";
pub const MANIFEST: &str = "mp-kit-manifest.json";
pub const NOTICES: &str = "THIRD_PARTY.txt";
const MAX_BINARY: usize = 64 * 1024 * 1024;
const MAX_NOTICE: usize = 4 * 1024 * 1024;
const MAX_MANIFEST: usize = 64 * 1024;

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
    /// Optional third-party notice (`THIRD_PARTY.txt`), hashed like binaries.
    /// Absent in older kits; older verifiers ignore the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notices: Option<Entry>,
}

fn binary(path: &Path) -> Result<Vec<u8>> {
    let bytes = crate::local_files::read(path, MAX_BINARY)?;
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
    let manifest: Manifest = serde_json::from_slice(&crate::local_files::read(
        &root.join(MANIFEST),
        MAX_MANIFEST,
    )?)?;
    ensure!(
        matches!(manifest.schema, 1 | 2)
            && manifest.target == "i686-pc-windows-gnu"
            && manifest.game_profile == crate::profile::ID,
        "unsupported kit manifest"
    );
    revision(&manifest.source_revision)?;
    let payload: &[&str] = if manifest.schema == 1 {
        &LEGACY_PAYLOAD
    } else {
        &PAYLOAD
    };
    ensure!(
        manifest.files.len() == payload.len(),
        "incomplete kit manifest"
    );
    for &name in payload {
        let matches: Vec<_> = manifest.files.iter().filter(|e| e.name == name).collect();
        ensure!(matches.len() == 1, "missing/duplicate kit file: {name}");
        ensure!(
            matches[0].bytes <= MAX_BINARY as u64 && hash_valid(&matches[0].sha256),
            "invalid kit size/hash"
        );
        let actual = entry(name, &binary(&root.join(name))?);
        ensure!(
            matches[0].bytes == actual.bytes && matches[0].sha256 == actual.sha256,
            "kit file differs: {name}; recopy the entire kit"
        );
    }
    if let Some(expected) = &manifest.notices {
        ensure!(expected.name == NOTICES, "unexpected notice file name");
        ensure!(
            expected.bytes <= MAX_NOTICE as u64 && hash_valid(&expected.sha256),
            "invalid notice size/hash"
        );
        let bytes = crate::local_files::read(&root.join(NOTICES), MAX_NOTICE)
            .context("third-party notice missing/oversized")?;
        ensure!(std::str::from_utf8(&bytes).is_ok(), "notice must be UTF-8");
        let actual = entry(NOTICES, &bytes);
        ensure!(
            expected.bytes == actual.bytes && expected.sha256 == actual.sha256,
            "third-party notice differs; recopy the entire kit"
        );
    }
    settings(root)?;
    Ok(manifest)
}
fn hash_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn revision(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" ._-+".contains(&b)),
        "invalid source revision"
    );
    Ok(())
}
pub fn settings(root: &Path) -> Result<Settings> {
    let result: Settings =
        serde_json::from_slice(&crate::local_files::read(&root.join(CONFIG), 4096)?)?;
    result.validate()?;
    Ok(result)
}

/// `notices`: optional text produced by `license_audit --notice` (NEW file).
pub fn create(
    build: &Path,
    output: &Path,
    revision: &str,
    settings: &Settings,
    notices: Option<&Path>,
) -> Result<Manifest> {
    settings.validate()?;
    self::revision(revision)?;
    // Validate all inputs BEFORE creating a new output directory. Never overwrite.
    let binaries = PAYLOAD
        .iter()
        .map(|name| Ok((*name, binary(&build.join(name))?)))
        .collect::<Result<Vec<_>>>()?;
    let notice = notices
        .map(|path| -> Result<Vec<u8>> {
            let bytes = crate::local_files::read(path, MAX_NOTICE).context("third-party notice")?;
            ensure!(
                bytes.len() <= 4 * 1024 * 1024 && std::str::from_utf8(&bytes).is_ok(),
                "notice must be UTF-8 text up to 4 MiB"
            );
            Ok(bytes)
        })
        .transpose()?;
    let manifest = Manifest {
        schema: 2,
        source_revision: revision.into(),
        target: "i686-pc-windows-gnu".into(),
        game_profile: crate::profile::ID.into(),
        lan_tested: false,
        files: binaries
            .iter()
            .map(|(name, bytes)| entry(name, bytes))
            .collect(),
        notices: notice.as_deref().map(|b| entry(NOTICES, b)),
    };
    fs::create_dir(output).context("kit output must be a NEW directory with an existing parent")?;
    for (name, bytes) in binaries {
        fs::write(output.join(name), bytes)?;
    }
    fs::write(output.join(CONFIG), serde_json::to_vec_pretty(settings)?)?;
    fs::write(output.join(MANIFEST), serde_json::to_vec_pretty(&manifest)?)?;
    fs::write(output.join("LICENSE.txt"), include_str!("../../LICENSE"))?;
    fs::write(output.join("LEIA-ME.txt"), instructions(settings))?;
    fs::write(output.join("CARREIRA.txt"), career_instructions())?;
    if let Some(bytes) = &notice {
        fs::write(output.join(NOTICES), bytes)?;
    }
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
   a parada pendente. Nao substitua a DLL com o jogo/mod ainda aberto.\r\n\
8. Opcao 5: escolha captura e movimento para a PROXIMA conexao. O padrao\r\n\
   continua o anterior; mudar as escolhas nao altera uma sessao ativa.\r\n\
   Para a rodada de aparencia, escolha Guardar aparencia = 1 (Sim).\r\n\
   A coleta e optativa e nao altera o desenho ou a pintura do caminhao remoto.\r\n\
9. Opcao 6: prepare o diagnostico para compartilhar, inclusive apos parar.\r\n\
   Envie SOMENTE a pasta nova indicada: ela tem relatorios filtrados, analise\r\n\
   e manifesto. Nao envie a pasta de dados inteira, saves ou backups.\r\n\
   Confira a pasta com mp_doctor.exe --diagnostics PASTA-INDICADA.\r\n\r\n\
10. Se ativou Guardar aparencia antes de iniciar, a opcao 7 prepara OUTRA\r\n\
    pasta para compartilhar, com identificadores candidatos e recusas de leitura.\r\n\
    A coleta ocorre ao iniciar, consultar e parar; nao exige repetir movimentos.\r\n\
    Confira com mp_doctor.exe --appearance-evidence PASTA-INDICADA.\r\n\
    Envie as pastas indicadas por 6 e 7; nao a pasta de dados inteira.\r\n\r\n\
Dados privados, backups e logs: %LOCALAPPDATA%\\18wos-mp\\<instalacao>\\\r\n\
Saves padrao: %USERPROFILE%\\Documents\\18 WoS Across America\\saves\r\n\
Se estiverem no OneDrive/outra pasta, informe o caminho real no assistente.\r\n\
Nao compartilhe a pasta de dados: ela contem identidade e copias de saves.\r\n\r\n\
O visual remoto ainda nao tem colisao, rodas/reboque ou economia sincronizada.\r\n\
A LAN entre dois PCs passou numa revisao anterior; este kit atualizado ainda\r\n\
aguarda a rodada agrupada. A janela sem foco pode pausar render/envio.\r\n\
Comecem na mesma cidade/local proximo; o kit nao sincroniza saves/teleporta.\r\n\r\n\
Diagnostico manual (terminal na raiz do jogo):\r\n\
  .\\mp_doctor.exe --game . --client-dir .\r\n\
  .\\mp_lan.exe --check-runtime\r\n\
  .\\memory_tool.exe processes\r\n\
--check-runtime executa o launcher e carrega/consulta/libera a DLL apenas\r\n\
num processo proprio novo. Nao abre jogo, conecta rede ou acessa saves.\r\n\
Nao exige Rust e nao testa DirectX, graficos, firewall ou a partida.\r\n\
A opcao 1 tambem faz essa checagem antes de iniciar uma sessao nova.\r\n\
--check-only e mp_doctor continuam somente estaticos, sem carregar a DLL.\r\n\
O manifesto schema 2 identifica seis binarios por SHA-256, inclusive mp_career.\r\n\
Ele verifica integridade local, nao e assinatura de publicacao.\r\n\
Para o modo de carreira por arquivo, leia CARREIRA.txt.\r\n\
\r\nColeta opcional para a rodada agrupada (sessao ja iniciada):\r\n\
  .\\mp_launcher.exe --pid PID --dll .\\client_dll.dll runtime-info\r\n\
  .\\mp_launcher.exe --pid PID --dll .\\client_dll.dll session-record --output PASTA-NOVA --seconds 600 --interval-ms 5000\r\n\
  .\\mp_doctor.exe --recording PASTA-NOVA\r\n\
Substitua PID pelo processo atual. O coletor nao inicia nem para a sessao.\r\n\
Ele salva estados filtrados, analise e hashes; nao precisa pedir status a\r\n\
cada movimento. Ctrl+C finaliza apos a chamada em andamento. Fechar a janela\r\n\
pode deixar .INCOMPLETE; preserve os arquivos e use pasta nova. No fim, use\r\n\
3 no assistente para limpar antes de fechar o jogo, como acima. Coleta\r\n\
completa nao equivale a validacao visual nem substitui os hashes de parada.\r\n\
Captura experimental sem draw: inicie mp_lan.exe --capture game-pose em\r\n\
terminal, antes da opcao 1; ainda exige a rodada pratica de cameras/foco/load.\r\n",
        settings.server,
        settings.room,
        settings.server.port()
    )
}
fn career_instructions() -> &'static str {
    "CARREIRA LAN POR ARQUIVO - OPCIONAL\r\n\r\n\
Este modo verifica a copia congelada do save-base, identidade, mapa/mods e\r\n\
ticket de admissao. Ainda NAO carrega/instala o save no jogo nem compartilha\r\n\
horario, transito, fisica, saldo ou vitoria do motor. O visual e o ja existente.\r\n\r\n\
O host deve preparar a carreira e usar mp_career host (TCP e relay UDP juntos).\r\n\
Cada jogador recebe o convite PRIVADO por fora do kit, importa em pasta NOVA e\r\n\
cria sua PROPRIA identidade/ticket. Inclusive o host como jogador faz import.\r\n\
Nao copie identidade, launch.json, plan.json ou a pasta de dados entre PCs.\r\n\
MAPA significa o mesmo arquivo escolhido pelo host; informe tambem --mod por\r\n\
cada mod adicional declarado. Crie a pasta PRIVADO antes dos comandos abaixo.\r\n\r\n\
Exemplos de terminal (substitua IP e caminhos):\r\n\
mp_career.exe join --server IP:39001 --invite CONVITE.json --output IMPORTACAO-NOVA --map MAPA\r\n\
mp_career.exe prepare-launch --server IP:39001 --invite CONVITE.json --import IMPORTACAO-NOVA --identity-file PRIVADO/identity.json --output PRIVADO/launch.json --assistant-plan PRIVADO/plan.json --map MAPA\r\n\
mp_career.exe check-plan --plan PRIVADO/plan.json\r\n\
mp_lan.exe --career-plan PRIVADO/plan.json --check-only\r\n\r\n\
O ticket dura ate uma hora para novas entradas, e reiniciar o host o invalida.\r\n\
Renovar exige novos arquivos de ticket/plano, nao sobrescrever os antigos.\r\n\
Se o ticket foi salvo mas a gravacao do plano falhou, recupere sem rede com:\r\n\
mp_career.exe plan-launch --launch PRIVADO/launch.json --import IMPORTACAO-NOVA --identity-file PRIVADO/identity.json --map MAPA --output PRIVADO/plan-NOVO.json\r\n\r\n\
Confira a importacao e copie manualmente bootstrap.sav para um slot de teste\r\n\
NOVO na pasta real dos saves, com o jogo fechado e sem substituir seus saves.\r\n\
Carregue esse save pela Steam. Abra entao:\r\n\
mp_lan.exe --career-plan PRIVADO/plan.json\r\n\
Escolha 1; servidor/sala/identidade sao os vinculados ao arquivo. Confirme a\r\n\
pasta dos saves. Use 3 antes de fechar o jogo e aguarde a limpeza, como no modo\r\n\
visual. Opcoes 2/3/4 continuam disponiveis mesmo se o plano/ticket ficar invalido.\r\n\
Os checks locais nao consultam validade no servidor nem comprovam qual save\r\n\
esta carregado. Admissao efetiva e feita pelo launcher/relay no inicio.\r\n\r\n\
Ferramentas de desenvolvimento adicionais: mp_career appearance-sync recebe\r\n\
aparencias DECLARADAS e guarda progresso em arquivo privado NOVO; --state\r\n\
retoma o arquivo anterior. appearance-state-check confere esse estado offline.\r\n\
Consulte --help para os arquivos de contexto e ticket necessarios. Nao e\r\n\
necessario usar esses comandos para a rodada visual pelo mp_lan. Eles ainda\r\n\
nao mudam modelo/pintura no jogo nem sincronizam o mundo vivo.\r\n"
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
    fn new_kit_includes_career_tool_and_legacy_schema_remains_verifiable() {
        let t = tempfile::tempdir().unwrap();
        fixtures(t.path());
        let out = t.path().join("kit");
        let mut m = create(t.path(), &out, "own revision", &settings(), None).unwrap();
        assert_eq!(m.schema, 2);
        assert!(m.files.iter().any(|e| e.name == "mp_career.exe"));
        assert!(
            fs::read_to_string(out.join("CARREIRA.txt"))
                .unwrap()
                .contains("--career-plan")
        );
        m.schema = 1;
        m.files.retain(|e| e.name != "mp_career.exe");
        fs::remove_file(out.join("mp_career.exe")).unwrap();
        fs::write(out.join(MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(verify(&out).is_ok());
        m.schema = 2;
        fs::write(out.join(MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(verify(&out).is_err());
        fs::remove_file(t.path().join("mp_career.exe")).unwrap();
        let absent = t.path().join("incomplete-new-kit");
        assert!(create(t.path(), &absent, "own", &settings(), None).is_err());
        assert!(!absent.exists());
    }
    #[test]
    fn rejects_incomplete_wrong_architecture_and_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("kit");
        assert!(create(temp.path(), &out, "test", &settings(), None).is_err());
        assert!(!out.exists());
        fixtures(temp.path());
        let bad = temp.path().join("mp_lan.exe");
        let mut bytes = fs::read(&bad).unwrap();
        bytes[68] = 0x64;
        fs::write(&bad, bytes).unwrap();
        assert!(create(temp.path(), &out, "test", &settings(), None).is_err());
        assert!(!out.exists());
        fixtures(temp.path());
        create(temp.path(), &out, "test", &settings(), None).unwrap();
        fs::write(out.join("preserve.txt"), "owned by user").unwrap();
        assert!(create(temp.path(), &out, "test", &settings(), None).is_err());
        assert_eq!(
            fs::read_to_string(out.join("preserve.txt")).unwrap(),
            "owned by user"
        );
    }
    #[test]
    fn optional_third_party_notice_is_hashed_verified_and_absent_in_older_kits() {
        let temp = tempfile::tempdir().unwrap();
        fixtures(temp.path());
        let notice = temp.path().join("notice.txt");
        fs::write(&notice, "AVISOS DE TERCEIROS\r\n== crate 1.0 ==\r\n").unwrap();
        let out = temp.path().join("kit");
        let m = create(temp.path(), &out, "test", &settings(), Some(&notice)).unwrap();
        assert_eq!(m.notices.as_ref().unwrap().name, NOTICES);
        assert!(verify(&out).is_ok());
        fs::write(out.join(NOTICES), "edited").unwrap();
        assert!(verify(&out).is_err());
        fs::remove_file(out.join(NOTICES)).unwrap();
        assert!(verify(&out).is_err());
        // Kits without the field stay valid; binary notices are refused.
        let plain = temp.path().join("plain");
        let m = create(temp.path(), &plain, "test", &settings(), None).unwrap();
        assert!(m.notices.is_none() && !plain.join(NOTICES).exists());
        assert!(
            !fs::read_to_string(plain.join(MANIFEST))
                .unwrap()
                .contains("notices")
        );
        fs::write(&notice, [0xffu8, 0xfe]).unwrap();
        let refused = temp.path().join("refused");
        assert!(create(temp.path(), &refused, "test", &settings(), Some(&notice)).is_err());
        assert!(!refused.exists());
    }
    #[test]
    fn detects_tampering_duplicate_and_traversal_entries() {
        let temp = tempfile::tempdir().unwrap();
        fixtures(temp.path());
        let out = temp.path().join("kit");
        let mut manifest = create(temp.path(), &out, "test", &settings(), None).unwrap();
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
    #[test]
    fn bounded_manifest_settings_and_notice_fail_without_new_output() {
        let t = tempfile::tempdir().unwrap();
        fixtures(t.path());
        let out = t.path().join("kit");
        create(t.path(), &out, "own revision", &settings(), None).unwrap();
        let manifest = fs::read(out.join(MANIFEST)).unwrap();
        fs::write(out.join(MANIFEST), vec![b' '; MAX_MANIFEST + 1]).unwrap();
        assert!(verify(&out).is_err());
        fs::write(out.join(MANIFEST), manifest).unwrap();
        fs::write(out.join(CONFIG), vec![b' '; 4097]).unwrap();
        assert!(verify(&out).is_err());
        fs::write(out.join(CONFIG), serde_json::to_vec(&settings()).unwrap()).unwrap();
        verify(&out).unwrap();
        let notice = t.path().join("notice.txt");
        let file = fs::File::create(&notice).unwrap();
        file.set_len(MAX_NOTICE as u64 + 1).unwrap();
        drop(file);
        let refused = t.path().join("refused");
        assert!(create(t.path(), &refused, "own", &settings(), Some(&notice)).is_err());
        assert!(!refused.exists());
    }
    #[test]
    fn malformed_revision_sizes_and_mutated_member_names_never_escape_allowlist() {
        let t = tempfile::tempdir().unwrap();
        fixtures(t.path());
        let out = t.path().join("kit");
        let mut manifest =
            create(t.path(), &out, "legacy spaces allowed", &settings(), None).unwrap();
        for invalid in [
            "../outside.dll",
            "/outside.dll",
            "C:\\outside.dll",
            "CON",
            "client_dll.dll/..",
            "mp_launcher.exe:stream",
            "客户端.dll",
            "client_dll.dll\0",
            "client_dll.dll ",
        ] {
            manifest.files[0].name = invalid.into();
            fs::write(out.join(MANIFEST), serde_json::to_vec(&manifest).unwrap()).unwrap();
            assert!(verify(&out).is_err());
        }
        manifest.files[0].name = PAYLOAD[0].into();
        manifest.files[0].bytes = u64::MAX;
        fs::write(out.join(MANIFEST), serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(verify(&out).is_err());
        manifest.files[0].bytes = binary(&out.join(PAYLOAD[0])).unwrap().len() as u64;
        manifest.source_revision = "own\nprivate text".into();
        fs::write(out.join(MANIFEST), serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(verify(&out).is_err());
    }
}
