//! Own native ESC generation. No original screen text or scripts are bundled.
//! Native actions require an adapter: generated MP screens have no game handler.
use crate::{
    flow::{App, Screen},
    locale::{self, Language},
    view::{Kind, View},
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
    let handler = handler
        .map(|s| format!("entity \"{s}\" {{ evalues {{ }} }}"))
        .unwrap_or_default();
    format!(
        "estruct {{\n client {{\n entities {{\n entity \"ui.window\" {{\n evalues {{\n \"wclass\" : \"edited\"\n \"name\" : \"window\"\n \"coords\" : \"0 0 1024 768\"\n \"fitting\" : \"true\"\n }}\n entities {{\n{children}\n }}\n }}\n{handler}\n }}\n }}\n}}\n"
    )
}
pub fn screen(app: &App) -> String {
    let v = View::build(app);
    let mut out = text(&v.title, 72, 590, 880, 60);
    out.push_str(&text(&v.subtitle, 72, 550, 880, 30));
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
    wrap(&out, None)
}
/// Only the proven legacy IDs are interactive. MP is an explicitly inactive label,
/// not an unknown ID sent to the original compiled handler.
pub fn main_preview(language: Language) -> String {
    let mut out = component(
        "ui.comp.text",
        &[
            ("layer", "-1".into()),
            (
                "text",
                "<img src=/shaders/ui/desktop.shd xscale=stretch yscale=stretch color=FF101923>"
                    .into(),
            ),
            ("coords", "0 0 1024 768".into()),
        ],
    );
    out.push_str(&text(locale::text("brand", language), 72, 640, 880, 70));
    out.push_str(&text(locale::text("main", language), 72, 580, 880, 44));
    for (i, (id, key, name)) in [
        (10, "career", "new game"),
        (20, "load", "load game"),
        (30, "settings", "options"),
        (40, "credits", "credits"),
        (50, "exit", "quit"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 470 - i as i32 * 60;
        out.push_str(&component(
            "ui.comp.common.button",
            &[
                ("look", "5".into()),
                ("value", locale::text(key, language).into()),
                ("layer", "2".into()),
                ("id", id.to_string()),
                ("name", name.into()),
                ("area", format!("72 {y} 540 {}", y + 40)),
                ("coords", format!("72 {y} 540 {}", y + 40)),
            ],
        ));
    }
    out.push_str(&text(
        locale::text("native.pending", language),
        600,
        410,
        380,
        60,
    ));
    out.push_str(&text(
        locale::text("native.note", language),
        72,
        80,
        880,
        30,
    ));
    wrap(&out, Some("ui.handler.menu.main"))
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
        for &s in crate::flow::SCREENS {
            let text = screen(&App::preview(s, Language::PtBr));
            assert!(!text.contains("ui.handler"));
            assert!(text.is_ascii());
        }
    }
}
