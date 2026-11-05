use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

use crate::font::myanmar::{
    is_ignorable_myanmar_format_char, is_myanmar_char, is_myanmar_dependent_mark,
};
use crate::font::{FontStyle, FontSystem};

#[derive(Debug, Clone)]
pub struct EditorGraphemeCluster {
    pub byte_start: usize,
    pub byte_end: usize,
    pub char_start: usize,
    pub char_end: usize,
    pub text: String,
    pub is_myanmar: bool,
    pub width_px: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct EditorPositionedGlyph {
    pub font_id: usize,
    pub glyph_id: u16,
    pub x: f32,
    pub y_offset: f32,
    pub is_myanmar: bool,
    pub cluster_idx: usize,
}

#[derive(Debug, Clone)]
pub struct ShapedLine {
    pub text: String,
    pub clusters: Vec<EditorGraphemeCluster>,
    pub x_offsets: Vec<f32>,
    pub total_width: f32,
    pub glyphs: Vec<EditorPositionedGlyph>,
}

impl ShapedLine {
    pub fn empty() -> Self {
        Self {
            text: String::new(),
            clusters: Vec::new(),
            x_offsets: vec![0.0],
            total_width: 0.0,
            glyphs: Vec::new(),
        }
    }

    pub fn cluster_count(&self) -> usize {
        self.clusters.len()
    }

    pub fn visual_x_for_cluster(&self, cluster_idx: usize) -> f32 {
        if cluster_idx < self.x_offsets.len() {
            self.x_offsets[cluster_idx]
        } else {
            self.total_width
        }
    }

    pub fn cluster_at_visual_x(&self, x: f32) -> usize {
        if self.clusters.is_empty() || x <= 0.0 {
            return 0;
        }
        for i in 0..self.clusters.len() {
            let start_x = self.visual_x_for_cluster(i);
            let end_x = self.visual_x_for_cluster(i + 1);
            let mid_x = (start_x + end_x) * 0.5;
            if x <= mid_x {
                return i;
            } else if x < end_x {
                return i + 1;
            }
        }
        self.clusters.len()
    }

    pub fn char_to_cluster(&self, char_idx: usize) -> usize {
        for (i, c) in self.clusters.iter().enumerate() {
            if char_idx <= c.char_start {
                return i;
            }
            if char_idx < c.char_end {
                return i;
            }
        }
        self.clusters.len()
    }

    pub fn cluster_to_char_idx(&self, cluster_idx: usize) -> usize {
        if cluster_idx < self.clusters.len() {
            self.clusters[cluster_idx].char_start
        } else if let Some(last) = self.clusters.last() {
            last.char_end
        } else {
            0
        }
    }

    pub fn prev_cluster_char_range(&self, char_idx: usize) -> Option<(usize, usize)> {
        if self.clusters.is_empty() || char_idx == 0 {
            return None;
        }
        let cur_cluster = self.char_to_cluster(char_idx);
        if cur_cluster > 0 {
            let prev = &self.clusters[cur_cluster - 1];
            Some((prev.char_start, prev.char_end))
        } else {
            None
        }
    }

