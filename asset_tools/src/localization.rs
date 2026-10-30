//! Local-only language investigation and authored PT-BR catalogue tooling.
//! Original tables/fonts/audio remain private and are never needed by fixtures.
//! `build` writes a NEW mod project whose files are all `derived_local`: they
//! mix our translation with structure/pixels read from the local installation.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
pub mod catalog;
pub mod codec;
pub mod def;
pub mod font;
pub mod glyphs;

pub fn read_entry(archive: &Path, name: &str) -> Result<Vec<u8>> {
    crate::safe_relative(name)?;
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    ensure!(zip.len() <= 100_000, "archive entry limit");
    let mut found = None;
    for i in 0..zip.len() {
        if zip.by_index(i)?.name()?.as_ref() == name {
            ensure!(found.is_none(), "duplicate entry");
            found = Some(i);
        }
    }
    let mut f = zip.by_index(found.context("entry not found")?)?;
    ensure!(f.is_file() && f.size() <= 1024 * 1024, "entry size/type");
    let expected = f.size();
    let mut bytes = Vec::new();
    f.by_ref().take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 1024 * 1024 && bytes.len() as u64 == expected,
        "expanded size mismatch"
    );
    Ok(bytes)
}
pub fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Diagnostic only: escaped bytes cannot be mistaken for decoded glyph coverage.
#[derive(Serialize)]
pub struct Inspection {
    pub bytes: usize,
    pub sha256: String,
    pub utf8: bool,
    pub escaped_prefix: String,
}
pub fn inspect(bytes: &[u8], limit: usize) -> Inspection {
    Inspection {
        bytes: bytes.len(),
        sha256: sha(bytes),
        utf8: std::str::from_utf8(bytes).is_ok(),
        escaped_prefix: bytes
            .iter()
            .take(limit.min(4096))
            .flat_map(|b| std::ascii::escape_default(*b))
            .map(char::from)
            .collect(),
    }
}

/// Archives in ascending mount priority, as `aa/init.cfg` declares them
/// (`data.scs` p1, `aa/base.scs` p2, `aa/patch_1_10.scs` p3).
pub struct Mounts {
    archives: Vec<(PathBuf, Vec<String>)>,
}
impl Mounts {
    pub fn open(archives: &[PathBuf]) -> Result<Self> {
        ensure!((1..=8).contains(&archives.len()), "supply 1..=8 archives");
        let mut out = vec![];
        for path in archives {
            let mut zip = zip::ZipArchive::new(File::open(path)?)
                .with_context(|| format!("{} is not an archive", path.display()))?;
            ensure!(zip.len() <= 100_000, "archive entry limit");
            let mut names = Vec::with_capacity(zip.len());
            for i in 0..zip.len() {
                let f = zip.by_index(i)?;
                if f.is_file() {
                    names.push(f.name()?.into_owned());
                }
            }
            out.push((path.clone(), names));
        }
        Ok(Self { archives: out })
    }
    /// The visible file: highest-priority archive wins; 3nK envelopes decoded.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let (path, _) = self
            .archives
            .iter()
            .rev()
            .find(|(_, names)| names.iter().any(|n| n == name))
            .with_context(|| format!("{name} not found in mounted archives"))?;
        codec::decode(&read_entry(path, name)?)
    }
    pub fn names(&self, prefix: &str, ext: &str) -> Vec<String> {
        let mut all: Vec<String> = self
            .archives
            .iter()
            .flat_map(|(_, n)| n.iter())
            .filter(|n| {
                n.starts_with(prefix) && n.ends_with(ext) && !n[prefix.len()..].contains('/')
            })
            .cloned()
            .collect();
        all.sort();
        all.dedup();
        all
    }
}

pub const TABLE: &str = "scripts/local.def";
pub const SAMPLE: &str =
    "AÇÃO À VÊ ÔNIBUS ÕES ÚLTIMO ÉPOCA ÍNDICE ÓTIMO - ação à você ônibus põe útil é índice ótimo";

#[derive(Serialize)]
pub struct FontReport {
    pub path: String,
    pub added: Vec<String>,
    pub atlas: [usize; 2],
}
#[derive(Serialize)]
pub struct BuildReport {
    pub schema: u32,
    pub catalog: catalog::Report,
    pub fonts: Vec<FontReport>,
    pub skipped_fonts: Vec<String>,
    pub files: usize,
    pub game_tested: bool,
    pub text_outside_table_translated: bool,
}

