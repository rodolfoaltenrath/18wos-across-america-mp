//! All new interface strings have stable keys. Original game text is not embedded.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "pt-BR")]
    PtBr,
    #[serde(rename = "en")]
    En,
}
#[derive(Clone, Copy)]
pub struct Entry {
    pub key: &'static str,
    pub pt: &'static str,
    pub en: &'static str,
}
macro_rules! entries { ($( $key:literal => ($pt:literal,$en:literal)),* $(,)?) => { pub const ENTRIES: &[Entry]=&[$(Entry{key:$key,pt:$pt,en:$en}),*]; }; }
entries! {
    "brand" => ("ACROSS AMERICA / MP", "ACROSS AMERICA / MP"),
    "preview" => ("PRÉVIA LOCAL • SEM CONEXÃO", "LOCAL PREVIEW • OFFLINE"),
    "main" => ("A estrada começa aqui.", "Your next road starts here."),
    "main.sub" => ("Carreira, estradas e novas companhias.", "A career, open roads and good company."),
    "continue" => ("Continuar carreira", "Continue career"),
    "career" => ("Nova carreira", "New career"),
    "load" => ("Carregar partida", "Load a save"),
    "multiplayer" => ("Multiplayer", "Multiplayer"),
    "settings" => ("Configurações", "Settings"),
    "credits" => ("Créditos", "Credits"),
    "exit" => ("Sair", "Exit"),
    "back" => ("Voltar", "Back"),
    "cancel" => ("Cancelar", "Cancel"),
    "host" => ("Hospedar partida", "Host a game"),
    "join" => ("Entrar em uma partida", "Join a game"),
    "mp.sub" => ("Escolha como compartilhar a estrada.", "Choose how to share the road."),
    "host.sub" => ("Uma carreira. As mesmas regras para todos.", "One career. Shared rules for everyone."),
    "player" => ("Seu nome", "Your name"),
    "room" => ("Nome da sala", "Room name"),
    "limit" => ("Limite de jogadores", "Player limit"),
    "scenario" => ("Carreira: Become a Millionaire", "Career: Become a Millionaire"),
    "goal" => ("Meta financeira (valor do jogo)", "Financial target (game currency)"),
    "unlimited" => ("Sem limite financeiro", "No financial limit"),
    "rules.help" => ("A meta vale para a carreira; a moeda do jogo ainda não foi alterada.", "The target belongs to the career; game currency is unchanged."),
    "host.submit" => ("Preparar sala", "Prepare room"),
    "join.sub" => ("Encontre seus amigos na rede local.", "Find friends on your local network."),
    "address" => ("Endereço da partida", "Game address"),
    "search" => ("Procurar na LAN", "Find LAN games"),
    "connect" => ("Conectar", "Connect"),
    "empty" => ("Nenhuma partida encontrada.", "No games found."),
    "lobby" => ("Sala de espera", "Lobby"),
    "lobby.sub" => ("Confira a carreira antes de partir.", "Review the career before setting off."),
    "players" => ("Jogadores: {count}", "Players: {count}"),
    "start" => ("Entrar na estrada", "Enter the road"),
    "leave" => ("Sair da sala", "Leave room"),
    "loading" => ("Preparando sua viagem…", "Preparing your trip…"),
    "loading.sub" => ("Verificando a carreira e a conexão.", "Checking the career and connection."),
    "error" => ("Não foi possível continuar", "Could not continue"),
    "settings.sub" => ("Preferências da interface do mod.", "Mod interface preferences."),
    "language" => ("Idioma: Português (Brasil)", "Language: English"),
    "language.toggle" => ("Trocar idioma", "Change language"),
    "pause" => ("Sua viagem", "Your journey"),
    "resume" => ("Voltar à estrada", "Return to the road"),
    "pause.sub" => ("Uma pausa no caminho.", "A break along the way."),
    "offline" => ("Sem conexão ativa nesta prévia.", "No active connection in this preview."),
    "examples" => ("Ver exemplo de sala", "Preview a lobby"),
    "example.progress" => ("Ver tela de preparação", "Preview preparation"),
    "err.preview" => ("Esta prévia não abre o jogo nem inicia uma conexão.", "This preview does not launch a game or make a connection."),
    "err.name" => ("Informe um nome de até 32 caracteres.", "Enter a name of up to 32 characters."),
    "err.room" => ("Use até 32 letras, números, traços ou sublinhados na sala.", "Use up to 32 letters, numbers, dashes or underscores for the room."),
    "err.address" => ("Informe um IP e uma porta válidos, como 192.168.1.10:39000.", "Enter a valid IP and port, such as 192.168.1.10:39000."),
    "err.goal" => ("Informe uma meta positiva, por exemplo 1.000.000,00.", "Enter a positive target, for example 1.000.000,00."),
    "err.limit" => ("O limite de jogadores deve estar entre 2 e 64.", "Player limit must be between 2 and 64."),
    "err.failed" => ("A operação falhou. Seus arquivos existentes foram preservados.", "The operation failed. Your existing files were preserved."),
    "native.pending" => ("Multiplayer (em preparo)", "Multiplayer (in preparation)"),
    "native.note" => ("PREVIA DO MENU / conexao ainda pelo assistente", "MENU PREVIEW / connect using the companion"),
}