    pub fn next_cluster_char_range(&self, char_idx: usize) -> Option<(usize, usize)> {
        if self.clusters.is_empty() {
            return None;
        }
        let cur_cluster = self.char_to_cluster(char_idx);
        if cur_cluster < self.clusters.len() {
            let next = &self.clusters[cur_cluster];
            Some((next.char_start, next.char_end))
        } else {
            None
        }
    }
}

pub fn segment_myanmar_graphemes(line: &str) -> Vec<EditorGraphemeCluster> {
    if line.is_empty() {
        return Vec::new();
    }

    let mut raw_clusters: Vec<(usize, usize, &str)> = Vec::new();
    for (byte_idx, g) in line.grapheme_indices(true) {
        raw_clusters.push((byte_idx, byte_idx + g.len(), g));
    }

    let mut merged: Vec<(usize, usize, String, bool)> = Vec::new();

    let mut i = 0;
    while i < raw_clusters.len() {
        let (b_start, mut b_end, g_str) = raw_clusters[i];
        let mut cluster_text = g_str.to_string();
        let mut is_mm = cluster_text.chars().any(is_myanmar_char);

        // Myanmar virama / kinzi continuation:
        // If cluster ends with virama U+1039 or contains kinzi (U+1004 U+103A U+1039),
        // or next cluster starts with a dependent mark / subjoined consonant, join them.
        while i + 1 < raw_clusters.len() {
            let next_str = raw_clusters[i + 1].2;
            let next_starts_with_dep = next_str
                .chars()
                .next()
                .is_some_and(is_myanmar_dependent_mark);
            let cluster_ends_with_virama = cluster_text.ends_with('\u{1039}');
            let cluster_is_kinzi = cluster_text.ends_with("\u{1004}\u{103A}\u{1039}");

            if cluster_ends_with_virama || cluster_is_kinzi || next_starts_with_dep {
                i += 1;
                b_end = raw_clusters[i].1;
                cluster_text.push_str(raw_clusters[i].2);
                if cluster_text.chars().any(is_myanmar_char) {
                    is_mm = true;
                }
            } else {
                break;
            }
        }

        merged.push((b_start, b_end, cluster_text, is_mm));
        i += 1;
    }

    let mut char_offset = 0;
    let mut out = Vec::with_capacity(merged.len());
    for (b_start, b_end, text, is_mm) in merged {
        let char_count = text.chars().count();
        let char_start = char_offset;
        let char_end = char_offset + char_count;
        char_offset = char_end;

        out.push(EditorGraphemeCluster {
            byte_start: b_start,
            byte_end: b_end,
            char_start,
            char_end,
            text,
            is_myanmar: is_mm,
            width_px: 0.0,
        });
    }

    out
}

pub fn shape_and_measure_line(line: &str, font_sys: &mut FontSystem) -> ShapedLine {
    if line.is_empty() {
        return ShapedLine::empty();
    }

    let mut clusters = segment_myanmar_graphemes(line);
    let cell_w = font_sys.metrics.cell_width;

    let mut x_offsets = Vec::with_capacity(clusters.len() + 1);
    let mut glyphs = Vec::new();
    let mut cur_x = 0.0_f32;

    for (c_idx, cluster) in clusters.iter_mut().enumerate() {
        x_offsets.push(cur_x);

        if cluster.is_myanmar {
            let mut shaped_glyphs = Vec::new();
            let cluster_w;

            let first_char = cluster.text.chars().next().unwrap_or('က');
            let font_id = font_sys.resolve_font_for_char(first_char, FontStyle::Regular);

            let font = font_sys.font(font_id);
            let data: Arc<Vec<u8>> = Arc::clone(&font.data);
            let face_index = font.face_index;
            let units_per_em = font.units_per_em.max(1) as f32;
            let font_px = font_sys.metrics.myanmar_font_px;
            let scale = font_px / units_per_em;

            if let Some(face) = rustybuzz::Face::from_slice(&data, face_index) {
                let mut buffer = rustybuzz::UnicodeBuffer::new();
                for c in cluster.text.chars() {
                    if !is_ignorable_myanmar_format_char(c) {
                        buffer.push_str(&c.to_string());
                    }
                }
                buffer.set_script(rustybuzz::script::MYANMAR);
                buffer.set_direction(rustybuzz::Direction::LeftToRight);

                let output = rustybuzz::shape(&face, &font_sys.features, buffer);
                let infos = output.glyph_infos();
                let positions = output.glyph_positions();

                let mut rel_x = 0.0_f32;
                for (info, pos) in infos.iter().zip(positions.iter()) {
                    let gx = cur_x + rel_x + (pos.x_offset as f32 * scale);
                    let gy = pos.y_offset as f32 * scale;
                    shaped_glyphs.push(EditorPositionedGlyph {
                        font_id,
                        glyph_id: info.glyph_id as u16,
                        x: gx,
                        y_offset: gy,
                        is_myanmar: true,
                        cluster_idx: c_idx,
                    });
                    rel_x += pos.x_advance as f32 * scale;
                }
                cluster_w = rel_x.max(cell_w);
            } else {
                cluster_w = cell_w * 1.5;
            }

            cluster.width_px = cluster_w;
            cur_x += cluster_w;
            glyphs.extend(shaped_glyphs);
        } else {
            // ASCII / Monospace / Non-Myanmar
            let mut cluster_w = 0.0_f32;
            for ch in cluster.text.chars() {
                if ch == '\t' {
                    let tab_w = cell_w * 4.0;
                    cluster_w += tab_w;
                    continue;
                }
                let cw = if unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) > 1 {
                    cell_w * 2.0
                } else {
                    cell_w
                };
                let font_id = font_sys.resolve_font_for_char(ch, FontStyle::Regular);
                let gid = font_sys.font(font_id).glyph_id(ch);
                if gid != 0 {
                    glyphs.push(EditorPositionedGlyph {
                        font_id,
                        glyph_id: gid,
                        x: cur_x + cluster_w,
                        y_offset: 0.0,
                        is_myanmar: false,
                        cluster_idx: c_idx,
                    });
                }
                cluster_w += cw;
            }
            cluster.width_px = cluster_w;
            cur_x += cluster_w;
        }
    }

