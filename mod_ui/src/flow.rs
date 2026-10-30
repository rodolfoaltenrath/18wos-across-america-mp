//! Navigation and validated intents. A backend must explicitly complete requests.
//! Cancelling or changing screen invalidates pending responses; no file/game effects here.
use crate::locale::Language;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    Main,
    Multiplayer,
    Host,
    Join,
    Lobby,
    Loading,
    Settings,
    Error,
    Pause,
}
pub const SCREENS: &[Screen] = &[
    Screen::Main,
    Screen::Multiplayer,
    Screen::Host,
    Screen::Join,
    Screen::Lobby,
    Screen::Loading,
    Screen::Settings,
    Screen::Error,
    Screen::Pause,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Name,
    Room,
    Goal,
    Limit,
    Address,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Navigate { screen: Screen },
    Back,
    SubmitHost,
    SubmitJoin,
    Discover,
    Start,
    ToggleUnlimited,
    ToggleLanguage,
    Exit,
    Showcase { screen: Screen },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Goal {
    Unlimited,
    Financial { minor_units: u64 },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDraft {
    pub name: String,
    pub room: String,
    pub limit: u16,
    pub goal: Goal,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinDraft {
    pub name: String,
    pub address: SocketAddr,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Host { request: u64, draft: HostDraft },
    Join { request: u64, draft: JoinDraft },
    Discover { request: u64 },
    Cancel { request: u64 },
    Start,
    Exit,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct App {
    pub screen: Screen,
    pub language: Language,
    pub preview: bool,
    pub name: String,
    pub room: String,
    pub goal: String,
    pub limit: String,
    pub address: String,
    pub unlimited: bool,
    pub error_key: String,
    pub connected: bool,
    pub pending: Option<u64>,
    next: u64,
    previous: Screen,
}
impl Default for App {
    fn default() -> Self {
        Self {
            screen: Screen::Main,
            language: Language::PtBr,
            preview: true,
            name: "Motorista".into(),
            room: "Minha_sala".into(),
            goal: "1.000.000,00".into(),
            limit: "4".into(),
            address: "127.0.0.1:39000".into(),
            unlimited: false,
            error_key: "err.preview".into(),
            connected: false,
            pending: None,
            next: 0,
            previous: Screen::Main,
        }
    }
}
impl App {
    pub fn preview(screen: Screen, language: Language) -> Self {
        Self {
            screen,
            language,
            ..Self::default()
        }
    }
    pub fn field(&self, f: Field) -> &str {
        match f {
            Field::Name => &self.name,
            Field::Room => &self.room,
            Field::Goal => &self.goal,
            Field::Limit => &self.limit,
            Field::Address => &self.address,
        }
    }
    pub fn set_field(&mut self, f: Field, value: String) -> Result<(), String> {
        if self.pending.is_some() || value.len() > 128 || value.chars().any(char::is_control) {
            return Err("input unavailable or out of bounds".into());
        }
        if f == Field::Goal && self.unlimited {
            return Err("financial target disabled".into());
        }
        *match f {
            Field::Name => &mut self.name,
            Field::Room => &mut self.room,
            Field::Goal => &mut self.goal,
            Field::Limit => &mut self.limit,
            Field::Address => &mut self.address,
        } = value;
        Ok(())
    }
    fn name_valid(&self) -> bool {
        let n = self.name.trim();
        !n.is_empty() && n.chars().count() <= 32 && !n.chars().any(char::is_control)
    }
    pub fn host(&self) -> Result<HostDraft, &'static str> {
        if !self.name_valid() {
            return Err("err.name");
        }
        if self.room.is_empty()
            || self.room.len() > 32
            || !self
                .room
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err("err.room");
        }
        let limit = self
            .limit
            .parse::<u16>()
            .ok()
            .filter(|v| (2..=64).contains(v))
            .ok_or("err.limit")?;
        let goal = if self.unlimited {
            Goal::Unlimited
        } else {
            Goal::Financial {
                minor_units: parse_goal(&self.goal).map_err(|_| "err.goal")?,
            }
        };
        Ok(HostDraft {
            name: self.name.trim().into(),
            room: self.room.clone(),
            limit,
            goal,
        })
    }
    pub fn join(&self) -> Result<JoinDraft, &'static str> {
        if !self.name_valid() {
            return Err("err.name");
        }
        let address = self
            .address
            .trim()
            .parse::<SocketAddr>()
            .ok()
            .filter(|a| {
                a.port() != 0
                    && !a.ip().is_unspecified()
                    && !a.ip().is_multicast()
                    && !matches!(a.ip(),IpAddr::V4(ip)if ip.is_broadcast())
            })
            .ok_or("err.address")?;
        Ok(JoinDraft {
            name: self.name.trim().into(),
            address,
        })
    }
    fn request(&mut self) -> Option<u64> {
        self.next = self.next.checked_add(1)?;
        self.pending = Some(self.next);
        Some(self.next)
    }
    pub fn fail(&mut self, key: &str) {
        self.error_key =
            if key.starts_with("err.") && crate::locale::ENTRIES.iter().any(|e| e.key == key) {
                key.into()
            } else {
                "err.failed".into()
            };
        self.pending = None;
        self.screen = Screen::Error;
    }
    pub fn complete(&mut self, request: u64, success: bool) -> bool {
        if self.pending != Some(request) {
            return false;
        }
        self.pending = None;
        if success && !self.preview {
            self.connected = true;
            self.screen = Screen::Lobby;
        } else {
            self.fail(if self.preview {
                "err.preview"
            } else {
                "err.failed"
            });
        }
        true
    }
    pub fn act(&mut self, action: Action) -> Option<Effect> {
        if let Some(request) = self.pending {
            if action == Action::Back {
                self.pending = None;
                self.screen = self.previous;
                return Some(Effect::Cancel { request });
            }
            return None;
        }
        match action {
            Action::Back => {
                self.screen = match self.screen {
                    Screen::Host | Screen::Join => Screen::Multiplayer,
                    Screen::Lobby => {
                        self.connected = false;
                        Screen::Multiplayer
                    }
                    Screen::Error | Screen::Loading => self.previous,
                    _ => Screen::Main,
                };
                None
            }
            Action::Navigate { screen } => {
                self.previous = self.screen;
                self.screen = screen;
                None
            }
            Action::Showcase { screen } => {
                if self.preview {
                    self.previous = self.screen;
                    self.screen = screen;
                }
                None
            }
            Action::ToggleUnlimited => {
                self.unlimited = !self.unlimited;
                None
            }
            Action::ToggleLanguage => {
                self.language = match self.language {
                    Language::PtBr => Language::En,
                    Language::En => Language::PtBr,
                };
                None
            }
            Action::SubmitHost => match self.host() {
                Ok(draft) => {
                    self.previous = Screen::Host;
                    let request = self.request()?;
                    self.screen = Screen::Loading;
                    Some(Effect::Host { request, draft })
                }
                Err(key) => {
                    self.previous = Screen::Host;
                    self.fail(key);
                    None
                }
            },
            Action::SubmitJoin => match self.join() {
                Ok(draft) => {
                    self.previous = Screen::Join;
                    let request = self.request()?;
                    self.screen = Screen::Loading;
                    Some(Effect::Join { request, draft })
                }
                Err(key) => {
                    self.previous = Screen::Join;
                    self.fail(key);
                    None
                }
            },
            Action::Discover => {
                self.previous = Screen::Join;
                let request = self.request()?;
                self.screen = Screen::Loading;
                Some(Effect::Discover { request })
            }
            Action::Start => {
                if self.connected && !self.preview {
                    Some(Effect::Start)
                } else {
                    self.previous = self.screen;
                    self.fail("err.preview");
                    None
                }
            }
            Action::Exit => Some(Effect::Exit),
        }
    }
}
/// Exact Brazilian numeric input, independent of float/currency conversion.
pub fn parse_goal(value: &str) -> Result<u64, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 32 {
        return Err("invalid target".into());
    }
    let mut parts = value.split(',');
    let whole = parts.next().unwrap_or("");
    let decimal = parts.next().unwrap_or("00");
    if parts.next().is_some() || decimal.len() != 2 || !decimal.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("use exactly two decimal digits".into());
    }
    let groups: Vec<_> = whole.split('.').collect();
    if groups
        .iter()
        .any(|g| g.is_empty() || !g.bytes().all(|b| b.is_ascii_digit()))
        || (groups.len() > 1 && (groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3)))
    {
        return Err("invalid grouping".into());
    }
    let number = groups
        .concat()
        .parse::<u64>()
        .map_err(|_| "target overflow")?;
    let minor = number
        .checked_mul(100)
        .and_then(|n| decimal.parse::<u64>().ok().and_then(|d| n.checked_add(d)))
        .filter(|n| (1..=1_000_000_000_000_000).contains(n))
        .ok_or("target outside bounds")?;
    Ok(minor)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn financial_input_is_exact_bounded_and_unlimited_retains_the_draft() {
        assert_eq!(parse_goal("1.000.000,01").unwrap(), 100_000_001);
        for bad in [
            "0",
            "1,2",
            "1,234",
            "1.00,00",
            "NaN",
            "-1",
            "18446744073709551615,99",
        ] {
            assert!(parse_goal(bad).is_err(), "{bad}");
        }
        let mut a = App {
            goal: "invalid".into(),
            ..App::default()
        };
        assert!(a.host().is_err());
        a.act(Action::ToggleUnlimited);
        assert_eq!(a.host().unwrap().goal, Goal::Unlimited);
        assert!(a.set_field(Field::Goal, "5".into()).is_err());
        a.act(Action::ToggleUnlimited);
        assert_eq!(a.goal, "invalid");
    }
    #[test]
    fn cancel_invalidates_late_success_and_preview_never_claims_connection() {
        let mut a = App {
            screen: Screen::Host,
            ..App::default()
        };
        let Some(Effect::Host { request, .. }) = a.act(Action::SubmitHost) else {
            panic!("host intent")
        };
        assert_eq!(a.act(Action::Back), Some(Effect::Cancel { request }));
        assert!(!a.complete(request, true));
        assert!(!a.connected);
        let Some(Effect::Join { request, .. }) = a.act(Action::SubmitJoin) else {
            panic!("join intent")
        };
        assert!(a.complete(request, true));
        assert_eq!(a.screen, Screen::Error);
        assert!(!a.connected);
    }
    #[test]
    fn address_and_input_limits_are_checked_before_any_effect() {
        let mut a = App::default();
        for bad in [
            "0.0.0.0:39000",
            "239.0.0.1:39000",
            "255.255.255.255:39000",
            "127.0.0.1:0",
            "server",
        ] {
            a.address = bad.into();
            assert!(a.join().is_err());
        }
        assert!(a.set_field(Field::Name, "x".repeat(129)).is_err());
        a.room = "name with spaces".into();
        assert!(a.host().is_err());
    }
}