#[derive(Serialize)]
struct ProjectFile {
    source: String,
    path: String,
    origin: &'static str,
    depends_on: Vec<String>,
}
#[derive(Serialize)]
struct Project {
    schema: u32,
    id: &'static str,
    version: &'static str,
    files: Vec<ProjectFile>,
}

fn create_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    fs::create_dir_all(path.parent().context("output parent")?)?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("{} already exists", path.display()))?;
    f.write_all(bytes)?;
    Ok(())
}

/// Shader variants (plain, 3D, shadow) of one font must share one atlas.
fn font_texture(
    mounts: &Mounts,
    font_path: &str,
    font: &font::Font,
) -> Result<(Vec<String>, String)> {
    let mut atlas = None;
    let mut shaders = vec![];
    for image in &font.images {
        let shd = image.trim_start_matches('/').to_string();
        let shd_text = String::from_utf8(mounts.read(&shd)?).context("font shader text")?;
        let tobj = shd_text
            .split("UI(")
            .nth(1)
            .and_then(|s| s.split(',').next())
            .context("font shader without UI()")?
            .trim();
        let dir = shd.rsplit_once('/').map_or("", |(d, _)| d);
        let tobj = format!("{dir}/{tobj}");
        ensure!(
            atlas.get_or_insert_with(|| tobj.clone()) == &tobj,
            "{font_path}: shader variants use different atlases"
        );
        shaders.push(shd);
    }
    let tobj = atlas.context("font without images")?;
    let tobj_text = String::from_utf8(mounts.read(&tobj)?)?;
    let tga = tobj_text
        .split_whitespace()
        .find(|w| w.to_ascii_lowercase().ends_with(".tga"))
        .context("texture object without TGA")?;
    ensure!(!tga.contains('/'), "texture outside the font directory");
    let dir = tobj.rsplit_once('/').map_or("", |(d, _)| d);
    let tga = format!("{dir}/{tga}");
    Ok((shaders, tga))
}

