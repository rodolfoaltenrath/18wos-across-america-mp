//! Own UI preview/export. No game discovery, injection, sockets or save access.
use anyhow::{Result, ensure};
use clap::{ArgGroup, Parser, ValueEnum};
use mod_ui::{
    flow::{App, SCREENS, Screen},
    locale::Language,
};
use std::path::{Path, PathBuf};
#[cfg(windows)]
mod ui_preview;
#[derive(Clone, Copy, ValueEnum)]
enum Lang {
    PtBr,
    En,
}
#[derive(Parser)]
#[command(about="Prévia navegável das telas do mod, sem abrir jogo ou rede",group(ArgGroup::new("mode").args(["preview","render","export","self_test"])))]
struct Args {
    #[arg(long)]
    preview: bool,
    /// NEW folder of own offscreen BMP screenshots (Windows).
    #[arg(long)]
    render: Option<PathBuf>,
    /// NEW folder of layout JSON, Unicode catalogues and handler-free native ESC.
    #[arg(long)]
    export: Option<PathBuf>,
    #[arg(long)]
    self_test: bool,
    #[arg(long, requires = "self_test")]
    output: Option<PathBuf>,
    #[arg(long,default_value="main",value_parser=parse_screen)]
    screen: Screen,
    #[arg(long, value_enum, default_value = "pt-br")]
    language: Lang,
}
fn parse_screen(value: &str) -> Result<Screen, String> {
    SCREENS
        .iter()
        .copied()
        .find(|&s| mod_ui::native::name(s) == value)
        .ok_or_else(|| "unknown screen".into())
}
fn fresh(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create_new(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn main() -> Result<()> {
    let mut args = Args::parse();
    if !args.preview && !args.self_test && args.render.is_none() && args.export.is_none() {
        args.preview = true;
    }
    let app = App::preview(
        args.screen,
        match args.language {
            Lang::PtBr => Language::PtBr,
            Lang::En => Language::En,
        },
    );
    if let Some(dir) = args.export {
        std::fs::create_dir(&dir)?;
        for &screen in SCREENS {
            let a = App::preview(screen, app.language);
            let name = mod_ui::native::name(screen);
            fresh(
                &dir.join(format!("{name}.json")),
                &serde_json::to_vec_pretty(&mod_ui::view::View::build(&a))?,
            )?;
            fresh(
                &dir.join(format!("{name}.esc")),
                mod_ui::native::screen(&a).as_bytes(),
            )?;
        }
        for (name, lang) in [("pt-BR", Language::PtBr), ("en", Language::En)] {
            fresh(
                &dir.join(format!("{name}.json")),
                &serde_json::to_vec_pretty(&mod_ui::locale::catalogue(lang))?,
            )?;
        }
        fresh(&dir.join("PREVIEW.txt"),b"Own UI layouts. No game handler/network adapter. ESC uses ASCII fallback pending native font/codepage validation.\n")?;
        println!(
            "Exportadas {} telas; nenhuma conexao ou partida iniciada.",
            SCREENS.len()
        );
        return Ok(());
    }
    #[cfg(windows)]
    {
        if args.preview {
            return ui_preview::run(app);
        }
        if let Some(dir) = args.render {
            std::fs::create_dir(&dir)?;
            for &screen in SCREENS {
                let a = App::preview(screen, app.language);
                fresh(
                    &dir.join(format!("{}.bmp", mod_ui::native::name(screen))),
                    &ui_preview::bitmap(&a)?,
                )?;
            }
            println!(
                "{} telas renderizadas fora da tela; nao e captura do jogo.",
                SCREENS.len()
            );
            return Ok(());
        }
        if args.self_test {
            let report = ui_preview::self_test()?;
            let bytes = serde_json::to_vec_pretty(&report)?;
            if let Some(path) = args.output {
                fresh(&path, &bytes)?;
            }
            println!("{}", String::from_utf8(bytes)?);
            return Ok(());
        }
    }
    ensure!(
        !args.preview && !args.self_test && args.render.is_none(),
        "Graphical preview is Windows; export is portable. Native ESC integration is separate."
    );
    Ok(())
}
