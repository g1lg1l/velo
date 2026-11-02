//! A Text node's glyphs and where they go. Every glyph its words need, in Inter, JetBrains Mono
//! and the node's own font file, as written and in capitals, is a signed distance field in an
//! atlas; above it, in the same picture, the layout: each word and its glyphs' places, a float in
//! each texel's four bytes. Made once per text and font file on the media library's thread
//! (media.rs). text.wgsl breaks the lines and draws the glyphs, so size, tracking, line height,
//! wrap, outline, reveal and the rest are its parameters: moving them never sets the words again.

use std::collections::HashMap;

use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont};

/// The bundled fonts (OFL), shared with the interface rather than copied: the Font parameter's
/// first two choices. The node's own file is the third.
const FONTS: [&[u8]; 2] = [include_bytes!("../../velo/assets/fonts/Inter.ttf"), include_bytes!("../../velo/assets/fonts/JetBrainsMono.ttf")];
/// Atlas texels a layout unit takes. A unit is a line's height at size 1 (the Size parameter, in
/// canvas heights); a font's ascent to descent fills [`GLYPHS`] of it.
pub const UNIT: f32 = 100.0;
const GLYPHS: f32 = 0.8;
/// How far a glyph's field reaches past its edge, in texels: room for weight, outline and blur.
pub const SPREAD: u32 = 20;
/// The picture's width: the layout's rows, then the atlas'.
pub const WIDTH: u32 = 1024;
/// Distances are found at this many times the atlas' resolution.
const FINE: u32 = 4;
/// Floats in each record: the header, a section (a font in a case), a word, a glyph.
pub const RECORD: usize = 8;

