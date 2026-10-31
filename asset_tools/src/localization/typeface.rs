//! Local TrueType/OpenType → the game's byte-indexed bitmap slots. All Latin
//! ink uses real outlines/coverage, while advances, kerning and line metrics
//! stay those of the original layout. No OS font/GDI/runtime dependency.
use super::{
    def,
    font::{self, Glyph},
    glyphs::Atlas,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

pub struct Typeface {
    font: fontdue::Font,
    pub sha256: String,
    pub name: String,
    strength: u8,
}
impl Typeface {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && (1..=16 * 1024 * 1024).contains(&metadata.len()),
            "typeface file size/type"
        );
        let mut bytes = vec![];
        file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == metadata.len(),
            "typeface changed while reading"
        );
        Self::from_bytes(bytes)
    }
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= 16 * 1024 * 1024,
            "typeface size"
        );
        let sha256 = super::sha(&bytes);
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(anyhow::Error::msg)?;
        for c in (b' '..=b'~').map(char::from).chain(def::PT_LETTERS.chars()) {
            ensure!(
                font.has_glyph(c),
                "typeface lacks required glyph U+{:04X}",
                c as u32
            );
        }
        let name = font
            .name()
            .unwrap_or("Local typeface")
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect();
        Ok(Self {
            font,
            sha256,
            name,
            strength: 40,
        })
    }
    pub fn with_strength(mut self, strength: u8) -> Result<Self> {
        ensure!(strength <= 100, "font strength outside 0..100");
        self.strength = strength;
        Ok(self)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Stats {
    pub typeface: String,
    pub source_sha256: String,
    pub coverage_strength: u8,
    pub pixels_per_em: f32,
    pub replaced_slots: usize,
    pub added_slots: usize,
    pub horizontally_fitted: usize,
    pub antialiased_pixels: usize,
    pub layout_metrics_preserved: bool,
}
pub struct Replacement {
    pub glyphs: BTreeMap<u8, Glyph>,
    pub atlas: Atlas,
    pub stats: Stats,
}
struct Ink {
    width: usize,
    height: usize,
    left: i32,
    top: i32,
    advance: i32,
    alpha: Vec<u8>,
}
fn base(c: char) -> char {
    match c {
        'À' | 'Á' | 'Â' | 'Ã' => 'A',
        'Ç' => 'C',
        'É' | 'Ê' => 'E',
        'Í' => 'I',
        'Ó' | 'Ô' | 'Õ' => 'O',
        'Ú' | 'Ü' => 'U',
        'à' | 'á' | 'â' | 'ã' => 'a',
        'ç' => 'c',
        'é' | 'ê' => 'e',
        'í' => 'i',
        'ó' | 'ô' | 'õ' => 'o',
        'ú' | 'ü' => 'u',
        _ => c,
    }
}
/// Area averaging preserves fractional edge coverage when ink must fit a
/// narrower legacy cell; never stretches a thin glyph to fill its advance.
fn narrow(alpha: &[u8], width: usize, height: usize, target: usize) -> Vec<u8> {
    let mut result = vec![0; target * height];
    for y in 0..height {
        for x in 0..target {
            // Integer overlaps in units of 1/target source pixels.
            let lo = x * width;
            let hi = (x + 1) * width;
            let mut sum = 0u64;
            for source in lo / target..hi.div_ceil(target) {
                let overlap = hi.min((source + 1) * target) - lo.max(source * target);
                sum += u64::from(alpha[y * width + source]) * overlap as u64;
            }
            result[y * target + x] = ((sum + width as u64 / 2) / width as u64) as u8;
        }
    }
    result
}
fn strengthen(alpha: u8, strength: u8) -> u8 {
    let a = u32::from(alpha);
    (a + (a * (255 - a) * u32::from(strength) + 12750) / 25500).min(255) as u8
}
pub fn replace(original: &[u8], atlas: &Atlas, face: &Typeface) -> Result<Replacement> {
    let old = font::parse(original)?;
    ensure!(
        atlas.w > 0
            && atlas.h > 0
            && atlas.w <= 4096
            && atlas.h <= 4096
            && atlas.rgba.len() == atlas.w * atlas.h,
        "invalid source atlas"
    );
    for g in old.glyphs.values() {
        ensure!(
            g.x >= 0 && g.y >= 0 && g.x + g.w <= atlas.w as i32 && g.y + g.h <= atlas.h as i32,
            "source glyph outside atlas"
        );
    }
    let reference = old
        .glyphs
        .get(&b'H')
        .or_else(|| old.glyphs.get(&b'A'))
        .context("no capital baseline reference")?;
    ensure!(
        reference.h >= 3 && reference.h <= old.vert_span,
        "invalid capital reference"
    );
    let baseline = reference.top + reference.h;
    let at = face.font.metrics('H', old.vert_span as f32);
    ensure!(at.height > 0, "empty capital outline");
    let mut px = old.vert_span as f32 * reference.h as f32 / at.height as f32;
    let chars: Vec<_> = (b' '..=b'~')
        .map(char::from)
        .chain(def::PT_LETTERS.chars())
        .collect();
    // Fit all printable Latin and accents into the existing line box together.
    // This preserves one common vertical scale instead of flattening accents.
    for attempt in 0..80 {
        let fits = chars.iter().all(|c| {
            let m = face.font.metrics(*c, px);
            m.height == 0
                || (baseline - m.ymin - m.height as i32 >= 0 && baseline - m.ymin <= old.vert_span)
        });
        if fits {
            break;
        }
        ensure!(
            attempt < 79 && px > 3.,
            "typeface cannot fit the original line box"
        );
        px *= 0.97;
    }
    let mut inks = BTreeMap::new();
    let mut fitted = 0;
    for c in chars {
        let slot = c as u32 as u8;
        let metric = old
            .glyphs
            .get(&slot)
            .or_else(|| old.glyphs.get(&(base(c) as u8)))
            .with_context(|| format!("missing legacy cell for U+{:04X}", c as u32))?;
        let (m, mut alpha) = face.font.rasterize(c, px);
        ensure!(
            m.width <= 1024 && m.height <= 512 && alpha.len() == m.width * m.height,
            "raster bounds"
        );
        let mut width = m.width;
        if width > 0 {
            let cell = metric.w.max(metric.advance - metric.left).max(1) as usize;
            if width > cell {
                alpha = narrow(&alpha, width, m.height, cell);
                width = cell;
                fitted += 1;
            }
        }
        for a in &mut alpha {
            *a = strengthen(*a, face.strength);
        }
        let left = if width == 0 {
            metric.left
        } else {
            metric.left + (metric.w.max(0) - width as i32).max(0) / 2
        };
        inks.insert(
            slot,
            Ink {
                width,
                height: m.height,
                left,
                top: baseline - m.ymin - m.height as i32,
                advance: metric.advance,
                alpha,
            },
        );
    }
    pack(
        atlas,
        &old,
        inks,
        Stats {
            typeface: face.name.clone(),
            source_sha256: face.sha256.clone(),
            coverage_strength: face.strength,
            pixels_per_em: px,
            replaced_slots: 0,
            added_slots: 0,
            horizontally_fitted: fitted,
            antialiased_pixels: 0,
            layout_metrics_preserved: true,
        },
    )
}
fn pack(
    atlas: &Atlas,
    old: &font::Font,
    inks: BTreeMap<u8, Ink>,
    mut stats: Stats,
) -> Result<Replacement> {
    let mut height = atlas.h;
    let mut placements = BTreeMap::new();
    loop {
        height *= 2;
        ensure!(height <= 4096, "replacement atlas exceeds 4096 pixels");
        let (mut x, mut y, mut row) = (1usize, atlas.h + 1, 0usize);
        placements.clear();
        for (slot, ink) in &inks {
            ensure!(ink.width + 2 <= atlas.w, "glyph wider than the atlas");
            if x + ink.width + 1 > atlas.w {
                x = 1;
                y += row + 2;
                row = 0;
            }
            placements.insert(*slot, (x, y));
            x += ink.width + 2;
            row = row.max(ink.height);
        }
        if y + row < height {
            break;
        }
    }
    let mut new_atlas = Atlas {
        w: atlas.w,
        h: height,
        rgba: atlas.rgba.clone(),
    };
    new_atlas.rgba.resize(atlas.w * height, [255, 255, 255, 0]);
    let mut glyphs = BTreeMap::new();
    for (slot, ink) in inks {
        let (x, y) = placements[&slot];
        for yy in 0..ink.height {
            for xx in 0..ink.width {
                let alpha = ink.alpha[yy * ink.width + xx];
                if (1..255).contains(&alpha) {
                    stats.antialiased_pixels += 1;
                }
                new_atlas.rgba[(y + yy) * atlas.w + x + xx] = [255, 255, 255, alpha];
            }
        }
        if old.glyphs.contains_key(&slot) {
            stats.replaced_slots += 1;
        } else {
            stats.added_slots += 1;
        }
        glyphs.insert(
            slot,
            Glyph {
                x: x as i32,
                y: y as i32,
                w: ink.width as i32,
                h: ink.height as i32,
                left: ink.left,
                top: ink.top,
                advance: ink.advance,
            },
        );
    }
    Ok(Replacement {
        glyphs,
        atlas: new_atlas,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coverage_is_averaged_without_binary_thresholds() {
        assert_eq!(narrow(&[0, 255, 255, 0], 4, 1, 2), vec![128, 128]);
        assert_eq!(narrow(&[255, 0, 0, 0, 255, 255], 3, 2, 1), vec![85, 170]);
        assert!(Typeface::from_bytes(vec![]).is_err());
        assert!(Typeface::from_bytes(b"own invalid font".to_vec()).is_err());
        assert_eq!(strengthen(0, 100), 0);
        assert_eq!(strengthen(255, 100), 255);
        let values: Vec<_> = (0..=255).map(|n| strengthen(n, 40)).collect();
        assert!(values.windows(2).all(|w| w[0] <= w[1]));
        assert!(values[128] > 128 && values[128] < 255);
    }
    #[test]
    fn packing_preserves_non_latin_pixels_and_metrics_with_own_coverage() {
        let bytes = b"vert_span:12\nline_spacing:2\nimage:/fonts/own.shd\n$065,0,0,2,2,0,0,3\n$001,3,0,1,1,0,0,1\nkern:$065,$065,-1\n";
        let old = font::parse(bytes).unwrap();
        let atlas = Atlas {
            w: 16,
            h: 8,
            rgba: vec![[70, 80, 90, 111]; 128],
        };
        let inks = BTreeMap::from([
            (
                65,
                Ink {
                    width: 3,
                    height: 3,
                    left: 0,
                    top: 1,
                    advance: 3,
                    alpha: vec![0, 128, 0, 128, 255, 128, 255, 128, 255],
                },
            ),
            (
                227,
                Ink {
                    width: 2,
                    height: 2,
                    left: 0,
                    top: 1,
                    advance: 3,
                    alpha: vec![0, 100, 255, 255],
                },
            ),
        ]);
        let r = pack(
            &atlas,
            &old,
            inks,
            Stats {
                typeface: "own fixture".into(),
                source_sha256: super::super::sha(b"own"),
                coverage_strength: 40,
                pixels_per_em: 9.,
                replaced_slots: 0,
                added_slots: 0,
                horizontally_fitted: 0,
                antialiased_pixels: 0,
                layout_metrics_preserved: true,
            },
        )
        .unwrap();
        assert_eq!(&r.atlas.rgba[..128], &atlas.rgba);
        assert!(r.stats.antialiased_pixels > 0);
        let out = font::replace(bytes, &r.glyphs).unwrap();
        let parsed = font::parse(&out).unwrap();
        assert_eq!(parsed.glyphs[&1], old.glyphs[&1]);
        assert_eq!(parsed.glyphs[&65].advance, 3);
        assert_eq!(parsed.kerns, 1);
        assert_eq!(parsed.vert_span, 12);
    }
}
