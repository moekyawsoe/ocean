use std::sync::Arc;

use rustc_hash::FxHashMap;
use rustybuzz::UnicodeBuffer;

use crate::config::ZawgyiMode;
use crate::font::myanmar::{
    is_ignorable_myanmar_format_char, is_myanmar_char, is_myanmar_dependent_mark,
    is_myanmar_run_char, maybe_convert_zawgyi, sanitize_and_canonicalize_myanmar,
};
use crate::font::{FontStyle, FontSystem};

#[inline]
fn is_effectively_empty_cell(cell: &CellInput<'_>) -> bool {
    (cell.c == ' ' || cell.c == '\0' || is_ignorable_myanmar_format_char(cell.c))
        && cell
            .zerowidth
            .iter()
            .copied()
            .all(is_ignorable_myanmar_format_char)
}

#[inline]
fn cell_myanmar_rep_char(cell: &CellInput<'_>) -> Option<char> {
    if is_myanmar_char(cell.c) {
        Some(cell.c)
    } else {
        cell.zerowidth
            .iter()
            .copied()
            .find(|&ch| is_myanmar_char(ch))
    }
}

#[derive(Debug, Clone)]
pub struct CellInput<'a> {
    pub c: char,
    pub zerowidth: &'a [char],
    pub style: FontStyle,
    pub is_wide_spacer: bool,
    pub is_box_drawing: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PositionedGlyph {
    pub font_id: usize,
    pub glyph_id: u16,
    pub is_myanmar: bool,
    /// X position relative to the start of the row (in physical pixels)
    pub x: f32,
    /// Y offset from baseline (positive = upward, in physical pixels)
    pub y_offset: f32,
    /// Horizontal fit scale (1.0 normally; < 1.0 only when a wide Myanmar consonant overflows cell span)
    pub x_scale: f32,
    /// Column index in the terminal row where this glyph's cluster originated (for fg color lookup)
    pub col: usize,
}