    x_offsets.push(cur_x);

    ShapedLine {
        text: line.to_string(),
        clusters,
        x_offsets,
        total_width: cur_x,
        glyphs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_ascii() {
        let clusters = segment_myanmar_graphemes("Hello 123");
        assert_eq!(clusters.len(), 9);
        assert_eq!(clusters[0].text, "H");
        assert_eq!(clusters[5].text, " ");
        assert_eq!(clusters[8].text, "3");
        for c in &clusters {
            assert!(!c.is_myanmar);
        }
    }

    #[test]
    fn test_segment_myanmar_kinzi() {
        // "မင်္ဂလာပါ" consists of:
        // 1. မ (U+1019)
        // 2. င်္ဂ (Kinzi: U+1004 U+103A U+1039 U+1002)
        // 3. လာ (U+101param: U+101C U+102C)
        // 4. ပါ (U+1018 U+102C)
        let clusters = segment_myanmar_graphemes("မင်္ဂလာပါ");
        assert_eq!(clusters.len(), 4);
        assert_eq!(clusters[0].text, "မ");
        assert_eq!(clusters[1].text, "င်္ဂ");
        assert_eq!(clusters[2].text, "လာ");
        assert_eq!(clusters[3].text, "ပါ");
        for c in &clusters {
            assert!(c.is_myanmar);
        }
    }

    #[test]
    fn test_segment_myanmar_virama_stacked() {
        // "သစ္စာ" (Truth)
        // 1. သ (U+101သ)
        // 2. စ္စ (U+1005 U+1039 U+1005)
        // 3. ာ (or combining vowel)
        let clusters = segment_myanmar_graphemes("သစ္စာ");
        assert_eq!(clusters[0].text, "သ");
        assert_eq!(clusters[1].text, "စ္စာ");
        assert!(clusters[1].text.contains('\u{1039}')); // Virama retained
    }

    #[test]
    fn test_segment_myanmar_medials() {
        // "မြန်မာ" (Myanmar)
        // 1. မြ (U+1019 U+103B)
        // 2. န် (U+1014 U+103A)
        // 3. မာ (U+1019 U+102C)
        let clusters = segment_myanmar_graphemes("မြန်မာ");
        assert_eq!(clusters.len(), 3);
        assert_eq!(clusters[0].text, "မြ");
        assert_eq!(clusters[1].text, "န်");
        assert_eq!(clusters[2].text, "မာ");
    }

    #[test]
    fn test_cluster_mapping_boundaries() {
        let line = ShapedLine {
            text: "Hello".to_string(),
            clusters: segment_myanmar_graphemes("Hello"),
            x_offsets: vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0],
            total_width: 50.0,
            glyphs: Vec::new(),
        };

        assert_eq!(line.visual_x_for_cluster(0), 0.0);
        assert_eq!(line.visual_x_for_cluster(1), 10.0);
        assert_eq!(line.visual_x_for_cluster(5), 50.0);

        assert_eq!(line.cluster_at_visual_x(0.0), 0);
        assert_eq!(line.cluster_at_visual_x(12.0), 1);
        assert_eq!(line.cluster_at_visual_x(48.0), 5);
    }
}
