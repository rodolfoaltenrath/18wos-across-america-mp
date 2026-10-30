//! Own native ESC generation. No original screen text or scripts are bundled.
//! Native actions require an adapter: generated MP screens have no game handler.
use crate::{
    flow::{App, Screen},
    locale::{self, Language},
    view::{Kind, MAIN_PANEL, Rect, View, main_button_rect},
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
fn block(r: Rect, color: &str) -> String {
    component(
        "ui.comp.text",
        &[
            ("layer", "-1".into()),
            (
                "text",
                format!(
                    "<img src=/shaders/ui/desktop.shd xscale=stretch yscale=stretch color={color}>"
                ),
            ),
            ("coords", coords(r)),
        ],
    )
}
/// Only the proven legacy IDs are interactive. MP is an explicitly inactive label,
/// not an unknown ID sent to the original compiled handler.
pub fn main_preview(language: Language) -> String {
    let mut out = block(MAIN_PANEL, "CC101923");
    out.push_str(&text(
        locale::text("menu.title", language),
        47,
        444,
        280,
        28,
    ));
    for (i, (id, key, name)) in [
        (Some(10), "career", "new game"),
        (Some(20), "load", "load game"),
        (None, "native.pending", ""),
        (Some(30), "settings", "options"),
        (Some(40), "credits", "credits"),
        (Some(50), "exit", "quit"),
    ]
    .into_iter()
    .enumerate()
    {
        let r = main_button_rect(i);
        out.push_str(&block(
            r,
            if id.is_none() { "CC895D25" } else { "CC223344" },
        ));
        let Some(id) = id else {
            out.push_str(&text(
                locale::text(key, language),
                r.x,
                768 - r.y - r.h,
                r.w,
                r.h,
            ));
            continue;
        };
        out.push_str(&component(
            "ui.comp.common.button",
            &[
                ("look", "5".into()),
                ("value", locale::text(key, language).into()),
                ("layer", "2".into()),
                ("id", id.to_string()),
                ("name", name.into()),
                ("area", coords(r)),
                ("coords", coords(r)),
            ],
        ));
    }
    wrap_at(&out, Some("ui.handler.menu.main"), MAIN_PANEL)
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
        assert!(!text.contains("0 0 1024 768"));
        for &s in crate::flow::SCREENS {
            let text = screen(&App::preview(s, Language::PtBr));
            assert!(!text.contains("ui.handler"));
            assert!(text.is_ascii());
        }
    }
}