pub fn text(key: &str, language: Language) -> &'static str {
    ENTRIES
        .iter()
        .find(|v| v.key == key)
        .map(|v| match language {
            Language::PtBr => v.pt,
            Language::En => v.en,
        })
        .unwrap_or("[texto indisponível]")
}
fn placeholders(text: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(['{', '}']) {
        if rest.as_bytes()[start] == b'}' {
            return Err("unbalanced placeholder".into());
        }
        rest = &rest[start + 1..];
        let end = rest.find('}').ok_or("unclosed placeholder")?;
        let key = &rest[..end];
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("invalid placeholder".into());
        }
        out.push(key);
        rest = &rest[end + 1..];
    }
    out.sort_unstable();
    Ok(out)
}
/// A future translated catalogue must be complete, bounded and retain placeholders.
pub fn validate_catalogue(values: &BTreeMap<String, String>) -> Result<(), String> {
    if values.len() != ENTRIES.len() {
        return Err("missing/unknown translation keys".into());
    }
    for entry in ENTRIES {
        let value = values.get(entry.key).ok_or("missing translation key")?;
        if value.is_empty()
            || value.len() > 2048
            || value.chars().any(|c| c.is_control() && c != '\n')
            || placeholders(value)? != placeholders(entry.en)?
        {
            return Err(format!("invalid translation: {}", entry.key));
        }
    }
    Ok(())
}
pub fn catalogue(language: Language) -> BTreeMap<String, String> {
    ENTRIES
        .iter()
        .map(|e| (e.key.into(), text(e.key, language).into()))
        .collect()
}

/// Native font/codepage support is not proven. Preview-only transliteration,
/// never used to overwrite the canonical Unicode catalogue.
pub fn native_ascii(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
            'é' | 'ê' | 'è' | 'ë' => 'e',
            'É' | 'Ê' | 'È' | 'Ë' => 'E',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'ç' => 'c',
            'Ç' => 'C',
            '…' => '.',
            '•' => '/',
            c if c.is_ascii() && !c.is_control() => c,
            _ => '?',
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogues_are_complete_and_placeholders_cannot_disappear() {
        for l in [Language::PtBr, Language::En] {
            validate_catalogue(&catalogue(l)).unwrap();
        }
        let mut c = catalogue(Language::PtBr);
        c.insert("players".into(), "Jogadores".into());
        assert!(validate_catalogue(&c).is_err());
        c = catalogue(Language::PtBr);
        c.insert("wrong".into(), "x".into());
        assert!(validate_catalogue(&c).is_err());
        assert_eq!(native_ascii("Configurações: ação"), "Configuracoes: acao");
    }
}