/// `text` set in the bundled fonts and `file` (a font, if any): the picture text.wgsl reads.
///
/// The layout, in floats from the top left, in units unless said: the header (texels a unit
/// takes, the spread, how many sections, the atlas' first row), then a section for each font as
/// written and in capitals (its first word's float, how many words, how many characters, the
/// ascent). A word: its first glyph's float, how many glyphs, its advance, its characters, then
/// the spaces after it (their advance, how many), whether it starts a line, whether it starts one
/// of a Text List's items ([`velo_core::text_items`]: a line each, unless they're marked). A glyph: its cell from
/// the pen and the baseline (x, y, width, height), the cell in the atlas (texels), which character
/// of its word it is (tracking moves it that many times), which of the text (reveal shows it then).
pub fn set(text: &str, file: Option<&str>) -> Result<image::RgbaImage, String> {
    let mut fonts: Vec<FontArc> = FONTS.iter().map(|b| FontArc::try_from_slice(b).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
    if let Some(path) = file {
        let name = std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
        let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
        fonts.push(FontArc::try_from_vec(bytes).map_err(|_| format!("{name}: not a font (TrueType or OpenType)"))?);
    }
    let mut atlas = Atlas::default();
    let mut table = vec![UNIT, SPREAD as f32, (fonts.len() * 2) as f32];
    table.resize(RECORD * (1 + fonts.len() * 2), 0.0);
    for (f, font) in fonts.iter().enumerate() {
        let scaled = font.as_scaled(PxScale::from(UNIT * GLYPHS));
        for (upper, words) in [text.to_string(), text.to_uppercase()].iter().enumerate() {
            let (mut records, mut glyphs, mut index) = (Vec::new(), Vec::new(), 0);
            // each item's lines (an empty one is an empty line), the first word of each but the
            // very first starting a line, of each item an item
            let lines = velo_core::text_items(words).into_iter().enumerate().flat_map(|(k, item)| {
                let lines: Vec<&str> = if item.is_empty() { vec![""] } else { item.lines().collect() };
                lines.into_iter().enumerate().map(move |(l, line)| (k > 0 || l > 0, k > 0 && l == 0, line))
            });
            for (new_line, new_item, line) in lines {
                // a word is a run of letters and the spaces after it; a line starting with
                // spaces starts with an empty one
                let mut rest = line;
                loop {
                    let (word, after) = rest.split_at(rest.find(char::is_whitespace).unwrap_or(rest.len()));
                    let (gap, next) = after.split_at(after.find(|c: char| !c.is_whitespace()).unwrap_or(after.len()));
                    let (first, mut pen, mut prev, mut n) = (glyphs.len(), 0.0, None, 0);
                    for c in word.chars() {
                        let id = scaled.glyph_id(c);
                        pen += prev.map_or(0.0, |p| scaled.kern(p, id));
                        if let Some(cell) = atlas.cell(f, font, id) {
                            glyphs.push([(pen + cell.x) / UNIT, cell.y / UNIT, cell.w as f32 / UNIT, cell.h as f32 / UNIT, cell.at[0] as f32, cell.at[1] as f32, n as f32, index as f32]);
                        }
                        pen += scaled.h_advance(id);
                        (prev, n, index) = (Some(id), n + 1, index + 1);
                    }
                    let spaces = gap.chars().count();
                    let advance: f32 = gap.chars().map(|c| scaled.h_advance(scaled.glyph_id(c))).sum();
                    index += spaces;
                    let starts = rest.len() == line.len();
                    records.push([first as f32, (glyphs.len() - first) as f32, pen / UNIT, n as f32, advance / UNIT, spaces as f32, f32::from(u8::from(new_line && starts)), f32::from(u8::from(new_item && starts))]);
                    rest = next;
                    if rest.is_empty() {
                        break;
                    }
                }
            }
            let at = table.len();
            let glyphs_at = at + records.len() * RECORD;
            let s = RECORD * (1 + f * 2 + upper);
            table[s..s + 4].copy_from_slice(&[at as f32, records.len() as f32, index as f32, scaled.ascent() / UNIT]);
            for mut word in records {
                word[0] = (glyphs_at + word[0] as usize * RECORD) as f32;
                table.extend(word);
            }
            table.extend(glyphs.into_iter().flatten());
        }
    }
    let rows = table.len().div_ceil(WIDTH as usize) as u32;
    table[3] = rows as f32;
    let height = rows + atlas.y + atlas.row;
    if height > 8192 {
        return Err("too many different letters to set at once".into());
    }
    let mut picture = image::RgbaImage::new(WIDTH, height);
    for (i, v) in table.iter().enumerate() {
        picture.put_pixel(i as u32 % WIDTH, i as u32 / WIDTH, image::Rgba(v.to_le_bytes()));
    }
    for (cell, field) in &atlas.fields {
        for (i, &v) in field.iter().enumerate() {
            picture.put_pixel(cell.at[0] + i as u32 % cell.w, rows + cell.at[1] + i as u32 / cell.w, image::Rgba([255, 255, 255, v]));
        }
    }
    Ok(picture)
}

/// A glyph's square of the atlas: where it sits from the pen and the baseline (texels), its
/// size, and where it is in the atlas.
#[derive(Clone, Copy, Debug)]
struct Cell {
    x: f32,
    y: f32,
    w: u32,
    h: u32,
    at: [u32; 2],
}

/// The glyphs' fields, each made once (by font and glyph), packed in rows as they come.
#[derive(Default)]
struct Atlas {
    cells: HashMap<(usize, GlyphId), Option<Cell>>,
    fields: Vec<(Cell, Vec<u8>)>,
    x: u32,
    y: u32,
    row: u32,
}

impl Atlas {
    /// The glyph's cell, its field made the first time; none for a glyph with no outline (a space).
    fn cell(&mut self, f: usize, font: &FontArc, id: GlyphId) -> Option<Cell> {
        if let Some(cell) = self.cells.get(&(f, id)) {
            return *cell;
        }
        let made = field(font, id).map(|(mut cell, field)| {
            if self.x + cell.w > WIDTH {
                (self.x, self.y, self.row) = (0, self.y + self.row, 0);
            }
            cell.at = [self.x, self.y];
            (self.x, self.row) = (self.x + cell.w, self.row.max(cell.h));
            self.fields.push((cell, field));
            cell
        });
        self.cells.insert((f, id), made);
        made
    }
}

/// A glyph's signed distance field, 0.5 at its edge and more inside: its outline drawn at
/// [`FINE`] times the resolution, each pixel's distance to the nearest pixel across the edge,
/// averaged back down. Close enough (an eighth of a texel), and overlapping contours are one shape.
fn field(font: &FontArc, id: GlyphId) -> Option<(Cell, Vec<u8>)> {
    let outlined = |scale: f32| font.outline_glyph(id.with_scale(PxScale::from(UNIT * GLYPHS * scale)));
    let (coarse, fine) = (outlined(1.0)?, outlined(FINE as f32)?);
    let b = coarse.px_bounds();
    let (x, y) = (b.min.x as i32 - SPREAD as i32, b.min.y as i32 - SPREAD as i32);
    let (w, h) = (b.width() as u32 + 2 * SPREAD, b.height() as u32 + 2 * SPREAD);
    let (fw, fh) = ((w * FINE) as usize, (h * FINE) as usize);
    let mut inside = vec![false; fw * fh];
    let at = fine.px_bounds().min;
    let (ox, oy) = (at.x as i32 - x * FINE as i32, at.y as i32 - y * FINE as i32);
    fine.draw(|gx, gy, cover| {
        let (px, py) = ((ox + gx as i32) as usize, (oy + gy as i32) as usize);
        if cover > 0.5 && px < fw && py < fh {
            inside[py * fw + px] = true;
        }
    });
    let (to_inside, to_outside) = (distances(&inside, fw, fh, true), distances(&inside, fw, fh, false));
    // fine pixels from the edge, which runs between pixel centres; negative inside
    let signed = |i: usize| if inside[i] { 0.5 - to_outside[i].sqrt() } else { to_inside[i].sqrt() - 0.5 };
    let mut bytes = Vec::with_capacity((w * h) as usize);
    for cy in 0..h as usize {
        for cx in 0..w as usize {
            // a texel's middle is between the middle four of its fine pixels
            let (fx, fy) = (cx * FINE as usize + FINE as usize / 2 - 1, cy * FINE as usize + FINE as usize / 2 - 1);
            let d = (signed(fy * fw + fx) + signed(fy * fw + fx + 1) + signed((fy + 1) * fw + fx) + signed((fy + 1) * fw + fx + 1)) / (4 * FINE) as f32;
            bytes.push(((0.5 - d / (2 * SPREAD) as f32).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    Some((Cell { x: x as f32, y: y as f32, w, h, at: [0, 0] }, bytes))
}

/// Squared distances from each pixel to the nearest one that is (`of`) inside, by Felzenszwalb
/// and Huttenlocher's transform: down every column, then along every row.
fn distances(inside: &[bool], w: usize, h: usize, of: bool) -> Vec<f32> {
    let mut d: Vec<f32> = inside.iter().map(|&i| if i == of { 0.0 } else { 1e20 }).collect();
    let n = w.max(h);
    let (mut f, mut v, mut z) = (vec![0.0; n], vec![0; n], vec![0.0; n + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = d[y * w + x];
        }
        envelope(&f[..h], &mut v, &mut z, |y, e| d[y * w + x] = e);
    }
    for y in 0..h {
        f[..w].copy_from_slice(&d[y * w..(y + 1) * w]);
        envelope(&f[..w], &mut v, &mut z, |x, e| d[y * w + x] = e);
    }
    d
}

/// One line of the transform: the lower envelope of the parabolas rooted at each pixel.
fn envelope(f: &[f32], v: &mut [usize], z: &mut [f32], mut out: impl FnMut(usize, f32)) {
    let mut k = 0;
    (v[0], z[0], z[1]) = (0, f32::MIN, f32::MAX);
    for q in 1..f.len() {
        let s = loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2 * (q - p)) as f32;
            if s > z[k] {
                break s;
            }
            k -= 1;
        };
        k += 1;
        (v[k], z[k], z[k + 1]) = (q, s, f32::MAX);
    }
    k = 0;
    for q in 0..f.len() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        out(q, (q as f32 - v[k] as f32).powi(2) + f[v[k]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout's floats, read back from the picture.
    fn table(picture: &image::RgbaImage) -> Vec<f32> {
        picture.pixels().map(|p| f32::from_le_bytes(p.0)).collect()
    }

    #[test]
    fn words_glyphs_and_fields_are_laid_out() {
        let picture = set("VELO\nVE LO", None).unwrap();
        let t = table(&picture);
        assert_eq!((t[0], t[1], t[2]), (UNIT, SPREAD as f32, 4.0), "two fonts, each as written and in capitals");
        // Inter as written: three words, the second starting a line, the third after a space
        let s = &t[RECORD..RECORD * 2];
        let word = |i: usize| &t[s[0] as usize + i * RECORD..][..RECORD];
        assert_eq!((s[1], s[2]), (3.0, 9.0), "three words, nine characters but the newline");
        assert_eq!((word(0)[1], word(0)[3], word(0)[6]), (4.0, 4.0, 0.0));
        assert_eq!((word(1)[1], word(1)[5], word(1)[6]), (2.0, 1.0, 1.0), "VE, a space, starts a line");
        assert_eq!(word(2)[6], 0.0);
        assert!(word(1)[2] + word(1)[4] + word(2)[2] > word(0)[2], "a space wider");
        // a glyph: in its word, in the text; its cell in the atlas holds its field
        let o = &t[word(2)[0] as usize + RECORD..][..RECORD];
        assert_eq!((o[6], o[7]), (1.0, 8.0), "O: the second of LO, the ninth of the text");
        let (cx, cy) = (o[4] as u32, t[3] as u32 + o[5] as u32);
        let (cw, ch) = ((o[2] * UNIT).round() as u32, (o[3] * UNIT).round() as u32);
        let alpha = |x: u32, y: u32| picture.get_pixel(cx + x, cy + y).0[3];
        assert_eq!(alpha(0, 0), 0, "a corner is as far out as the field reaches");
        assert!(alpha(cw / 2, ch / 2) < 128, "the hole in the O is outside");
        assert!(alpha(SPREAD + 3, ch / 2) > 128, "its left stroke is inside");
        // capitals share the glyphs they already have; a font file that isn't there says so
        assert!(set("velo", None).unwrap().height() > set("VELO", None).unwrap().height());
        assert!(set("", None).is_ok());
        assert!(set("VELO", Some("no such font.ttf")).unwrap_err().contains("no such font.ttf"));
    }

    #[test]
    fn items_start_where_they_are_marked_and_run_over_lines() {
        let starts = |text: &str| {
            let t = table(&set(text, None).unwrap());
            let s = &t[RECORD..RECORD * 2];
            (0..s[1] as usize).map(|i| (t[s[0] as usize + i * RECORD + 6], t[s[0] as usize + i * RECORD + 7])).collect::<Vec<_>>()
        };
        // two items, the first over two lines: line breaks at each, an item only at the second
        assert_eq!(starts(&velo_core::join_items(&["A\nB", "C"])), [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
        // unmarked, a line is an item; an empty item is an empty line
        assert_eq!(starts("A\nB"), [(0.0, 0.0), (1.0, 1.0)]);
        assert_eq!(starts(&velo_core::join_items(&["A", "", "C"])).len(), 3);
    }
}