#[derive(Debug, Clone, Copy)]
struct CachedGlyph {
    glyph_id: u16,
    rel_x: f32,
    y_offset: f32,
    x_scale: f32,
    rel_col: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RunCacheKey {
    text: String,
    rel_cols: Vec<u16>,
    span_cols: u16,
    font_id: usize,
    is_myanmar: bool,
    zawgyi_rank: u8,
    cell_w_bits: u32,
    font_px_bits: u32,
}

pub struct TextShaper {
    cache: FxHashMap<RunCacheKey, Arc<Vec<CachedGlyph>>>,
}

impl Default for TextShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl TextShaper {
    pub fn new() -> Self {
        Self {
            cache: FxHashMap::with_capacity_and_hasher(2048, Default::default()),
        }
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Shape an entire terminal row into positioned GPU-ready glyphs.
    pub fn shape_row(
        &mut self,
        font_sys: &mut FontSystem,
        cells: &[CellInput<'_>],
        zawgyi_mode: ZawgyiMode,
        out: &mut Vec<PositionedGlyph>,
    ) {
        out.clear();
        if self.cache.len() > 8192 {
            self.cache.clear();
        }

        let num_cols = cells.len();
        let mut col = 0;

        while col < num_cols {
            let cell = &cells[col];
            if cell.is_wide_spacer || cell.is_box_drawing || is_effectively_empty_cell(cell) {
                col += 1;
                continue;
            }

            let first_style = cell.style;
            let first_mm_char = cell_myanmar_rep_char(cell);
            let first_is_mm = first_mm_char.is_some();
            let first_font =
                font_sys.resolve_font_for_char(first_mm_char.unwrap_or(cell.c), first_style);

            let start_col = col;
            let mut end_col = col + 1;

            // Absorb trailing wide-char spacer if `col` is a 2-cell wide char
            if end_col < num_cols && cells[end_col].is_wide_spacer {
                end_col += 1;
            }

            // Extend run across adjacent compatible cells
            while end_col < num_cols {
                let nc = &cells[end_col];
                if nc.is_box_drawing || is_effectively_empty_cell(nc) {
                    break;
                }
                if nc.is_wide_spacer {
                    end_col += 1;
                    continue;
                }

                let nc_mm_char = cell_myanmar_rep_char(nc);
                let nc_is_mm = if nc_mm_char.is_some() {
                    true
                } else if is_myanmar_run_char(nc.c)
                    || nc.zerowidth.iter().copied().any(is_myanmar_run_char)
                {
                    first_is_mm
                } else {
                    false
                };

                if nc_is_mm != first_is_mm {
                    break;
                }

                // Never split a Myanmar syllable mid-cluster across a style highlight boundary
                // when `nc` starts with a dependent vowel/medial/mark.
                let keep_across_style = first_is_mm
                    && (is_myanmar_dependent_mark(nc.c)
                        || ((nc.c == ' '
                            || nc.c == '\0'
                            || is_ignorable_myanmar_format_char(nc.c))
                            && nc_mm_char.is_some_and(is_myanmar_dependent_mark)));

                if nc.style != first_style && !keep_across_style {
                    break;
                }

                let nc_font = if first_is_mm {
                    match nc_mm_char {
                        Some(mm_ch) if !keep_across_style => {
                            font_sys.resolve_font_for_char(mm_ch, first_style)
                        }
                        _ => first_font,
                    }
                } else if is_ignorable_myanmar_format_char(nc.c) {
                    first_font
                } else {
                    font_sys.resolve_font_for_char(nc.c, first_style)
                };

                if nc_font != first_font {
                    break;
                }

                end_col += 1;
                if end_col < num_cols && cells[end_col].is_wide_spacer {
                    end_col += 1;
                }
            }

            self.shape_segment(
                font_sys,
                &cells[start_col..end_col],
                start_col,
                first_font,
                first_is_mm,
                zawgyi_mode,
                out,
            );

            col = end_col;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_segment(
        &mut self,
        font_sys: &FontSystem,
        seg_cells: &[CellInput<'_>],
        start_col: usize,
        font_id: usize,
        is_myanmar: bool,
        zawgyi_mode: ZawgyiMode,
        out: &mut Vec<PositionedGlyph>,
    ) {
        let span_cols = seg_cells.len() as u16;
        let mut raw_text = String::with_capacity(seg_cells.len() * 4);
        let mut rel_cols = Vec::with_capacity(seg_cells.len() * 2);

        for (rel_idx, cell) in seg_cells.iter().enumerate() {
            if cell.is_wide_spacer {
                continue;
            }
            let rc = rel_idx as u16;
            if cell.c != '\0' {
                raw_text.push(cell.c);
                rel_cols.push(rc);
            }
            for &zw in cell.zerowidth {
                raw_text.push(zw);
                rel_cols.push(rc);
            }
        }

        if raw_text.is_empty() {
            return;
        }

        let font_px = if is_myanmar {
            font_sys.metrics.myanmar_font_px
        } else {
            font_sys.metrics.font_px
        };
        let cell_w = font_sys.metrics.cell_width;

        let key = RunCacheKey {
            text: raw_text,
            rel_cols,
            span_cols,
            font_id,
            is_myanmar,
            zawgyi_rank: if is_myanmar { zawgyi_mode as u8 } else { 0 },
            cell_w_bits: cell_w.to_bits(),
            font_px_bits: font_px.to_bits(),
        };

        let cached = if let Some(hit) = self.cache.get(&key) {
            Arc::clone(hit)
        } else {
            let computed = Arc::new(shape_uncached(font_sys, &key, font_px, cell_w));
            self.cache.insert(key, Arc::clone(&computed));
            computed
        };

        let base_x = start_col as f32 * cell_w;
        for g in cached.iter() {
            out.push(PositionedGlyph {
                font_id,
                glyph_id: g.glyph_id,
                is_myanmar,
                x: base_x + g.rel_x,
                y_offset: g.y_offset,
                x_scale: g.x_scale,
                col: start_col + g.rel_col as usize,
            });
        }
    }
}

fn shape_uncached(
    font_sys: &FontSystem,
    key: &RunCacheKey,
    font_px: f32,
    cell_w: f32,
) -> Vec<CachedGlyph> {
    let loaded = font_sys.font(key.font_id);
    let Some(face) = loaded.rustybuzz_face() else {
        return Vec::new();
    };
    let scale = font_px / loaded.units_per_em as f32;

    // Prepare final text + byte->rel_col map
    let (shaped_text, char_rel_cols): (String, Vec<u16>) = if key.is_myanmar {
        let mode = match key.zawgyi_rank {
            1 => ZawgyiMode::Auto,
            2 => ZawgyiMode::Force,
            _ => ZawgyiMode::Off,
        };
        if let Some(converted) = maybe_convert_zawgyi(&key.text, mode) {
            let conv_chars: Vec<char> = converted.chars().collect();
            let total_c = conv_chars.len().max(1);
            let cols: Vec<u16> = (0..conv_chars.len())
                .map(|i| {
                    ((i * key.span_cols as usize) / total_c)
                        .min(key.span_cols.saturating_sub(1) as usize) as u16
                })
                .collect();
            (converted, cols)
        } else {
            let mut pairs: Vec<(char, usize)> = key
                .text
                .chars()
                .zip(key.rel_cols.iter().map(|&c| c as usize))
                .collect();
            sanitize_and_canonicalize_myanmar(&mut pairs);
            let s: String = pairs.iter().map(|(c, _)| *c).collect();
            let cols: Vec<u16> = pairs.iter().map(|(_, c)| *c as u16).collect();
            (s, cols)
        }
    } else {
        (key.text.clone(), key.rel_cols.clone())
    };

    if shaped_text.is_empty() {
        return Vec::new();
    }

    let mut byte_to_rel_col = vec![0u16; shaped_text.len() + 1];
    let mut byte_pos = 0;
    for (ch, &rc) in shaped_text.chars().zip(char_rel_cols.iter()) {
        let clen = ch.len_utf8();
        for b in byte_pos..byte_pos + clen {
            byte_to_rel_col[b] = rc;
        }
        byte_pos += clen;
    }
    if let Some(&last_rc) = char_rel_cols.last() {
        byte_to_rel_col[byte_pos] = last_rc;
    }

    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&shaped_text);
    buffer.guess_segment_properties();

    let glyph_buf = rustybuzz::shape(&face, &font_sys.features, buffer);
    let infos = glyph_buf.glyph_infos();
    let positions = glyph_buf.glyph_positions();

    let mut result = Vec::with_capacity(infos.len());

    if key.is_myanmar {
        // Continuous HarfBuzz OpenType `mym2`/`mymr` pen placement across the Myanmar run,
        // preserving natural cursive ligatures, mark attachments, and syllable transitions.
        let mut pen_x: i32 = 0;
        let mut pen_y: i32 = 0;
        let mut raw_glyphs = Vec::with_capacity(infos.len());

        for (info, pos) in infos.iter().zip(positions.iter()) {
            let gx = (pen_x + pos.x_offset) as f32 * scale;
            let gy = (pen_y + pos.y_offset) as f32 * scale;
            let rc = byte_to_rel_col
                .get(info.cluster as usize)
                .copied()
                .unwrap_or(0);
            raw_glyphs.push((info.glyph_id as u16, gx, gy, rc));
            pen_x += pos.x_advance;
            pen_y += pos.y_advance;
        }

        let natural_width = (pen_x as f32 * scale).max(1.0);
        let span_width = key.span_cols as f32 * cell_w;
        let fit_scale = if natural_width > span_width && span_width > 0.0 {
            (span_width / natural_width).max(0.55)
        } else {
            1.0
        };

        for (gid, gx, gy, rc) in raw_glyphs {
            if gid == 0 {
                continue;
            }
            result.push(CachedGlyph {
                glyph_id: gid,
                rel_x: gx * fit_scale,
                y_offset: gy,
                x_scale: fit_scale,
                rel_col: rc,
            });
        }
    } else {
        // Monospace / CJK / Symbol / Ligature grid-snapped cluster placement
        let mut i = 0;
        while i < infos.len() {
            let cluster = infos[i].cluster;
            let rc = byte_to_rel_col.get(cluster as usize).copied().unwrap_or(0);
            let cluster_base_x = rc as f32 * cell_w;
            let mut local_pen_x: i32 = 0;
            let mut local_pen_y: i32 = 0;

            while i < infos.len() && infos[i].cluster == cluster {
                let info = &infos[i];
                let pos = &positions[i];
                if info.glyph_id != 0 {
                    let gx = cluster_base_x + (local_pen_x + pos.x_offset) as f32 * scale;
                    let gy = (local_pen_y + pos.y_offset) as f32 * scale;
                    result.push(CachedGlyph {
                        glyph_id: info.glyph_id as u16,
                        rel_x: gx,
                        y_offset: gy,
                        x_scale: 1.0,
                        rel_col: rc,
                    });
                }
                local_pen_x += pos.x_advance;
                local_pen_y += pos.y_advance;
                i += 1;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn shapes_myanmar_syllables_without_dotted_circle() {
        let cfg = Config::default();
        let mut font_sys = FontSystem::new(&cfg, 1.0);
        let mm_font_id = font_sys.resolve_font_for_char('က', FontStyle::Regular);
        let dotted_gid = font_sys.font(mm_font_id).glyph_id('\u{25CC}');

        let mut shaper = TextShaper::new();
        let samples = [
            "မင်္ဂလာပါ",
            "ပြည်ထောင်စုသမ္မတမြန်မာနိုင်ငံတော်",
            "ကြွေး",
            "ဗုဒ္ဓ",
            "ဣန္ဒြေ",
            "မြို့",
            "ကောင်း",
            // Even swapped typing order (`ကုိ`, `ကေြ`, `ကာေ`, `မာျး`, `တေ်ာ`) must shape cleanly!
            "\u{1000}\u{102F}\u{102D}",
            "\u{1000}\u{1031}\u{103C}",
            "\u{1000}\u{102C}\u{1031}",
            "\u{1019}\u{102C}\u{103B}\u{1038}",
            "\u{1010}\u{1031}\u{103A}\u{102C}",
            // Exact AI CLI agent Burmese response sentence
            "သာလီစွပါ! ဒီနေ့ ပရောဂျက်နဲ့ ပတ်သက်ပြီး ဘာများ ကူညီဆောင်ရွက်ပေးရမလဲ ပြောပြပေးပါ။",
        ];

        for text in samples {
            // Simulate terminal cells exactly as `alacritty_terminal` populates them via `unicode_width`
            let mut cells_owned: Vec<(char, Vec<char>)> = Vec::new();
            for ch in text.chars() {
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if w == 0 {
                    if let Some(last) = cells_owned.last_mut() {
                        last.1.push(ch);
                    } else {
                        cells_owned.push((ch, Vec::new()));
                    }
                } else {
                    cells_owned.push((ch, Vec::new()));
                }
            }
            let inputs: Vec<CellInput<'_>> = cells_owned
                .iter()
                .map(|(c, zw)| CellInput {
                    c: *c,
                    zerowidth: zw.as_slice(),
                    style: FontStyle::Regular,
                    is_wide_spacer: false,
                    is_box_drawing: false,
                })
                .collect();

            let mut out = Vec::new();
            shaper.shape_row(&mut font_sys, &inputs, ZawgyiMode::Off, &mut out);
            assert!(!out.is_empty(), "empty shaped output for {text}");
            if dotted_gid != 0 {
                assert!(
                    out.iter().all(|g| g.glyph_id != dotted_gid),
                    "Dotted circle (gid={dotted_gid}) appeared when shaping `{text}`: {out:?}"
                );
            }
        }
    }

    #[test]
    fn shapes_ai_agent_tui_split_myanmar_graphemes_without_dotted_circles() {
        use crate::terminal::snapshot_from_bytes;

        let cfg = Config::default();
        let mut font_sys = FontSystem::new(&cfg, 1.0);
        let mm_font_id = font_sys.resolve_font_for_char('က', FontStyle::Regular);
        let dotted_gid = font_sys.font(mm_font_id).glyph_id('\u{25CC}');
        let mut shaper = TextShaper::new();

        let clean_line = "  သာလီစွပါ! ဒီနေ့ ပရောဂျက်နဲ့ ပတ်သက်ပြီး ဘာများ ကူညီဆောင်ရွက်ပေးရမလဲ ပြောပြပေးပါ။";

        // Shape clean reference via real `alacritty_terminal` VTE parser
        let clean_snap = snapshot_from_bytes(clean_line.as_bytes(), &cfg, 90, 4);
        let clean_inputs: Vec<CellInput<'_>> = clean_snap.cells[..90]
            .iter()
            .map(|rc| CellInput {
                c: rc.c,
                zerowidth: &rc.zerowidth,
                style: FontStyle::from_flags(rc.bold, rc.italic),
                is_wide_spacer: rc.wide_spacer,
                is_box_drawing: false,
            })
            .collect();
        let mut clean_glyphs = Vec::new();
        shaper.shape_row(
            &mut font_sys,
            &clean_inputs,
            ZawgyiMode::Off,
            &mut clean_glyphs,
        );
        assert!(!clean_glyphs.is_empty());

        // Simulate TUI cell serializers (Ink / xterm.js / agy) injecting zero-width separators
        // before split `SpacingMark` cells (`ေ`, `ျ`, `ြ`) as well as between every cell.
        for sep in [
            '\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}', '\u{034F}', '\u{00AD}',
            '\u{FE0E}',
        ] {
            let mut tui_stream = String::new();
            for ch in clean_line.chars() {
                if matches!(ch, '\u{1031}' | '\u{103B}' | '\u{103C}') {
                    tui_stream.push(sep);
                }
                tui_stream.push(ch);
            }
            let snap = snapshot_from_bytes(tui_stream.as_bytes(), &cfg, 90, 4);
            let inputs: Vec<CellInput<'_>> = snap.cells[..90]
                .iter()
                .map(|rc| CellInput {
                    c: rc.c,
                    zerowidth: &rc.zerowidth,
                    style: FontStyle::from_flags(rc.bold, rc.italic),
                    is_wide_spacer: rc.wide_spacer,
                    is_box_drawing: false,
                })
                .collect();

            let mut out = Vec::new();
            shaper.shape_row(&mut font_sys, &inputs, ZawgyiMode::Off, &mut out);
            if dotted_gid != 0 {
                assert!(
                    out.iter().all(|g| g.glyph_id != dotted_gid),
                    "Dotted circle appeared for TUI separator U+{:04X}: {out:?}",
                    sep as u32
                );
            }
            let clean_ids: Vec<(u16, i32)> = clean_glyphs
                .iter()
                .map(|g| (g.glyph_id, g.x.round() as i32))
                .collect();
            let got_ids: Vec<(u16, i32)> = out
                .iter()
                .map(|g| (g.glyph_id, g.x.round() as i32))
                .collect();
            assert_eq!(
                got_ids, clean_ids,
                "Shaped glyphs for TUI stream with U+{:04X} must match clean Burmese typography",
                sep as u32
            );
        }
    }
}
