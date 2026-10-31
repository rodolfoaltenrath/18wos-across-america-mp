//! Local synthesis of missing PT-BR glyphs. Base letters and the acute accent
//! are copied from the same font; other marks are small pixel patterns sized
//! from that accent. The atlas doubles in height and new glyphs go below the
//! original pixels, which stay untouched. Appearance in-game is not proven here.
use super::font::{self, Glyph};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

/// Uncompressed 32-bit TGA, rows stored top-down in memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Atlas {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<[u8; 4]>,
}
pub fn read_tga(b: &[u8]) -> Result<Atlas> {
    ensure!(b.len() >= 18, "truncated TGA");
    let id = usize::from(b[0]);
    ensure!(
        b[1] == 0 && b[2] == 2 && b[16] == 32,
        "only uncompressed 32-bit TGA"
    );
    let w = usize::from(u16::from_le_bytes([b[12], b[13]]));
    let h = usize::from(u16::from_le_bytes([b[14], b[15]]));
    ensure!(
        (1..=4096).contains(&w) && (1..=4096).contains(&h),
        "TGA size"
    );
    let data = b
        .get(18 + id..18 + id + w * h * 4)
        .context("truncated TGA pixels")?;
    let top_down = b[17] & 0x20 != 0;
    let mut rgba = vec![[0; 4]; w * h];
    for (i, px) in data.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i % w, i / w);
        let y = if top_down { y } else { h - 1 - y };
        rgba[y * w + x] = [px[2], px[1], px[0], px[3]];
    }
    Ok(Atlas { w, h, rgba })
}
/// Writes the same layout the game ships: bottom-up rows, 8 alpha bits.
pub fn write_tga(a: &Atlas) -> Vec<u8> {
    let mut out = vec![0u8; 18];
    out[2] = 2;
    out[12..14].copy_from_slice(&(a.w as u16).to_le_bytes());
    out[14..16].copy_from_slice(&(a.h as u16).to_le_bytes());
    out[16] = 32;
    out[17] = 0x08;
    for y in (0..a.h).rev() {
        for x in 0..a.w {
            let [r, g, b, al] = a.rgba[y * a.w + x];
            out.extend_from_slice(&[b, g, r, al]);
        }
    }
    out
}

