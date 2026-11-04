//! Own native ESC generation. No original screen text or scripts are bundled.
//! Native actions require an adapter: generated MP screens have no game handler.
use crate::{
    flow::{App, Screen},
    locale::{self, Language},
    view::{Kind, MAIN_PANEL, Rect, View},
};
fn quoted(text: &str) -> String {
    let text = locale::native_ascii(text);
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}
fn component(kind: &str, values: &[(&str, String)]) -> String {
    let mut out = format!("entity \"{kind}\" {{\n evalues {{\n");
    for (k, v) in values {
        out.push_str(&format!("  \"{k}\" : {}\n", quoted(v)));
    }
    out.push_str(" }\n}\n");
    out
}
fn text(value: &str, x: i32, y: i32, w: i32, h: i32) -> String {
    component(
        "ui.comp.common.text",
        &[
            ("look", "2".into()),
            ("value", value.into()),
            ("layer", "2".into()),
            ("coords", format!("{x} {y} {} {}", x + w, y + h)),
        ],
    )
}
fn wrap(children: &str, handler: Option<&str>) -> String {
    wrap_at(
        children,
        handler,
        Rect {
            x: 0,
            y: 0,
            w: 1024,
            h: 768,
        },
    )
}
fn coords(r: Rect) -> String {
    format!("{} {} {} {}", r.x, 768 - r.y - r.h, r.x + r.w, 768 - r.y)
}
fn wrap_at(children: &str, handler: Option<&str>, bounds: Rect) -> String {
    let handler = handler
        .map(|s| format!("entity \"{s}\" {{ evalues {{ }} }}"))
        .unwrap_or_default();
    format!(
        "estruct {{\n client {{\n entities {{\n entity \"ui.window\" {{\n evalues {{\n \"wclass\" : \"edited\"\n \"name\" : \"window\"\n \"coords\" : \"{}\"\n \"fitting\" : \"true\"\n }}\n entities {{\n{children}\n }}\n }}\n{handler}\n }}\n }}\n}}\n",
        coords(bounds)
    )
}
pub fn screen(app: &App) -> String {
    let v = View::build(app);
    let main = app.screen == Screen::Main;
    let mut out = if main {
        text(&v.title, 47, 444, 280, 28)
    } else {
        text(&v.title, 72, 590, 880, 60)
    };
    if !main {
        out.push_str(&text(&v.subtitle, 72, 550, 880, 30));
    }
    for widget in v.widgets {
        let y = 768 - widget.rect.y - widget.rect.h;
        match widget.kind {
            Kind::Button { .. } | Kind::Toggle { .. } => out.push_str(&component(
                "ui.comp.common.button",
                &[
                    ("look", "5".into()),
                    ("value", widget.text.replace(['<', '>', '\\'], " ")),
                    ("layer", "2".into()),
                    ("id", widget.id.to_string()),
                    (
                        "area",
                        format!(
                            "{} {y} {} {}",
                            widget.rect.x,
                            widget.rect.x + widget.rect.w,
                            y + widget.rect.h
                        ),
                    ),
                    (
                        "coords",
                        format!(
                            "{} {y} {} {}",
                            widget.rect.x,
                            widget.rect.x + widget.rect.w,
                            y + widget.rect.h
                        ),
                    ),
                ],
            )),
            _ => out.push_str(&text(
                &widget.text.replace(['<', '>', '\\'], " "),
                widget.rect.x,
                y,
                widget.rect.w,
                widget.rect.h,
            )),
        }
    }
    if main {
        wrap_at(&out, None, MAIN_PANEL)
    } else {
        wrap(&out, None)
    }
}
fn logo(id: u32, shader: &str, area: &str) -> String {
    let image = format!("<img src=/shaders/ui/logo/{shader}.shd xscale=stretch yscale=stretch>");
    component(
        "ui.comp.button",
        &[
            ("p_pml", format!("<offset hshift=2 vshift=2>{image}")),
            ("s_pml", image.clone()),
            ("n_pml", image),
            ("layer", "1".into()),
            ("id", id.to_string()),
            ("coords", area.into()),
        ],
    )
}
/// Same native components, IDs and table keys the game's own main menu uses
/// (frame look 6, button look 1, logos 100/110), so accents and fonts follow the
/// translated table. The only addition is Multiplayer: an inactive label, not an
/// unknown ID sent to the original compiled handler. Button text is `@@key@@`.
pub fn main_preview(language: Language) -> String {
    const X: (i32, i32, i32) = (76, 286, 306);
    const TOP: i32 = 411;
    const STEP: i32 = 46;
    const H: i32 = 32;
    let mut buttons = String::new();
    for (slot, (id, key, name)) in [
        (Some(10), "@@ui_new_game@@", "new game"),
        (Some(20), "@@ui_load_game@@", "load game"),
        (None, "native.pending", ""),
        (Some(30), "@@ui_options@@", "options"),
        (Some(40), "@@ui_credits@@", "credits"),
        (Some(60), "@@ui_show_room@@", "showroom"),
        (Some(50), "@@ui_quit@@", "quit"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = TOP - STEP * slot as i32;
        let Some(id) = id else {
            buttons.push_str(&text(locale::text(key, language), X.0, y, X.2 - X.0, H));
            continue;
        };
        buttons.push_str(&component(
            "ui.comp.common.button",
            &[
                ("look", "1".into()),
                ("value", key.into()),
                ("layer", "1".into()),
                ("id", id.to_string()),
                ("area", format!("{} {y} {} {}", X.0, X.1, y + H)),
                ("coords", format!("{} {y} {} {}", X.0, X.2, y + H)),
                ("name", name.into()),
            ],
        ));
    }
    let bottom = TOP - STEP * 6;
    let mut out = logo(100, "scs", "114 30 157 73");
    out.push_str(&logo(110, "valusoft_small", "167 30 247 73"));
    out.push_str(&format!(
        "entity \"ui.comp.group\" {{\n evalues {{\n  \"fitting\" : \"true\"\n  \"coords\" : \"{} {bottom} {} {}\"\n }}\n entities {{\n{buttons} }}\n}}\n",
        X.0,
        X.2,
        TOP + H
    ));
    out.push_str(&component(
        "ui.comp.common.frame",
        &[("look", "6".into()), ("coords", "27 80 347 490".into())],
    ));
    wrap_at(
        &out,
        Some("ui.handler.menu.main"),
        Rect {
            x: 27,
            y: 768 - 490,
            w: 320,
            h: 460,
        },
    )
}
pub fn name(screen: Screen) -> &'static str {
    match screen {
        Screen::Main => "main",
        Screen::Multiplayer => "multiplayer",
        Screen::Host => "host",
        Screen::Join => "join",
        Screen::Lobby => "lobby",
        Screen::Loading => "loading",
        Screen::Settings => "settings",
        Screen::Error => "error",
        Screen::Pause => "pause",
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_preview_preserves_existing_ids_and_does_not_dispatch_unknown_actions() {
        let text = main_preview(Language::PtBr);
        assert!(text.contains("ui.handler.menu.main"));
        assert!(!text.contains("6000"));
        assert!(text.is_ascii());
        assert!(text.contains("\"coords\" : \"27 80 347 490\""));
        assert!(text.contains("\"coords\" : \"27 30 347 490\""));
        assert!(!text.contains("0 0 1024 768"));
        // Every native element of the original menu stays, in its native look.
        for id in [10, 20, 30, 40, 50, 60, 100, 110] {
            assert!(text.contains(&format!("\"id\" : \"{id}\"")), "id {id}");
        }
        for key in [
            "new_game",
            "load_game",
            "options",
            "credits",
            "show_room",
            "quit",
        ] {
            assert!(text.contains(&format!("@@ui_{key}@@")), "{key}");
        }
        assert!(!text.contains("desktop.shd"));
        for &s in crate::flow::SCREENS {
            let text = screen(&App::preview(s, Language::PtBr));
            assert!(!text.contains("ui.handler"));
            assert!(text.is_ascii());
        }
    }
}
