//! Shared logical 1024x768 layout. Letterbox scaling never stretches hit regions.
use crate::{
    flow::{Action, App, Field, Screen},
    locale,
};
use serde::Serialize;
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
/// Original main-menu footprint, in our top-down 1024x768 coordinates.
/// Native ESC uses the same rectangle as `27 80 347 490` (bottom-up).
pub const MAIN_PANEL: Rect = Rect {
    x: 27,
    y: 278,
    w: 320,
    h: 410,
};
pub fn main_button_rect(slot: usize) -> Rect {
    Rect {
        x: 47,
        y: 334 + slot as i32 * 48,
        w: 280,
        h: 32,
    }
}
impl Rect {
    pub fn hit(self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && y >= self.y as f64
            && x < (self.x + self.w) as f64
            && y < (self.y + self.h) as f64
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    Label,
    Button { action: Action },
    Input { field: Field },
    Toggle { action: Action, checked: bool },
}
#[derive(Clone, Debug, Serialize)]
pub struct Widget {
    pub id: u16,
    pub rect: Rect,
    pub text: String,
    pub key: String,
    pub kind: Kind,
    pub enabled: bool,
    pub primary: bool,
}
#[derive(Debug, Serialize)]
pub struct View {
    pub screen: Screen,
    pub title: String,
    pub subtitle: String,
    pub preview: bool,
    pub widgets: Vec<Widget>,
}
impl View {
    pub fn build(a: &App) -> Self {
        let (title, sub) = match a.screen {
            Screen::Main => ("menu.title", ""),
            Screen::Multiplayer => ("multiplayer", "mp.sub"),
            Screen::Host => ("host", "host.sub"),
            Screen::Join => ("join", "join.sub"),
            Screen::Lobby => ("lobby", "lobby.sub"),
            Screen::Loading => ("loading", "loading.sub"),
            Screen::Settings => ("settings", "settings.sub"),
            Screen::Error => ("error", a.error_key.as_str()),
            Screen::Pause => ("pause", "pause.sub"),
        };
        let mut v = Self {
            screen: a.screen,
            title: locale::text(title, a.language).into(),
            subtitle: locale::text(sub, a.language).into(),
            preview: a.preview,
            widgets: Vec::new(),
        };
        let mut y = 220;
        let mut add = |key: &str, kind: Kind, enabled: bool, primary: bool| {
            let label = locale::text(key, a.language);
            let text = if let Kind::Input { field } = kind {
                format!("{label}: {}", a.field(field))
            } else {
                label.into()
            };
            v.widgets.push(Widget {
                id: 6000 + v.widgets.len() as u16,
                rect: Rect {
                    x: 72,
                    y,
                    w: 880,
                    h: 44,
                },
                text,
                key: key.into(),
                kind,
                enabled,
                primary,
            });
            y += 56;
        };
        let button = |screen| Kind::Button {
            action: Action::Navigate { screen },
        };
        match a.screen {
            Screen::Main => {
                add("career", button(Screen::Host), true, false);
                add(
                    "load",
                    Kind::Button {
                        action: Action::Start,
                    },
                    true,
                    false,
                );
                add("multiplayer", button(Screen::Multiplayer), true, true);
                add("settings", button(Screen::Settings), true, false);
                add(
                    "credits",
                    Kind::Button {
                        action: Action::Start,
                    },
                    true,
                    false,
                );
                add(
                    "exit",
                    Kind::Button {
                        action: Action::Exit,
                    },
                    true,
                    false,
                );
            }
            Screen::Multiplayer => {
                add("host", button(Screen::Host), true, true);
                add("join", button(Screen::Join), true, false);
                if a.preview {
                    add(
                        "examples",
                        Kind::Button {
                            action: Action::Showcase {
                                screen: Screen::Lobby,
                            },
                        },
                        true,
                        false,
                    );
                    add(
                        "example.progress",
                        Kind::Button {
                            action: Action::Showcase {
                                screen: Screen::Loading,
                            },
                        },
                        true,
                        false,
                    );
                }
            }
            Screen::Host => {
                add("player", Kind::Input { field: Field::Name }, true, false);
                add("room", Kind::Input { field: Field::Room }, true, false);
                add(
                    "limit",
                    Kind::Input {
                        field: Field::Limit,
                    },
                    true,
                    false,
                );
                add(
                    "goal",
                    Kind::Input { field: Field::Goal },
                    !a.unlimited,
                    false,
                );
                add(
                    "unlimited",
                    Kind::Toggle {
                        action: Action::ToggleUnlimited,
                        checked: a.unlimited,
                    },
                    true,
                    false,
                );
                add(
                    "host.submit",
                    Kind::Button {
                        action: Action::SubmitHost,
                    },
                    true,
                    true,
                );
                add("scenario", Kind::Label, false, false);
            }
            Screen::Join => {
                add("player", Kind::Input { field: Field::Name }, true, false);
                add(
                    "address",
                    Kind::Input {
                        field: Field::Address,
                    },
                    true,
                    false,
                );
                add(
                    "search",
                    Kind::Button {
                        action: Action::Discover,
                    },
                    true,
                    false,
                );
                add("empty", Kind::Label, false, false);
                add(
                    "connect",
                    Kind::Button {
                        action: Action::SubmitJoin,
                    },
                    true,
                    true,
                );
            }
            Screen::Lobby => {
                add("scenario", Kind::Label, false, false);
                add("offline", Kind::Label, false, false);
                add(
                    "start",
                    Kind::Button {
                        action: Action::Start,
                    },
                    a.connected && !a.preview,
                    true,
                );
                add(
                    "leave",
                    Kind::Button {
                        action: Action::Back,
                    },
                    true,
                    false,
                );
            }
            Screen::Loading => {
                add("scenario", Kind::Label, false, false);
                add(
                    "cancel",
                    Kind::Button {
                        action: Action::Back,
                    },
                    true,
                    false,
                );
            }
            Screen::Settings => {
                add("language", Kind::Label, false, false);
                add(
                    "language.toggle",
                    Kind::Button {
                        action: Action::ToggleLanguage,
                    },
                    true,
                    true,
                );
            }
            Screen::Error => {
                add(
                    "back",
                    Kind::Button {
                        action: Action::Back,
                    },
                    true,
                    true,
                );
            }
            Screen::Pause => {
                add(
                    "resume",
                    Kind::Button {
                        action: Action::Back,
                    },
                    true,
                    true,
                );
                add("multiplayer", button(Screen::Lobby), true, false);
                add("settings", button(Screen::Settings), true, false);
            }
        }
        if a.screen == Screen::Main {
            v.subtitle.clear();
            for (slot, widget) in v.widgets.iter_mut().enumerate() {
                widget.rect = main_button_rect(slot);
            }
        }
        if !matches!(a.screen, Screen::Main | Screen::Error | Screen::Loading) {
            v.widgets.push(Widget {
                id: 6999,
                rect: Rect {
                    x: 72,
                    y: 680,
                    w: 190,
                    h: 40,
                },
                text: locale::text("back", a.language).into(),
                key: "back".into(),
                kind: Kind::Button {
                    action: Action::Back,
                },
                enabled: true,
                primary: false,
            });
        }
        v
    }
    pub fn hit(&self, x: f64, y: f64) -> Option<&Widget> {
        self.widgets
            .iter()
            .rev()
            .find(|w| w.enabled && !matches!(w.kind, Kind::Label) && w.rect.hit(x, y))
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub scale: f64,
    pub x: f64,
    pub y: f64,
}
impl Transform {
    pub fn new(w: i32, h: i32) -> Option<Self> {
        if w < 1 || h < 1 {
            return None;
        }
        let scale = (w as f64 / 1024.0).min(h as f64 / 768.0);
        Some(Self {
            scale,
            x: (w as f64 - 1024.0 * scale) / 2.0,
            y: (h as f64 - 768.0 * scale) / 2.0,
        })
    }
    pub fn logical(self, x: i32, y: i32) -> (f64, f64) {
        (
            (x as f64 - self.x) / self.scale,
            (y as f64 - self.y) / self.scale,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_screen_fits_and_hit_testing_respects_letterboxing() {
        for &screen in crate::flow::SCREENS {
            let a = App::preview(screen, crate::locale::Language::PtBr);
            let v = View::build(&a);
            let mut ids = std::collections::BTreeSet::new();
            for w in &v.widgets {
                assert!(ids.insert(w.id));
                assert!(w.rect.y + w.rect.h <= 768);
                assert!(w.rect.x + w.rect.w <= 1024);
                if screen == Screen::Main {
                    assert!(w.rect.x >= MAIN_PANEL.x && w.rect.y >= MAIN_PANEL.y);
                    assert!(w.rect.x + w.rect.w <= MAIN_PANEL.x + MAIN_PANEL.w);
                    assert!(w.rect.y + w.rect.h <= MAIN_PANEL.y + MAIN_PANEL.h);
                }
            }
            if screen == Screen::Main {
                assert!(v.hit(500.0, 400.0).is_none());
            }
            for (width, height) in [(1024, 768), (1920, 1080), (800, 600), (2560, 1080)] {
                let t = Transform::new(width, height).unwrap();
                let p = t.logical(
                    (72.0 * t.scale + t.x) as i32,
                    (230.0 * t.scale + t.y) as i32,
                );
                if !v.widgets.is_empty() {
                    assert!(p.0 >= 70.0);
                }
            }
        }
        assert!(Transform::new(0, 768).is_none());
    }
}