/// Alpha bitmap positioned relative to the pen (left/top as in the font).
#[derive(Clone, Debug)]
struct Ink {
    left: i32,
    top: i32,
    w: i32,
    h: i32,
    a: Vec<u8>,
}
impl Ink {
    fn at(&self, x: i32, y: i32) -> u8 {
        if (0..self.w).contains(&x) && (0..self.h).contains(&y) {
            self.a[(y * self.w + x) as usize]
        } else {
            0
        }
    }
    fn crop(atlas: &Atlas, g: &Glyph) -> Result<Ink> {
        ensure!(
            g.x >= 0
                && g.y >= 0
                && (g.x + g.w) as usize <= atlas.w
                && (g.y + g.h) as usize <= atlas.h,
            "glyph outside atlas"
        );
        let mut a = Vec::with_capacity((g.w * g.h) as usize);
        for y in 0..g.h {
            for x in 0..g.w {
                a.push(atlas.rgba[(g.y + y) as usize * atlas.w + (g.x + x) as usize][3]);
            }
        }
        Ok(Ink {
            left: g.left,
            top: g.top,
            w: g.w,
            h: g.h,
            a,
        })
    }
    /// Rows `[0, rows)` of `self`, trimmed to their ink.
    fn rows(&self, rows: i32) -> Option<Ink> {
        let lit = |x: i32, y: i32| self.at(x, y) > 0;
        let ys: Vec<i32> = (0..rows.min(self.h))
            .filter(|&y| (0..self.w).any(|x| lit(x, y)))
            .collect();
        let xs: Vec<i32> = (0..self.w)
            .filter(|&x| ys.iter().any(|&y| lit(x, y)))
            .collect();
        let (y0, y1, x0, x1) = (*ys.first()?, *ys.last()?, *xs.first()?, *xs.last()?);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let a = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| self.at(x0 + x, y0 + y))
            .collect();
        Some(Ink {
            left: self.left + x0,
            top: self.top + y0,
            w,
            h,
            a,
        })
    }
    fn mirrored(&self) -> Ink {
        let mut m = self.clone();
        for y in 0..self.h {
            for x in 0..self.w {
                m.a[(y * self.w + x) as usize] = self.at(self.w - 1 - x, y);
            }
        }
        m
    }
    fn pattern(rows: &[&str], scale: i32, alpha: u8) -> Ink {
        let (pw, ph) = (rows[0].len() as i32, rows.len() as i32);
        let (w, h) = (pw * scale, ph * scale);
        let a = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                if rows[(y / scale) as usize].as_bytes()[(x / scale) as usize] == b'#' {
                    alpha
                } else {
                    0
                }
            })
            .collect();
        Ink {
            left: 0,
            top: 0,
            w,
            h,
            a,
        }
    }
    fn union(parts: &[Ink]) -> Ink {
        let left = parts.iter().map(|p| p.left).min().unwrap_or(0);
        let top = parts.iter().map(|p| p.top).min().unwrap_or(0);
        let right = parts.iter().map(|p| p.left + p.w).max().unwrap_or(0);
        let bottom = parts.iter().map(|p| p.top + p.h).max().unwrap_or(0);
        let (w, h) = (right - left, bottom - top);
        let mut a = vec![0u8; (w * h) as usize];
        for p in parts {
            for y in 0..p.h {
                for x in 0..p.w {
                    let i = ((p.top - top + y) * w + p.left - left + x) as usize;
                    a[i] = a[i].max(p.at(x, y));
                }
            }
        }
        Ink { left, top, w, h, a }
    }
    fn peak(&self) -> u8 {
        self.a.iter().copied().max().unwrap_or(255).max(1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    Acute,
    Grave,
    Circumflex,
    Tilde,
    Diaeresis,
    Cedilla,
}
fn decompose(c: char) -> Option<(char, Mark)> {
    use Mark::*;
    Some(match c {
        'À' => ('A', Grave),
        'Á' => ('A', Acute),
        'Â' => ('A', Circumflex),
        'Ã' => ('A', Tilde),
        'Ç' => ('C', Cedilla),
        'É' => ('E', Acute),
        'Ê' => ('E', Circumflex),
        'Í' => ('I', Acute),
        'Ó' => ('O', Acute),
        'Ô' => ('O', Circumflex),
        'Õ' => ('O', Tilde),
        'Ú' => ('U', Acute),
        'Ü' => ('U', Diaeresis),
        'à' => ('a', Grave),
        'á' => ('a', Acute),
        'â' => ('a', Circumflex),
        'ã' => ('a', Tilde),
        'ç' => ('c', Cedilla),
        'é' => ('e', Acute),
        'ê' => ('e', Circumflex),
        'ó' => ('o', Acute),
        'ô' => ('o', Circumflex),
        'õ' => ('o', Tilde),
        'ú' => ('u', Acute),
        'ü' => ('u', Diaeresis),
        _ => return None,
    })
}

/// The acute accent of this font, taken from an existing accented letter.
struct Accent {
    ink: Ink,
    /// Bottom row (exclusive) of the accent band, relative to the pen.
    bottom: i32,
}
fn accent(
    atlas: &Atlas,
    glyphs: &BTreeMap<u8, Glyph>,
    accented: u8,
    plain: u8,
) -> Result<Option<Accent>> {
    let (Some(ga), Some(gp)) = (glyphs.get(&accented), glyphs.get(&plain)) else {
        return Ok(None);
    };
    let ink = Ink::crop(atlas, ga)?;
    let rows = gp.top - ga.top;
    Ok(ink.rows(rows).map(|ink| Accent {
        bottom: ink.top + ink.h,
        ink,
    }))
}

fn mark_ink(mark: Mark, acute: &Accent, base: &Ink) -> Ink {
    let band = acute.ink.h;
    let alpha = acute.ink.peak().max(base.peak());
    let big = band >= 3;
    // A one-pixel-wide acute mirrors onto itself, so tiny fonts get a drawn grave.
    let thin = acute.ink.w < 2;
    let rows: &[&str] = match (mark, big) {
        (Mark::Acute, _) => return acute.ink.clone(),
        (Mark::Grave, _) if !thin => return acute.ink.mirrored(),
        (Mark::Grave, _) => &["#.", ".#"],
        (Mark::Circumflex, false) => &[".#.", "#.#"],
        (Mark::Circumflex, true) => &["..#..", ".#.#.", "#...#"],
        // Rise, fall, rise: alternating pixels would read as a double acute.
        (Mark::Tilde, false) => &[".##.#", "#..#."],
        (Mark::Tilde, true) => &[".##..#", "#..##."],
        (Mark::Diaeresis, false) => &["#.#"],
        (Mark::Diaeresis, true) => &["##.##", "##.##"],
        (Mark::Cedilla, false) => &[".#", "##"],
        (Mark::Cedilla, true) => &[".#.", "..#", "##."],
    };
    let scale = (band / rows.len() as i32).max(1);
    let mut ink = Ink::pattern(rows, scale, alpha);
    // Horizontal centre of the base letter's ink.
    let lit: Vec<i32> = (0..base.w)
        .filter(|&x| (0..base.h).any(|y| base.at(x, y) > 0))
        .collect();
    let centre2 = base.left * 2
        + lit.first().copied().unwrap_or(0)
        + lit.last().copied().unwrap_or(base.w - 1);
    ink.left = (centre2 + 1 - ink.w) / 2;
    if mark == Mark::Cedilla {
        ink.top = base.top + base.h;
        // Hook sits under the centre: the pattern's stem is its second column.
        ink.left = (centre2 + 1) / 2 - scale;
    } else {
        ink.top = (acute.bottom - ink.h).max(0);
    }
    ink
}

pub struct Synthesis {
    pub added: BTreeMap<u8, Glyph>,
    pub atlas: Atlas,
    pub missing_after: Vec<char>,
}
/// Adds every PT-BR letter absent from `font_bytes`. Fonts without an
/// accented sample cannot be extended and are reported, not guessed.
pub fn synthesize(font_bytes: &[u8], atlas: &Atlas) -> Result<Synthesis> {
    let f = font::parse(font_bytes)?;
    let lower = accent(atlas, &f.glyphs, 0xe1, b'a')?;
    let upper = accent(atlas, &f.glyphs, 0xc1, b'A')?;
    let mut inks = BTreeMap::new();
    let mut missing_after = vec![];
    for c in super::def::PT_LETTERS.chars() {
        let slot = c as u32 as u8;
        if f.glyphs.contains_key(&slot) {
            continue;
        }
        let Some((base_char, mark)) = decompose(c) else {
            missing_after.push(c);
            continue;
        };
        let accent = if base_char.is_ascii_uppercase() {
            upper.as_ref().or(lower.as_ref())
        } else {
            lower.as_ref()
        };
        let (Some(accent), Some(bg)) = (accent, f.glyphs.get(&(base_char as u8))) else {
            missing_after.push(c);
            continue;
        };
        let base = Ink::crop(atlas, bg)?;
        let mark = mark_ink(mark, accent, &base);
        inks.insert(slot, (Ink::union(&[base, mark]), bg.advance));
    }
    // Pack below the original pixels, doubling the height until everything fits.
    let mut h = atlas.h;
    let placements = loop {
        h *= 2;
        ensure!(h <= 4096, "atlas cannot grow further");
        if let Some(p) = pack(&inks, atlas.w, atlas.h, h) {
            break p;
        }
    };
    let mut out = Atlas {
        w: atlas.w,
        h,
        rgba: atlas.rgba.clone(),
    };
    out.rgba.resize(atlas.w * h, [255, 255, 255, 0]);
    let mut added = BTreeMap::new();
    for (slot, (ink, advance)) in &inks {
        let (x, y) = placements[slot];
        for yy in 0..ink.h {
            for xx in 0..ink.w {
                out.rgba[(y + yy) as usize * out.w + (x + xx) as usize] =
                    [255, 255, 255, ink.at(xx, yy)];
            }
        }
        added.insert(
            *slot,
            Glyph {
                x,
                y,
                w: ink.w,
                h: ink.h,
                left: ink.left,
                top: ink.top,
                advance: *advance,
            },
        );
    }
    Ok(Synthesis {
        added,
        atlas: out,
        missing_after,
    })
}
fn pack(
    inks: &BTreeMap<u8, (Ink, i32)>,
    w: usize,
    y0: usize,
    h: usize,
) -> Option<BTreeMap<u8, (i32, i32)>> {
    let (mut x, mut y, mut row) = (1i32, y0 as i32 + 1, 0i32);
    let mut out = BTreeMap::new();
    for (slot, (ink, _)) in inks {
        if x + ink.w + 1 > w as i32 {
            x = 1;
            y += row + 1;
            row = 0;
        }
        if x + ink.w + 1 > w as i32 || y + ink.h + 1 > h as i32 {
            return None;
        }
        out.insert(*slot, (x, y));
        x += ink.w + 1;
        row = row.max(ink.h);
    }
    Some(out)
}

/// Replaces the reference height in `UI(name.tobj,W,H,prio)`.
pub fn resize_shader(shd: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(shd).context("font shader is not text")?;
    let start = text.find("UI(").context("font shader without UI()")?;
    let end = start + text[start..].find(')').context("unclosed UI()")?;
    let args: Vec<&str> = text[start + 3..end].split(',').map(str::trim).collect();
    ensure!(
        args.len() == 4 && args[0].ends_with(".tobj"),
        "unexpected UI() arguments"
    );
    let (ow, oh): (usize, usize) = (args[1].parse()?, args[2].parse()?);
    ensure!(
        ow == w && oh * 2 <= h,
        "shader reference size does not match atlas"
    );
    let call = format!("UI({},{},{},{})", args[0], w, h, args[3]);
    Ok(format!("{}{}{}", &text[..start], call, &text[end + 1..]).into_bytes())
}

/// Renders `text` (single-byte slots) for inspection, enlarged `scale` times.
pub fn preview(
    font_bytes: &[u8],
    atlas: &Atlas,
    text: &[u8],
    scale: u32,
) -> Result<image::RgbaImage> {
    ensure!(
        (1..=8).contains(&scale) && text.len() <= 4096,
        "preview scale/text limit"
    );
    let f = font::parse(font_bytes)?;
    let height = f.vert_span + 6;
    let mut pen = 2i32;
    let mut placed = vec![];
    let mut previous = None;
    let kerning: BTreeMap<_, _> = f
        .kerning
        .iter()
        .map(|k| ((k.left, k.right), k.amount))
        .collect();
    for b in text {
        let Some(g) = f.glyphs.get(b) else {
            bail!("slot {b} missing in preview")
        };
        if let Some(prev) = previous {
            pen += kerning.get(&(prev, *b)).copied().unwrap_or(0);
        }
        placed.push((pen, Ink::crop(atlas, g)?));
        pen += g.advance;
        previous = Some(*b);
    }
    let min_x = placed
        .iter()
        .map(|(x, g)| x + g.left)
        .min()
        .unwrap_or(0)
        .min(0);
    let max_x = placed
        .iter()
        .map(|(x, g)| x + g.left + g.w)
        .max()
        .unwrap_or(1)
        .max(pen);
    let (w, h) = (u32::try_from(max_x - min_x + 2)?, height as u32);
    ensure!(
        u64::from(w) * u64::from(h) * u64::from(scale).pow(2) <= 16 * 1024 * 1024,
        "preview pixel limit"
    );
    let mut img =
        image::RgbaImage::from_pixel(w * scale, h * scale, image::Rgba([24, 32, 48, 255]));
    for (x0, ink) in placed {
        for y in 0..ink.h {
            for x in 0..ink.w {
                let a = u32::from(ink.at(x, y));
                let (px, py) = (x0 + ink.left + x - min_x, 3 + ink.top + y);
                if a == 0 || px < 0 || py < 0 || px as u32 >= w || py as u32 >= h {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let p = img.get_pixel_mut(px as u32 * scale + sx, py as u32 * scale + sy);
                        for c in 0..3 {
                            p.0[c] = ((u32::from(p.0[c]) * (255 - a) + 255 * a) / 255) as u8;
                        }
                    }
                }
            }
        }
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Own 5x5 pixel font: A, a, C, c, E, e, I, O, o, U, u plus á and Á.
    fn fixture() -> (Vec<u8>, Atlas) {
        let mut atlas = Atlas {
            w: 64,
            h: 16,
            rgba: vec![[255, 255, 255, 0]; 64 * 16],
        };
        let mut lines = String::from("vert_span:9\nline_spacing:1\nimage:/fonts/own.shd\n");
        let glyphs: [(u8, i32, &[&str]); 14] = [
            (b' ', 0, &["."]),
            (b'A', 2, &[".#.", "#.#", "###", "#.#", "#.#"]),
            (b'C', 2, &["###", "#..", "#..", "#..", "###"]),
            (b'E', 2, &["###", "#..", "##.", "#..", "###"]),
            (b'I', 2, &["#", "#", "#", "#", "#"]),
            (b'O', 2, &["###", "#.#", "#.#", "#.#", "###"]),
            (b'U', 2, &["#.#", "#.#", "#.#", "#.#", "###"]),
            (b'a', 4, &["##", ".#", "##"]),
            (b'c', 4, &["##", "#.", "##"]),
            (b'e', 4, &["##", "##", "#."]),
            (b'o', 4, &["##", "##", "##"]),
            (b'u', 4, &["#.#", "#.#", "###"]),
            (0xe1, 2, &[".#", "..", "##", ".#", "##"]),
            (0xc1, 1, &["..#", ".#.", "#.#", "###", "#.#"]),
        ];
        let mut x = 0;
        for (slot, top, rows) in glyphs {
            let (w, h) = (rows[0].len() as i32, rows.len() as i32);
            for (yy, r) in rows.iter().enumerate() {
                for (xx, c) in r.bytes().enumerate() {
                    if c == b'#' {
                        atlas.rgba[yy * 64 + x as usize + xx][3] = 255;
                    }
                }
            }
            lines += &format!("${slot:03},{x},0,{w},{h},0,{top},{}\n", w + 1);
            x += w + 1;
        }
        (lines.into_bytes(), atlas)
    }
    #[test]
    fn tga_roundtrip_keeps_bottom_up_layout() {
        let (_, atlas) = fixture();
        let bytes = write_tga(&atlas);
        assert_eq!(bytes[17], 0x08);
        assert_eq!(read_tga(&bytes).unwrap(), atlas);
        let mut rle = bytes.clone();
        rle[2] = 10;
        assert!(read_tga(&rle).is_err());
        assert!(read_tga(&bytes[..100]).is_err());
    }
    #[test]
    fn missing_letters_are_synthesized_below_untouched_original_pixels() {
        let (font_bytes, atlas) = fixture();
        let s = synthesize(&font_bytes, &atlas).unwrap();
        assert_eq!(s.atlas.h, 32);
        assert_eq!(&s.atlas.rgba[..64 * 16], &atlas.rgba[..]);
        // Only í stays missing: it would need a dotless i, which the fixture lacks.
        assert_eq!(s.missing_after, ['í']);
        let tilde = &s.added[&0xe3];
        assert!(tilde.y >= 16 && tilde.top < 4 && tilde.advance == 3);
        let ced = &s.added[&0xc7];
        assert!(ced.top == 2 && ced.h > 5);
        let out = font::append(&font_bytes, &s.added).unwrap();
        let parsed = font::parse(&out).unwrap();
        for c in "ÃÇÊÕÜãçêõü".chars() {
            assert!(parsed.glyphs.contains_key(&(c as u32 as u8)), "{c}");
        }
        let encoded = super::super::def::encode("AÇÃO ação").unwrap();
        let img = preview(&out, &s.atlas, &encoded, 2).unwrap();
        assert_eq!(img.height(), (9 + 6) * 2);
        assert!(preview(&font_bytes, &atlas, &encoded, 1).is_err());
    }
    #[test]
    fn shader_reference_height_follows_the_atlas() {
        let shd = b"#define COLORGEN\n#include /templates/ui.tpl\n\nUI(small_0.tobj,64,128,999)\n";
        let out = resize_shader(shd, 64, 256).unwrap();
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("UI(small_0.tobj,64,256,999)\n")
        );
        assert!(resize_shader(shd, 128, 256).is_err());
        assert!(resize_shader(b"UI(x.tga,1,2,3)", 1, 4).is_err());
    }
    #[test]
    fn preview_uses_actual_advances_and_kerning_and_bounds_allocation() {
        let atlas = Atlas {
            w: 2,
            h: 2,
            rgba: vec![[255; 4]; 4],
        };
        let bytes = b"vert_span:8\nline_spacing:0\nimage:/fonts/own.shd\n$065,0,0,2,2,0,0,3\nkern:$065,$065,-1\n";
        let image = preview(bytes, &atlas, b"AA", 1).unwrap();
        assert_eq!(image.width(), 9); // two advances, one kern, explicit padding.
        assert!(preview(bytes, &atlas, b"AA", 0).is_err());
        assert!(preview(bytes, &atlas, &vec![b'A'; 4097], 1).is_err());
    }
}