/// Writes a NEW mod project with the translated table and extended fonts.
/// Refuses incomplete markup/encoding; untranslated keys stay English.
pub fn build(mounts: &Mounts, cat: &catalog::Catalog, output: &Path) -> Result<BuildReport> {
    ensure!(!output.exists(), "output must be a NEW directory");
    let table = mounts.read(TABLE)?;
    let source = def::parse(&table)?;
    let report = catalog::check(cat, &source);
    ensure!(
        report.errors.is_empty() && report.stale.is_empty(),
        "catalogue has errors/stale entries; run check"
    );
    let values = cat
        .entries
        .iter()
        .map(|(k, e)| (k.clone(), e.text.clone()))
        .collect();
    let mut outputs: BTreeMap<String, (Vec<u8>, Vec<String>)> = BTreeMap::new();
    outputs.insert(TABLE.into(), (def::rebuild(&table, &values)?, vec![]));

    let mut fonts = vec![];
    let mut skipped = vec![];
    let sample = def::encode(SAMPLE)?;
    let mut previews = vec![];
    for path in mounts.names("fonts/", ".font") {
        // A font that cannot be resolved or extended keeps its original files.
        let extended = (|| -> Result<_> {
            let bytes = mounts.read(&path)?;
            let (shaders, tga) = font_texture(mounts, &path, &font::parse(&bytes)?)?;
            let atlas = glyphs::read_tga(&mounts.read(&tga)?)?;
            let s = glyphs::synthesize(&bytes, &atlas)?;
            ensure!(
                !s.added.is_empty() && s.missing_after.is_empty(),
                "cannot synthesize {}",
                s.missing_after.iter().collect::<String>()
            );
            let font_out = font::append(&bytes, &s.added)?;
            let resized = shaders
                .iter()
                .map(|shd| {
                    Ok((
                        shd.clone(),
                        glyphs::resize_shader(&mounts.read(shd)?, s.atlas.w, s.atlas.h)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let image = glyphs::preview(&font_out, &s.atlas, &sample, 4)?;
            Ok((font_out, shaders, resized, tga, s, image))
        })();
        let (font_out, shaders, resized, tga, s, image) = match extended {
            Ok(v) => v,
            Err(e) => {
                skipped.push(format!("{path}: {e:#}"));
                continue;
            }
        };
        for (shd, bytes) in resized {
            outputs.entry(shd).or_insert((bytes, vec![]));
        }
        previews.push((path.clone(), image));
        fonts.push(FontReport {
            path: path.clone(),
            added: s.added.keys().map(|k| char::from(*k).to_string()).collect(),
            atlas: [s.atlas.w, s.atlas.h],
        });
        ensure!(
            !outputs.contains_key(&tga),
            "{path}: atlas {tga} shared with another font"
        );
        outputs.insert(tga, (glyphs::write_tga(&s.atlas), vec![]));
        outputs.insert(path, (font_out, shaders));
    }

    fs::create_dir(output).context("creating output directory")?;
    let mut files = vec![];
    for (path, (bytes, deps)) in &outputs {
        let source = format!("inputs/local/{path}");
        create_new(&output.join(&source), bytes)?;
        files.push(ProjectFile {
            source,
            path: path.clone(),
            origin: "derived_local",
            depends_on: deps.clone(),
        });
    }
    let project = Project {
        schema: 1,
        id: "ptbr-text",
        version: "0.1.0",
        files,
    };
    create_new(
        &output.join("project.json"),
        &serde_json::to_vec_pretty(&project)?,
    )?;
    for (path, img) in previews {
        let name = path.trim_start_matches("fonts/").replace(".font", ".png");
        let file = output.join("preview").join(name);
        fs::create_dir_all(file.parent().context("preview dir")?)?;
        ensure!(!file.exists(), "preview exists");
        img.save(&file)?;
    }
    let result = BuildReport {
        schema: 1,
        catalog: report,
        fonts,
        skipped_fonts: skipped,
        files: outputs.len(),
        game_tested: false,
        text_outside_table_translated: false,
    };
    create_new(
        &output.join("l10n-report.json"),
        &serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn archive(path: &Path, files: &[(&str, Vec<u8>)]) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
    #[test]
    fn build_resolves_priority_and_writes_a_new_derived_project() {
        let t = tempfile::tempdir().unwrap();
        // Own 2-glyph font: 'a' and 'á' are enough to derive marks for a-letters.
        let mut atlas = glyphs::Atlas {
            w: 16,
            h: 8,
            rgba: vec![[255, 255, 255, 0]; 128],
        };
        for (x, y) in [
            (0, 2),
            (1, 2),
            (0, 3),
            (1, 3),
            (3, 0),
            (2, 2),
            (3, 2),
            (2, 3),
            (3, 3),
        ] {
            atlas.rgba[y * 16 + x][3] = 255;
        }
        let font = b"vert_span:6\nline_spacing:0\nimage:/fonts/own.shd\n$032,15,7,1,1,0,0,2\n$097,0,2,2,2,0,2,3\n$225,2,0,2,4,0,0,3\n".to_vec();
        let low = t.path().join("low.scs");
        let high = t.path().join("high.scs");
        archive(
            &low,
            &[
                (
                    "scripts/local.def",
                    b"def translate {\nk_a:\"Old\"\n}\n".to_vec(),
                ),
                ("fonts/own.font", font.clone()),
            ],
        );
        archive(
            &high,
            &[
                (
                    "scripts/local.def",
                    b"def translate {\n__fnt:\"face=own.font\"\nk_a:\"Action\"\nk_b:\"Keep\"\n}\n"
                        .to_vec(),
                ),
                ("fonts/own.shd", b"UI(own.tobj,16,8,999)\n".to_vec()),
                ("fonts/own.tobj", b"map\t2d\town.tga\r\n".to_vec()),
                ("fonts/own.tga", glyphs::write_tga(&atlas)),
            ],
        );
        let mounts = Mounts::open(&[low, high]).unwrap();
        let cat = catalog::Catalog {
            schema: 1,
            language: catalog::LANGUAGE.into(),
            source: TABLE.into(),
            entries: BTreeMap::from([(
                "k_a".to_string(),
                catalog::Entry {
                    src: catalog::source_hash("Action"),
                    text: "Ação".into(),
                },
            )]),
        };
        let out = t.path().join("project");
        // The own font has no capitals, so the font is reported, not guessed.
        let r = build(&mounts, &cat, &out).unwrap();
        assert_eq!(r.catalog.translated, 1);
        assert_eq!(r.catalog.untranslated, ["k_b"]);
        assert_eq!(r.skipped_fonts.len(), 1);
        assert!(!r.game_tested);
        let table = fs::read(out.join("inputs/local/scripts/local.def")).unwrap();
        assert!(def::decode(&table).contains("k_a:\"A\u{e7}\u{e3}o\"\nk_b:\"Keep\""));
        let project: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("project.json")).unwrap()).unwrap();
        assert_eq!(project["files"][0]["origin"], "derived_local");
        assert!(build(&mounts, &cat, &out).is_err());

        let mut stale = cat;
        stale.entries.get_mut("k_a").unwrap().src = catalog::source_hash("Old");
        assert!(build(&mounts, &stale, &t.path().join("other")).is_err());
        assert!(!t.path().join("other").exists());
    }
}
