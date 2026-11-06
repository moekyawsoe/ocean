use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::editor::buffer::TextBuffer;
use crate::editor::myanmar::{ShapedLine, shape_and_measure_line};
use crate::editor::syntax::{Language, SyntaxHighlighter, SyntaxTokenKind};
use crate::font::FontSystem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub cluster_idx: usize,
    pub preferred_visual_x: Option<u32>, // stored as bits to be Copy/Eq
}

impl Cursor {
    pub fn new(line: usize, cluster_idx: usize) -> Self {
        Self {
            line,
            cluster_idx,
            preferred_visual_x: None,
        }
    }

    pub fn get_pref_x(&self) -> Option<f32> {
        self.preferred_visual_x.map(f32::from_bits)
    }

    pub fn set_pref_x(&mut self, x: f32) {
        self.preferred_visual_x = Some(x.to_bits());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    Char,
    Line,
}

#[derive(Debug, Clone)]
pub struct CachedLine {
    pub shaped: Arc<ShapedLine>,
    pub cluster_tokens: Vec<SyntaxTokenKind>,
}

pub struct EditorDocument {
    pub buffer: TextBuffer,
    pub cursor: Cursor,
    pub extra_cursors: Vec<Cursor>,
    pub selection_anchor: Option<Cursor>,
    pub selection_kind: SelectionKind,
    pub scroll_y: usize,
    pub scroll_x: f32,
    pub language: Language,
    pub syntax: SyntaxHighlighter,
    pub display_name: Option<String>,
    pub scroll_to_cursor_requested: bool,
    line_cache: HashMap<usize, (u64, CachedLine)>,
}

impl Default for EditorDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorDocument {
    pub fn new() -> Self {
        let lang = Language::Plain;
        Self {
            buffer: TextBuffer::new(),
            cursor: Cursor::new(0, 0),
            extra_cursors: Vec::new(),
            selection_anchor: None,
            selection_kind: SelectionKind::Char,
            scroll_y: 0,
            scroll_x: 0.0,
            language: lang,
            syntax: SyntaxHighlighter::new(lang),
            display_name: None,
            scroll_to_cursor_requested: true,
            line_cache: HashMap::new(),
        }
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let buffer = TextBuffer::from_file(&path)?;
        let lang = Language::from_path(path);
        Ok(Self {
            buffer,
            cursor: Cursor::new(0, 0),
            extra_cursors: Vec::new(),
            selection_anchor: None,
            selection_kind: SelectionKind::Char,
            scroll_y: 0,
            scroll_x: 0.0,
            language: lang,
            syntax: SyntaxHighlighter::new(lang),
            display_name: None,
            scroll_to_cursor_requested: true,
            line_cache: HashMap::new(),
        })
    }

    pub fn from_text(text: &str, path: Option<PathBuf>, dirty: bool) -> Self {
        let language = path
            .as_ref()
            .map(Language::from_path)
            .unwrap_or(Language::Plain);
        let mut buffer = TextBuffer::from_str(text);
        buffer.path = path;
        buffer.dirty = dirty;
        Self {
            buffer,
            cursor: Cursor::new(0, 0),
            extra_cursors: Vec::new(),
            selection_anchor: None,
            selection_kind: SelectionKind::Char,
            scroll_y: 0,
            scroll_x: 0.0,
            language,
            syntax: SyntaxHighlighter::new(language),
            display_name: None,
            scroll_to_cursor_requested: true,
            line_cache: HashMap::new(),
        }
    }

    pub fn file_name(&self) -> String {
        self.display_name
            .clone()
            .unwrap_or_else(|| self.buffer.file_name())
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.dirty
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.buffer.path.as_ref()
    }

    pub fn cached_line(&mut self, line_idx: usize, font_sys: &mut FontSystem) -> &CachedLine {
        let rev = self.buffer.revision;
        let needs_build = match self.line_cache.get(&line_idx) {
            Some((cached_rev, _)) => *cached_rev != rev,
            None => true,
        };

        if needs_build {
            if self.line_cache.len() > 2048 {
                self.line_cache.clear();
            }

            let line_str = self.buffer.line_without_newline(line_idx);
            let shaped = Arc::new(shape_and_measure_line(&line_str, font_sys));
            let spans = self.syntax.highlight_line(&line_str);

            let mut cluster_tokens = Vec::with_capacity(shaped.cluster_count() + 1);
            let mut span_idx = 0;
            for cluster in &shaped.clusters {
                let char_idx = cluster.char_start;
                while span_idx < spans.len() && spans[span_idx].range.end <= char_idx {
                    span_idx += 1;
                }
                let kind = if span_idx < spans.len() && spans[span_idx].range.start <= char_idx {
                    spans[span_idx].kind
                } else {
                    SyntaxTokenKind::Plain
                };
                cluster_tokens.push(kind);
            }
            if cluster_tokens.is_empty() {
                cluster_tokens.push(SyntaxTokenKind::Plain);
            }

            self.line_cache.insert(
                line_idx,
                (
                    rev,
                    CachedLine {
                        shaped,
                        cluster_tokens,
                    },
                ),
            );
        }

        &self.line_cache.get(&line_idx).unwrap().1
    }

    pub fn shaped_line(&mut self, line_idx: usize, font_sys: &mut FontSystem) -> Arc<ShapedLine> {
        Arc::clone(&self.cached_line(line_idx, font_sys).shaped)
    }

    pub fn cursor_char_offset(&mut self, font_sys: &mut FontSystem) -> usize {
        let line = self
            .cursor
            .line
            .min(self.buffer.len_lines().saturating_sub(1));
        let cluster = self.cursor.cluster_idx;
        let shaped = self.shaped_line(line, font_sys);
        let col_char = shaped.cluster_to_char_idx(cluster);
        self.buffer.line_col_to_char(line, col_char)
    }

    /// Cursor position as LSP expects it: 0-based line and UTF-16 code units.
    pub fn cursor_utf16_position(&mut self, font_sys: &mut FontSystem) -> (u32, u32) {
        let line = self
            .cursor
            .line
            .min(self.buffer.len_lines().saturating_sub(1));
        let shaped = self.shaped_line(line, font_sys);
        let char_col = shaped.cluster_to_char_idx(self.cursor.cluster_idx);
        let text = self.buffer.line_without_newline(line);
        let utf16_col: usize = text.chars().take(char_col).map(char::len_utf16).sum();
        (line as u32, utf16_col as u32)
    }

    /// Convert an LSP UTF-16 column into the editor's shaped-cluster index.
    pub fn utf16_col_to_cluster(
        &mut self,
        line: usize,
        utf16_col: usize,
        font_sys: &mut FontSystem,
    ) -> usize {
        let line = line.min(self.buffer.len_lines().saturating_sub(1));
        let text = self.buffer.line_without_newline(line);
        let mut units = 0usize;
        let mut char_col = 0usize;
        for ch in text.chars() {
            if units >= utf16_col {
                break;
            }
            units += ch.len_utf16();
            char_col += 1;
        }
        let shaped = self.shaped_line(line, font_sys);
        shaped.char_to_cluster(char_col)
    }

    pub fn cursor_visual_pos(&mut self, font_sys: &mut FontSystem, gutter_w: f32) -> (f32, f32) {
        let line = self
            .cursor
            .line
            .min(self.buffer.len_lines().saturating_sub(1));
        let cluster = self.cursor.cluster_idx;
        let shaped = self.shaped_line(line, font_sys);
        let visual_x = shaped.visual_x_for_cluster(cluster);
        let line_h = font_sys.metrics.cell_height;
        let row_idx = line.saturating_sub(self.scroll_y);
        let x = (gutter_w + visual_x - self.scroll_x).max(0.0);
        let y = row_idx as f32 * line_h;
        (x, y)
    }

    pub fn insert_char(&mut self, c: char, font_sys: &mut FontSystem) {
        let mut s = String::new();
        s.push(c);
        self.insert_text(&s, font_sys);
    }

    pub fn insert_text(&mut self, text: &str, font_sys: &mut FontSystem) {
        self.delete_selection(font_sys);

        let before_cursor = (self.cursor.line, self.cursor.cluster_idx);
        let char_offset = self.cursor_char_offset(font_sys);

        let new_lines = text.matches('\n').count();
        self.buffer
            .insert(char_offset, text, before_cursor, before_cursor);

        if new_lines > 0 {
            self.cursor.line += new_lines;
            let after_newline = text.rsplit('\n').next().unwrap_or("");
            let shaped = self.shaped_line(self.cursor.line, font_sys);
            self.cursor.cluster_idx = shaped.char_to_cluster(after_newline.chars().count());
        } else {
            let line = self.cursor.line;
            let cur_cluster = self.cursor.cluster_idx;
            let shaped = self.shaped_line(line, font_sys);
            let char_in_line = shaped.cluster_to_char_idx(cur_cluster) + text.chars().count();
            let new_shaped = self.shaped_line(line, font_sys);
            self.cursor.cluster_idx = new_shaped.char_to_cluster(char_in_line);
        }

        self.cursor.preferred_visual_x = None;
        self.selection_anchor = None;
        self.scroll_to_cursor_requested = true;
    }

    pub fn caret_count(&self) -> usize {
        1 + self.extra_cursors.len()
    }

    pub fn clear_extra_cursors(&mut self) {
        self.extra_cursors.clear();
    }

    pub fn add_caret_at(&mut self, line: usize, cluster: usize) {
        let line = line.min(self.buffer.len_lines().saturating_sub(1));
        if self.cursor.line == line && self.cursor.cluster_idx == cluster {
            return;
        }
        if self
            .extra_cursors
            .iter()
            .any(|caret| caret.line == line && caret.cluster_idx == cluster)
        {
            return;
        }
        self.extra_cursors.push(Cursor::new(line, cluster));
    }

    pub fn add_caret_vertical(&mut self, delta: isize, font_sys: &mut FontSystem) {
        let target_line = self.cursor.line as isize + delta;
        if target_line < 0 {
            return;
        }
        let target_line = (target_line as usize).min(self.buffer.len_lines().saturating_sub(1));
        let shaped = self.shaped_line(target_line, font_sys);
        let cluster = self.cursor.cluster_idx.min(shaped.cluster_count());
        self.add_caret_at(target_line, cluster);
    }

    fn caret_offset(&mut self, caret: Cursor, font_sys: &mut FontSystem) -> usize {
        let line = caret.line.min(self.buffer.len_lines().saturating_sub(1));
        let shaped = self.shaped_line(line, font_sys);
        let char_col = shaped.cluster_to_char_idx(caret.cluster_idx);
        self.buffer.line_col_to_char(line, char_col)
    }

    fn caret_from_offset(&mut self, offset: usize, font_sys: &mut FontSystem) -> Cursor {
        let offset = offset.min(self.buffer.len_chars());
        let (line, col) = self.buffer.char_to_line_col(offset);
        let line = line.min(self.buffer.len_lines().saturating_sub(1));
        let shaped = self.shaped_line(line, font_sys);
        Cursor::new(line, shaped.char_to_cluster(col))
    }

    fn all_caret_offsets(&mut self, font_sys: &mut FontSystem) -> Vec<usize> {
        let mut offsets = vec![self.cursor_char_offset(font_sys)];
        let extras = self.extra_cursors.clone();
        for caret in extras {
            offsets.push(self.caret_offset(caret, font_sys));
        }
        offsets
    }

    fn apply_multi_offsets(
        &mut self,
        primary_offset: usize,
        extra_offsets: Vec<usize>,
        font_sys: &mut FontSystem,
    ) {
        self.cursor = self.caret_from_offset(primary_offset, font_sys);
        self.cursor.preferred_visual_x = None;
        self.extra_cursors = extra_offsets
            .into_iter()
            .map(|offset| self.caret_from_offset(offset, font_sys))
            .collect();
        self.scroll_to_cursor_requested = true;
    }

    pub fn insert_text_all(&mut self, text: &str, font_sys: &mut FontSystem) {
        if self.extra_cursors.is_empty() {
            self.insert_text(text, font_sys);
            return;
        }

        let primary = self.cursor_char_offset(font_sys);
        let mut offsets = self.all_caret_offsets(font_sys);
        offsets.sort_unstable();
        offsets.dedup();
        let Some(primary_rank) = offsets.iter().position(|offset| *offset == primary) else {
            self.insert_text(text, font_sys);
            return;
        };

        self.selection_anchor = None;
        self.extra_cursors.clear();
        let text_chars = text.chars().count();
        let before = (self.cursor.line, self.cursor.cluster_idx);
        for offset in offsets.iter().rev() {
            self.buffer.insert(*offset, text, before, before);
        }

        let new_offsets: Vec<usize> = offsets
            .iter()
            .enumerate()
            .map(|(rank, offset)| offset + text_chars * (rank + 1))
            .collect();
        let primary_offset = new_offsets[primary_rank];
        let extras: Vec<usize> = new_offsets
            .into_iter()
            .enumerate()
            .filter(|(rank, _)| *rank != primary_rank)
            .map(|(_, offset)| offset)
            .collect();
        self.apply_multi_offsets(primary_offset, extras, font_sys);
    }

    fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, end) in ranges {
            if start >= end {
                continue;
            }
            if let Some(last) = merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
                continue;
            }
            merged.push((start, end));
        }
        merged
    }

    fn offsets_after_deletions(offsets: &[usize], ranges: &[(usize, usize)]) -> Vec<usize> {
        let mut adjusted = Vec::with_capacity(ranges.len());
        let mut deleted = 0usize;
        for (start, end) in ranges {
            adjusted.push((*start, *end, start.saturating_sub(deleted)));
            deleted += end - start;
        }
        offsets
            .iter()
            .map(|offset| {
                if let Some((_, _, new_start)) = adjusted
                    .iter()
                    .find(|(start, end, _)| *offset >= *start && *offset < *end)
                {
                    *new_start
                } else {
                    let before: usize = adjusted
                        .iter()
                        .filter(|(_, end, _)| *end <= *offset)
                        .map(|(start, end, _)| end - start)
                        .sum();
                    offset.saturating_sub(before)
                }
            })
            .collect()
    }

    pub fn backspace_all(&mut self, font_sys: &mut FontSystem) {
        if self.extra_cursors.is_empty() {
            self.backspace(font_sys);
            return;
        }
        let mut offsets = self.all_caret_offsets(font_sys);
        offsets.sort_unstable();
        offsets.dedup();

        let mut ranges = Vec::new();
        for offset in &offsets {
            let (line, char_col) = self.buffer.char_to_line_col(*offset);
            let shaped = self.shaped_line(line, font_sys);
            if char_col > 0 {
                if let Some((start, end)) = shaped.prev_cluster_char_range(char_col) {
                    let line_start = self.buffer.line_col_to_char(line, 0);
                    ranges.push((line_start + start, line_start + end));
                }
            } else if *offset > 0 {
                ranges.push((offset - 1, *offset));
            }
        }
        let ranges = Self::merge_ranges(ranges);
        if ranges.is_empty() {
            return;
        }
        let primary = self.cursor_char_offset(font_sys);
        let new_offsets = Self::offsets_after_deletions(&offsets, &ranges);
        let primary_rank = offsets
            .iter()
            .position(|offset| *offset == primary)
            .unwrap_or(0);

        self.selection_anchor = None;
        let before = (self.cursor.line, self.cursor.cluster_idx);
        for (start, end) in ranges.iter().rev() {
            self.buffer.delete(*start, *end, before, before);
        }
        let primary_offset = new_offsets[primary_rank];
        let extras: Vec<usize> = new_offsets
            .into_iter()
            .enumerate()
            .filter(|(rank, _)| *rank != primary_rank)
            .map(|(_, offset)| offset)
            .collect();
        self.apply_multi_offsets(primary_offset, extras, font_sys);
    }

    pub fn delete_all(&mut self, font_sys: &mut FontSystem) {
        if self.extra_cursors.is_empty() {
            self.delete(font_sys);
            return;
        }
        let mut offsets = self.all_caret_offsets(font_sys);
        offsets.sort_unstable();
        offsets.dedup();

        let mut ranges = Vec::new();
        for offset in &offsets {
            let (line, char_col) = self.buffer.char_to_line_col(*offset);
            let shaped = self.shaped_line(line, font_sys);
            if let Some((start, end)) = shaped.next_cluster_char_range(char_col) {
                let line_start = self.buffer.line_col_to_char(line, 0);
                ranges.push((line_start + start, line_start + end));
            } else if *offset < self.buffer.len_chars() {
                ranges.push((*offset, *offset + 1));
            }
        }
        let ranges = Self::merge_ranges(ranges);
        if ranges.is_empty() {
            return;
        }
        let primary = self.cursor_char_offset(font_sys);
        let new_offsets = Self::offsets_after_deletions(&offsets, &ranges);
        let primary_rank = offsets
            .iter()
            .position(|offset| *offset == primary)
            .unwrap_or(0);

        self.selection_anchor = None;
        let before = (self.cursor.line, self.cursor.cluster_idx);
        for (start, end) in ranges.iter().rev() {
            self.buffer.delete(*start, *end, before, before);
        }
        let primary_offset = new_offsets[primary_rank];
        let extras: Vec<usize> = new_offsets
            .into_iter()
            .enumerate()
            .filter(|(rank, _)| *rank != primary_rank)
            .map(|(_, offset)| offset)
            .collect();
        self.apply_multi_offsets(primary_offset, extras, font_sys);
    }

    pub fn move_carets(&mut self, dx: isize, dy: isize, select: bool, font_sys: &mut FontSystem) {
        if self.extra_cursors.is_empty() {
            match (dx, dy) {
                (-1, 0) => self.move_left(select, font_sys),
                (1, 0) => self.move_right(select, font_sys),
                (0, -1) => self.move_up(select, font_sys),
                (0, 1) => self.move_down(select, font_sys),
                _ => {}
            }
            return;
        }

        if select && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !select {
            self.selection_anchor = None;
        }

        let mut carets = vec![self.cursor];
        carets.extend(self.extra_cursors.iter().copied());
        for caret in &mut carets {
            if dx < 0 {
                if caret.cluster_idx > 0 {
                    caret.cluster_idx -= 1;
                } else if caret.line > 0 {
                    caret.line -= 1;
                    let count = self.shaped_line(caret.line, font_sys).cluster_count();
                    caret.cluster_idx = count;
                }
            } else if dx > 0 {
                let count = self.shaped_line(caret.line, font_sys).cluster_count();
                if caret.cluster_idx < count {
                    caret.cluster_idx += 1;
                } else if caret.line + 1 < self.buffer.len_lines() {
                    caret.line += 1;
                    caret.cluster_idx = 0;
                }
            }
            if dy < 0 && caret.line > 0 {
                caret.line -= 1;
                let count = self.shaped_line(caret.line, font_sys).cluster_count();
                caret.cluster_idx = caret.cluster_idx.min(count);
            } else if dy > 0 && caret.line + 1 < self.buffer.len_lines() {
                caret.line += 1;
                let count = self.shaped_line(caret.line, font_sys).cluster_count();
                caret.cluster_idx = caret.cluster_idx.min(count);
            }
            caret.preferred_visual_x = None;
        }

        self.cursor = carets[0];
        self.extra_cursors = carets[1..].to_vec();
        self.scroll_to_cursor_requested = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn extra_caret_at_pixel(
        &mut self,
        px: f32,
        py: f32,
        vp_x: f32,
        content_y: f32,
        gutter_w: f32,
        line_h: f32,
        font_sys: &mut FontSystem,
    ) {
        let primary = self.cursor;
        let anchor = self.selection_anchor;
        self.set_cursor_from_pixel(px, py, vp_x, content_y, gutter_w, line_h, font_sys, false);
        let caret = self.cursor;
        self.cursor = primary;
        self.selection_anchor = anchor;
        self.add_caret_at(caret.line, caret.cluster_idx);
    }

    pub fn backspace(&mut self, font_sys: &mut FontSystem) {
        self.scroll_to_cursor_requested = true;
        if self.delete_selection(font_sys) {
            return;
        }

        let line = self.cursor.line;
        let cluster = self.cursor.cluster_idx;

        if cluster > 0 {
            let shaped = self.shaped_line(line, font_sys);
            let (char_start, char_end) = shaped
                .prev_cluster_char_range(shaped.cluster_to_char_idx(cluster))
                .unwrap_or((0, 0));
            if char_end > char_start {
                let line_start_char = self.buffer.line_col_to_char(line, 0);
                let del_start = line_start_char + char_start;
                let del_end = line_start_char + char_end;
                let before = (line, cluster);
                let after = (line, cluster - 1);
                self.buffer.delete(del_start, del_end, before, after);
                self.cursor.cluster_idx -= 1;
            }
        } else if line > 0 {
            // Join with previous line
            let prev_line = line - 1;
            let prev_line_len_clusters = self.shaped_line(prev_line, font_sys).cluster_count();
            let prev_line_end_char = self.buffer.line_col_to_char(
                prev_line,
                self.buffer.line_without_newline(prev_line).chars().count(),
            );
            let cur_line_start_char = self.buffer.line_col_to_char(line, 0);

            let before = (line, 0);
            let after = (prev_line, prev_line_len_clusters);
            self.buffer
                .delete(prev_line_end_char, cur_line_start_char, before, after);

            self.cursor.line = prev_line;
            self.cursor.cluster_idx = prev_line_len_clusters;
        }

        self.cursor.preferred_visual_x = None;
    }

    pub fn delete(&mut self, font_sys: &mut FontSystem) {
        self.scroll_to_cursor_requested = true;
        if self.delete_selection(font_sys) {
            return;
        }

        let line = self.cursor.line;
        let cluster = self.cursor.cluster_idx;
        let total_clusters = self.shaped_line(line, font_sys).cluster_count();

        if cluster < total_clusters {
            let shaped = self.shaped_line(line, font_sys);
            let (char_start, char_end) = shaped
                .next_cluster_char_range(shaped.cluster_to_char_idx(cluster))
                .unwrap_or((0, 0));
            if char_end > char_start {
                let line_start_char = self.buffer.line_col_to_char(line, 0);
                let del_start = line_start_char + char_start;
                let del_end = line_start_char + char_end;
                let before = (line, cluster);
                let after = (line, cluster);
                self.buffer.delete(del_start, del_end, before, after);
            }
        } else if line + 1 < self.buffer.len_lines() {
            // Join with next line
            let line_end_char = self
                .buffer
                .line_col_to_char(line, self.buffer.line_without_newline(line).chars().count());
            let next_line_start_char = self.buffer.line_col_to_char(line + 1, 0);
            let before = (line, cluster);
            let after = (line, cluster);
            self.buffer
                .delete(line_end_char, next_line_start_char, before, after);
        }

        self.cursor.preferred_visual_x = None;
    }

    pub fn delete_selection(&mut self, font_sys: &mut FontSystem) -> bool {
        let Some(anchor) = self.selection_anchor else {
            return false;
        };
        if anchor == self.cursor {
            self.selection_anchor = None;
            return false;
        }

        let (start, end) = self.selection_ordered_cursors();
        let start_col = self
            .shaped_line(start.line, font_sys)
            .cluster_to_char_idx(start.cluster_idx);
        let start_char = self.buffer.line_col_to_char(start.line, start_col);

        let end_col = self
            .shaped_line(end.line, font_sys)
            .cluster_to_char_idx(end.cluster_idx);
        let end_char = self.buffer.line_col_to_char(end.line, end_col);

        let before = (self.cursor.line, self.cursor.cluster_idx);
        let after = (start.line, start.cluster_idx);
        self.buffer.delete(start_char, end_char, before, after);

        self.cursor = start;
        self.cursor.preferred_visual_x = None;
        self.selection_anchor = None;
        self.scroll_to_cursor_requested = true;
        true
    }

    pub fn selected_text(&mut self, font_sys: &mut FontSystem) -> Option<String> {
        let anchor = self.selection_anchor?;
        if anchor == self.cursor {
            return None;
        }
        let (start, end) = self.selection_ordered_cursors();
        let start_col = self
            .shaped_line(start.line, font_sys)
            .cluster_to_char_idx(start.cluster_idx);
        let start_char = self.buffer.line_col_to_char(start.line, start_col);

        let end_col = self
            .shaped_line(end.line, font_sys)
            .cluster_to_char_idx(end.cluster_idx);
        let end_char = self.buffer.line_col_to_char(end.line, end_col);

        Some(self.buffer.slice_to_string(start_char, end_char))
    }

    pub fn selection_ordered_cursors(&self) -> (Cursor, Cursor) {
        let anchor = self.selection_anchor.unwrap_or(self.cursor);
        if anchor.line < self.cursor.line
            || (anchor.line == self.cursor.line && anchor.cluster_idx <= self.cursor.cluster_idx)
        {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        }
    }

    pub fn move_left(&mut self, select: bool, font_sys: &mut FontSystem) {
        self.scroll_to_cursor_requested = true;
        if select && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !select {
            self.selection_anchor = None;
        }

        if self.cursor.cluster_idx > 0 {
            self.cursor.cluster_idx -= 1;
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            let shaped = self.shaped_line(self.cursor.line, font_sys);
            self.cursor.cluster_idx = shaped.cluster_count();
        }
        self.cursor.preferred_visual_x = None;
    }

    pub fn move_right(&mut self, select: bool, font_sys: &mut FontSystem) {
        self.scroll_to_cursor_requested = true;
        if select && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !select {
            self.selection_anchor = None;
        }

        let total_clusters = self.shaped_line(self.cursor.line, font_sys).cluster_count();

        if self.cursor.cluster_idx < total_clusters {
            self.cursor.cluster_idx += 1;
        } else if self.cursor.line + 1 < self.buffer.len_lines() {
            self.cursor.line += 1;
            self.cursor.cluster_idx = 0;
        }
        self.cursor.preferred_visual_x = None;
    }

    pub fn move_up(&mut self, select: bool, font_sys: &mut FontSystem) {
        self.scroll_to_cursor_requested = true;
        if select && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !select {
            self.selection_anchor = None;
        }

        if self.cursor.line > 0 {
            let pref_x = match self.cursor.get_pref_x() {
                Some(px) => px,
                None => {
                    let cur_line = self.cursor.line;
                    let cur_cl = self.cursor.cluster_idx;
                    self.shaped_line(cur_line, font_sys)
                        .visual_x_for_cluster(cur_cl)
                }
            };

            self.cursor.line -= 1;
            let target_line = self.cursor.line;
            let cluster = self
                .shaped_line(target_line, font_sys)
                .cluster_at_visual_x(pref_x);
            self.cursor.cluster_idx = cluster;
            self.cursor.set_pref_x(pref_x);
        }
    }

    pub fn move_down(&mut self, select: bool, font_sys: &mut FontSystem) {
        self.scroll_to_cursor_requested = true;
        if select && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !select {
            self.selection_anchor = None;
        }

        if self.cursor.line + 1 < self.buffer.len_lines() {
            let pref_x = match self.cursor.get_pref_x() {
                Some(px) => px,
                None => {
                    let cur_line = self.cursor.line;
                    let cur_cl = self.cursor.cluster_idx;
                    self.shaped_line(cur_line, font_sys)
                        .visual_x_for_cluster(cur_cl)
                }
            };

            self.cursor.line += 1;
            let target_line = self.cursor.line;
            let cluster = self
                .shaped_line(target_line, font_sys)
                .cluster_at_visual_x(pref_x);
            self.cursor.cluster_idx = cluster;
            self.cursor.set_pref_x(pref_x);
        }
    }

    pub fn move_home(&mut self, select: bool, _font_sys: &mut FontSystem) {
        self.scroll_to_cursor_requested = true;
        if select && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !select {
            self.selection_anchor = None;
        }
        self.cursor.cluster_idx = 0;
        self.cursor.preferred_visual_x = None;
    }

    pub fn move_end(&mut self, select: bool, font_sys: &mut FontSystem) {
        self.scroll_to_cursor_requested = true;
        if select && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !select {
            self.selection_anchor = None;
        }
        let total = self.shaped_line(self.cursor.line, font_sys).cluster_count();
        self.cursor.cluster_idx = total;
        self.cursor.preferred_visual_x = None;
    }

    pub fn page_up(&mut self, lines: usize, select: bool, font_sys: &mut FontSystem) {
        for _ in 0..lines {
            self.move_up(select, font_sys);
        }
    }

    pub fn page_down(&mut self, lines: usize, select: bool, font_sys: &mut FontSystem) {
        for _ in 0..lines {
            self.move_down(select, font_sys);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_cursor_from_pixel(
        &mut self,
        px: f32,
        py: f32,
        view_x: f32,
        view_y: f32,
        gutter_w: f32,
        line_h: f32,
        font_sys: &mut FontSystem,
        select: bool,
    ) {
        if select && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        } else if !select {
            self.selection_anchor = None;
        }

        let rel_y = (py - view_y).max(0.0);
        let clicked_row = (rel_y / line_h).floor() as usize;
        let line = (self.scroll_y + clicked_row).min(self.buffer.len_lines().saturating_sub(1));

        let rel_x = (px - (view_x + gutter_w) + self.scroll_x).max(0.0);
        let cluster = self.shaped_line(line, font_sys).cluster_at_visual_x(rel_x);

        self.cursor.line = line;
        self.cursor.cluster_idx = cluster;
        self.cursor.preferred_visual_x = None;
        self.scroll_to_cursor_requested = false;
    }

    pub fn select_word_at_cursor(&mut self, font_sys: &mut FontSystem) {
        let line = self.cursor.line;
        let line_text = self.buffer.line_without_newline(line);
        let cur_cl = self.cursor.cluster_idx;
        let shaped = self.shaped_line(line, font_sys);
        let char_idx = shaped.cluster_to_char_idx(cur_cl);

        let chars: Vec<char> = line_text.chars().collect();
        if chars.is_empty() {
            return;
        }

        let idx = char_idx.min(chars.len().saturating_sub(1));
        let is_word_char =
            |c: char| c.is_alphanumeric() || c == '_' || crate::font::myanmar::is_myanmar_char(c);

        let mut start = idx;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }

        let mut end = idx;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }

        let cl_start = shaped.char_to_cluster(start);
        let cl_end = shaped.char_to_cluster(end);

        self.selection_anchor = Some(Cursor::new(line, cl_start));
        self.cursor = Cursor::new(line, cl_end);
    }

    pub fn select_line_at_cursor(&mut self, font_sys: &mut FontSystem) {
        let line = self.cursor.line;
        let cluster_count = self.shaped_line(line, font_sys).cluster_count();
        self.selection_anchor = Some(Cursor::new(line, 0));
        self.cursor = Cursor::new(line, cluster_count);
    }

    pub fn undo(&mut self, font_sys: &mut FontSystem) {
        if let Some((line, cluster)) = self.buffer.undo() {
            self.cursor.line = line.min(self.buffer.len_lines().saturating_sub(1));
            let total = self.shaped_line(self.cursor.line, font_sys).cluster_count();
            self.cursor.cluster_idx = cluster.min(total);
            self.selection_anchor = None;
            self.cursor.preferred_visual_x = None;
            self.scroll_to_cursor_requested = true;
        }
    }

    pub fn redo(&mut self, font_sys: &mut FontSystem) {
        if let Some((line, cluster)) = self.buffer.redo() {
            self.cursor.line = line.min(self.buffer.len_lines().saturating_sub(1));
            let total = self.shaped_line(self.cursor.line, font_sys).cluster_count();
            self.cursor.cluster_idx = cluster.min(total);
            self.selection_anchor = None;
            self.cursor.preferred_visual_x = None;
            self.scroll_to_cursor_requested = true;
        }
    }

    pub fn ensure_cursor_visible(
        &mut self,
        visible_lines: usize,
        view_w: f32,
        gutter_w: f32,
        font_sys: &mut FontSystem,
    ) {
        let line = self.cursor.line;
        if line < self.scroll_y {
            self.scroll_y = line;
        } else if line >= self.scroll_y + visible_lines && visible_lines > 0 {
            self.scroll_y = line - visible_lines + 1;
        }

        let cur_cl = self.cursor.cluster_idx;
        let cur_x = self
            .shaped_line(line, font_sys)
            .visual_x_for_cluster(cur_cl);

        let avail_w = (view_w - gutter_w - 40.0).max(100.0);
        if cur_x < self.scroll_x {
            self.scroll_x = (cur_x - 30.0).max(0.0);
        } else if cur_x > self.scroll_x + avail_w {
            self.scroll_x = cur_x - avail_w + 30.0;
        }
    }

    pub fn line_start_char(&self, line: usize) -> usize {
        let line = line.min(self.buffer.len_lines().saturating_sub(1));
        self.buffer.line_col_to_char(line, 0)
    }

    pub fn line_end_char(&self, line: usize) -> usize {
        let line = line.min(self.buffer.len_lines().saturating_sub(1));
        let col = self.buffer.line_without_newline(line).chars().count();
        self.buffer.line_col_to_char(line, col)
    }

    pub fn set_cursor_char_offset(&mut self, offset: usize, font_sys: &mut FontSystem) {
        let offset = offset.min(self.buffer.len_chars());
        let (line, col) = self.buffer.char_to_line_col(offset);
        let line = line.min(self.buffer.len_lines().saturating_sub(1));
        let shaped = self.shaped_line(line, font_sys);
        self.cursor = Cursor::new(line, shaped.char_to_cluster(col));
        self.cursor.preferred_visual_x = None;
        self.scroll_to_cursor_requested = true;
    }

    pub fn move_first_non_blank(&mut self, select: bool, font_sys: &mut FontSystem) {
        let line = self.cursor.line;
        let text = self.buffer.line_without_newline(line);
        let col = text.chars().position(|c| !c.is_whitespace()).unwrap_or(0);
        let shaped = self.shaped_line(line, font_sys);
        let cluster = shaped.char_to_cluster(col);
        if select {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
        } else {
            self.selection_anchor = None;
        }
        self.cursor = Cursor::new(line, cluster);
        self.cursor.preferred_visual_x = None;
        self.scroll_to_cursor_requested = true;
    }

    pub fn move_last_char(&mut self, select: bool, font_sys: &mut FontSystem) {
        let line = self.cursor.line;
        let shaped = self.shaped_line(line, font_sys);
        let cluster = shaped.cluster_count().saturating_sub(1);
        if select {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
        } else {
            self.selection_anchor = None;
        }
        self.cursor = Cursor::new(line, cluster);
        self.cursor.preferred_visual_x = None;
        self.scroll_to_cursor_requested = true;
    }

    pub fn move_to_line(&mut self, line: usize, font_sys: &mut FontSystem) {
        let line = line.min(self.buffer.len_lines().saturating_sub(1));
        self.cursor = Cursor::new(line, 0);
        self.cursor.preferred_visual_x = None;
        self.selection_anchor = None;
        self.move_first_non_blank(false, font_sys);
        self.scroll_to_cursor_requested = true;
    }

    pub fn move_word_forward(&mut self, count: usize, select: bool, font_sys: &mut FontSystem) {
        let mut offset = self.cursor_char_offset(font_sys);
        let total = self.buffer.len_chars();
        for _ in 0..count.max(1) {
            if offset < total && is_word_char(self.buffer.char_at(offset).unwrap_or('\n')) {
                while offset < total && is_word_char(self.buffer.char_at(offset).unwrap_or('\n')) {
                    offset += 1;
                }
            }
            while offset < total && !is_word_char(self.buffer.char_at(offset).unwrap_or('\n')) {
                offset += 1;
            }
        }
        self.navigate_to_offset(offset, select, font_sys);
    }

    pub fn move_word_backward(&mut self, count: usize, select: bool, font_sys: &mut FontSystem) {
        let mut offset = self.cursor_char_offset(font_sys);
        for _ in 0..count.max(1) {
            if offset == 0 {
                break;
            }
            offset -= 1;
            while offset > 0 && !is_word_char(self.buffer.char_at(offset).unwrap_or('\n')) {
                offset -= 1;
            }
            while offset > 0 && is_word_char(self.buffer.char_at(offset - 1).unwrap_or('\n')) {
                offset -= 1;
            }
        }
        self.navigate_to_offset(offset, select, font_sys);
    }

    pub fn move_word_end(&mut self, count: usize, select: bool, font_sys: &mut FontSystem) {
        let mut offset = self.cursor_char_offset(font_sys);
        let total = self.buffer.len_chars();
        for _ in 0..count.max(1) {
            if offset + 1 >= total {
                break;
            }
            offset += 1;
            while offset < total
                && !is_word_char(self.buffer.char_at(offset).unwrap_or('\n'))
                && self.buffer.char_at(offset) != Some('\n')
            {
                offset += 1;
            }
            while offset < total && is_word_char(self.buffer.char_at(offset).unwrap_or('\n')) {
                offset += 1;
            }
            offset = offset.saturating_sub(1);
        }
        self.navigate_to_offset(offset, select, font_sys);
    }

    fn navigate_to_offset(&mut self, offset: usize, select: bool, font_sys: &mut FontSystem) {
        if select {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
        } else {
            self.selection_anchor = None;
        }
        self.set_cursor_char_offset(offset, font_sys);
    }

    pub fn delete_char_range(
        &mut self,
        start: usize,
        end: usize,
        font_sys: &mut FontSystem,
    ) -> String {
        let total = self.buffer.len_chars();
        let start = start.min(total);
        let end = end.min(total);
        if end <= start {
            return String::new();
        }
        let cursor = (self.cursor.line, self.cursor.cluster_idx);
        let removed = self.buffer.delete(start, end, cursor, cursor);
        self.selection_anchor = None;
        self.selection_kind = SelectionKind::Char;
        self.set_cursor_char_offset(start, font_sys);
        removed
    }

    pub fn delete_lines(
        &mut self,
        start_line: usize,
        count: usize,
        font_sys: &mut FontSystem,
    ) -> String {
        let start_line = start_line.min(self.buffer.len_lines().saturating_sub(1));
        let end_line = (start_line + count.max(1)).min(self.buffer.len_lines());
        let start = self.line_start_char(start_line);
        let end = if end_line < self.buffer.len_lines() {
            self.line_start_char(end_line)
        } else {
            self.buffer.len_chars()
        };
        self.delete_char_range(start, end, font_sys)
    }

    pub fn delete_next_cluster(&mut self, font_sys: &mut FontSystem) -> String {
        let line = self.cursor.line;
        let shaped = self.shaped_line(line, font_sys);
        let cur_char = shaped.cluster_to_char_idx(self.cursor.cluster_idx);

        let (start_char, end_char) = if let Some((s, e)) = shaped.next_cluster_char_range(cur_char)
        {
            let start = self.buffer.line_col_to_char(line, s);
            let end = self.buffer.line_col_to_char(line, e);
            (start, end)
        } else if line + 1 < self.buffer.len_lines() {
            let start = self.line_end_char(line);
            (start, (start + 1).min(self.buffer.len_chars()))
        } else {
            return String::new();
        };

        self.delete_char_range(start_char, end_char, font_sys)
    }

    pub fn delete_prev_cluster(&mut self, font_sys: &mut FontSystem) -> String {
        let line = self.cursor.line;
        let shaped = self.shaped_line(line, font_sys);
        let cur_char = shaped.cluster_to_char_idx(self.cursor.cluster_idx);

        let (start_char, end_char) = if let Some((s, e)) = shaped.prev_cluster_char_range(cur_char)
        {
            let start = self.buffer.line_col_to_char(line, s);
            let end = self.buffer.line_col_to_char(line, e);
            (start, end)
        } else if line > 0 {
            let start = self.line_end_char(line - 1);
            (start, (start + 1).min(self.buffer.len_chars()))
        } else {
            return String::new();
        };

        self.delete_char_range(start_char, end_char, font_sys)
    }

    pub fn yank_lines(&self, start_line: usize, count: usize) -> String {
        let start_line = start_line.min(self.buffer.len_lines().saturating_sub(1));
        let end_line = (start_line + count.max(1)).min(self.buffer.len_lines());
        let start = self.line_start_char(start_line);
        let end = if end_line < self.buffer.len_lines() {
            self.line_start_char(end_line)
        } else {
            self.buffer.len_chars()
        };
        self.buffer.slice_to_string(start, end)
    }

    pub fn put_text(
        &mut self,
        text: &str,
        linewise: bool,
        before: bool,
        font_sys: &mut FontSystem,
    ) {
        if text.is_empty() {
            return;
        }
        if linewise {
            let line = self.cursor.line;
            let insert_at = if before {
                self.line_start_char(line)
            } else if line + 1 < self.buffer.len_lines() {
                self.line_start_char(line + 1)
            } else {
                self.buffer.len_chars()
            };
            let mut payload = text.to_string();
            if !payload.ends_with('\n') {
                payload.push('\n');
            }
            self.set_cursor_char_offset(insert_at, font_sys);
            self.insert_text(&payload, font_sys);
        } else {
            let line = self.cursor.line;
            let shaped = self.shaped_line(line, font_sys);
            let cursor_char = shaped.cluster_to_char_idx(self.cursor.cluster_idx);
            let mut offset = self.buffer.line_col_to_char(line, cursor_char);
            if !before {
                offset = if let Some((_, end)) = shaped.next_cluster_char_range(cursor_char) {
                    self.buffer.line_col_to_char(line, end)
                } else {
                    (offset + 1).min(self.buffer.len_chars())
                };
            }
            self.set_cursor_char_offset(offset, font_sys);
            self.insert_text(text, font_sys);
        }
    }

    pub fn join_lines(&mut self, count: usize, font_sys: &mut FontSystem) {
        for _ in 0..count.max(1) {
            let line = self.cursor.line;
            if line + 1 >= self.buffer.len_lines() {
                break;
            }
            let line_chars = self.buffer.line_without_newline(line).chars().count();
            let newline_pos = self.buffer.line_col_to_char(line, line_chars);
            let next_text = self.buffer.line_without_newline(line + 1);
            let next_len = next_text.chars().count();
            let trim = next_text.chars().take_while(|c| c.is_whitespace()).count();
            let mut end = self.buffer.line_col_to_char(line + 1, trim.min(next_len));
            end = end.min(self.buffer.len_chars());
            if end > newline_pos {
                let cursor = (self.cursor.line, self.cursor.cluster_idx);
                self.buffer.delete(newline_pos, end, cursor, cursor);
            }
            if line_chars > 0 && self.buffer.char_at(newline_pos) != Some(' ') {
                self.set_cursor_char_offset(newline_pos, font_sys);
                self.insert_text(" ", font_sys);
            }
            self.set_cursor_char_offset(newline_pos, font_sys);
        }
    }

    pub fn indent_lines(
        &mut self,
        start_line: usize,
        count: usize,
        dedent: bool,
        font_sys: &mut FontSystem,
    ) {
        let end_line = (start_line + count.max(1)).min(self.buffer.len_lines());
        for line in start_line..end_line {
            let start = self.line_start_char(line);
            if dedent {
                let text = self.buffer.line_without_newline(line);
                let remove = text.chars().take(4).take_while(|c| *c == ' ').count();
                if remove > 0 {
                    let cursor = (self.cursor.line, self.cursor.cluster_idx);
                    self.buffer.delete(start, start + remove, cursor, cursor);
                }
            } else {
                let cursor = (self.cursor.line, self.cursor.cluster_idx);
                self.buffer.insert(start, "    ", cursor, cursor);
            }
        }
        self.set_cursor_char_offset(self.line_start_char(start_line), font_sys);
        self.move_first_non_blank(false, font_sys);
    }

    pub fn replace_char(&mut self, ch: char, font_sys: &mut FontSystem) {
        let offset = self.cursor_char_offset(font_sys);
        if offset >= self.buffer.len_chars() {
            return;
        }
        let end = (offset + 1).min(self.buffer.len_chars());
        self.delete_char_range(offset, end, font_sys);
        self.set_cursor_char_offset(offset, font_sys);
        self.insert_text(&ch.to_string(), font_sys);
        self.set_cursor_char_offset(offset, font_sys);
    }

    pub fn toggle_case(&mut self, font_sys: &mut FontSystem) {
        let offset = self.cursor_char_offset(font_sys);
        let Some(ch) = self.buffer.char_at(offset) else {
            return;
        };
        let replaced = if ch.is_uppercase() {
            ch.to_lowercase().next().unwrap_or(ch)
        } else {
            ch.to_uppercase().next().unwrap_or(ch)
        };
        let end = (offset + 1).min(self.buffer.len_chars());
        self.delete_char_range(offset, end, font_sys);
        self.set_cursor_char_offset(offset, font_sys);
        self.insert_text(&replaced.to_string(), font_sys);
        self.set_cursor_char_offset(offset, font_sys);
    }

    pub fn matching_bracket(
        &mut self,
        font_sys: &mut FontSystem,
    ) -> Option<((usize, usize), (usize, usize))> {
        let line = self.cursor.line;
        let text = self.buffer.line_without_newline(line);
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return None;
        }
        let shaped = self.shaped_line(line, font_sys);
        let cursor_char = shaped.cluster_to_char_idx(self.cursor.cluster_idx);

        let idx = if cursor_char < chars.len() && is_bracket(chars[cursor_char]) {
            cursor_char
        } else if cursor_char > 0 && is_bracket(chars[cursor_char - 1]) {
            cursor_char - 1
        } else {
            return None;
        };

        let ch = chars[idx];
        let (open, close, direction) = match ch {
            '(' => ('(', ')', 1),
            '[' => ('[', ']', 1),
            '{' => ('{', '}', 1),
            ')' => ('(', ')', -1),
            ']' => ('[', ']', -1),
            '}' => ('{', '}', -1),
            _ => return None,
        };

        let mut depth = 0i32;
        let mut current = idx as isize;
        loop {
            let c = *chars.get(current as usize)?;
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            current += direction;
            if current < 0 || current as usize >= chars.len() {
                return None;
            }
        }

        let other = current as usize;
        let a = shaped.char_to_cluster(idx);
        let b = shaped.char_to_cluster(other);
        Some(((line, a), (line, b)))
    }

    /// Inclusive character range for the current Visual selection.
    pub fn visual_char_range(&mut self, font_sys: &mut FontSystem) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        let (start, end) = self.selection_ordered_cursors();

        if self.selection_kind == SelectionKind::Line {
            let start_char = self.line_start_char(start.line);
            let end_char = if end.line + 1 < self.buffer.len_lines() {
                self.line_start_char(end.line + 1)
            } else {
                self.buffer.len_chars()
            };
            return Some((start_char, end_char));
        }

        let start_col = self
            .shaped_line(start.line, font_sys)
            .cluster_to_char_idx(start.cluster_idx);
        let start_char = self.buffer.line_col_to_char(start.line, start_col);

        let end_col = self
            .shaped_line(end.line, font_sys)
            .cluster_to_char_idx(end.cluster_idx);
        let line_end = self.line_end_char(end.line);
        let end_char = (self.buffer.line_col_to_char(end.line, end_col) + 1).min(line_end);
        let _ = anchor;
        Some((start_char.min(end_char), start_char.max(end_char)))
    }

    pub fn save(&mut self) -> Result<(), std::io::Error> {
        self.buffer.save()
    }

    pub fn save_to<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        self.language = Language::from_path(&path);
        self.syntax = SyntaxHighlighter::new(self.language);
        self.buffer.save_to(path)
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn is_bracket(ch: char) -> bool {
    matches!(ch, '(' | ')' | '[' | ']' | '{' | '}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_document_insert_text_and_cursor() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut doc = EditorDocument::new();

        doc.insert_text("fn main() {\n}", &mut font_sys);
        assert_eq!(doc.buffer.len_lines(), 2);
        assert_eq!(doc.cursor.line, 1);
        assert_eq!(doc.cursor.cluster_idx, 1); // after '}'

        // Move cursor left
        doc.move_left(false, &mut font_sys);
        assert_eq!(doc.cursor.line, 1);
        assert_eq!(doc.cursor.cluster_idx, 0); // before '}'

        // Insert inside braces
        doc.insert_text("let x = 42; ", &mut font_sys);
        assert_eq!(doc.buffer.line_to_string(1), "let x = 42; }");
    }

    #[test]
    fn test_multi_cursor_insert_and_backspace() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut doc = EditorDocument::from_text("aa", None, false);

        doc.add_caret_at(0, 1);
        assert_eq!(doc.caret_count(), 2);
        doc.insert_text_all("X", &mut font_sys);

        assert_eq!(doc.buffer.line_to_string(0), "XaXa");
        assert_eq!(doc.cursor.line, 0);
        assert_eq!(doc.cursor.cluster_idx, 1);
        assert_eq!(doc.extra_cursors[0].cluster_idx, 3);

        doc.backspace_all(&mut font_sys);
        assert_eq!(doc.buffer.line_to_string(0), "aa");
    }

    #[test]
    fn test_document_myanmar_atomic_cluster_backspace() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut doc = EditorDocument::new();

        // "မင်္ဂလာပါ" has 4 clusters: "မ", "င်္ဂ" (Kinzi), "လာ", "ပါ"
        doc.insert_text("မင်္ဂလာပါ", &mut font_sys);
        assert_eq!(doc.cursor.cluster_idx, 4);

        // Backspace should delete the entire 4th cluster "ပါ" atomically
        doc.backspace(&mut font_sys);
        assert_eq!(doc.cursor.cluster_idx, 3);
        assert_eq!(doc.buffer.line_to_string(0), "မင်္ဂလာ");

        // Backspace again should delete "လာ"
        doc.backspace(&mut font_sys);
        assert_eq!(doc.cursor.cluster_idx, 2);
        assert_eq!(doc.buffer.line_to_string(0), "မင်္ဂ");

        // Backspace again should delete the entire Kinzi cluster "င်္ဂ" atomically
        // without leaving dangling virama or broken consonants
        doc.backspace(&mut font_sys);
        assert_eq!(doc.cursor.cluster_idx, 1);
        assert_eq!(doc.buffer.line_to_string(0), "မ");
    }

    #[test]
    fn test_document_selection_delete() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut doc = EditorDocument::new();

        doc.insert_text("Alpha Beta Gamma", &mut font_sys);

        // Select "Beta "
        doc.selection_anchor = Some(Cursor {
            line: 0,
            cluster_idx: 6,
            preferred_visual_x: None,
        });
        doc.cursor = Cursor {
            line: 0,
            cluster_idx: 11,
            preferred_visual_x: None,
        };

        let sel_text = doc.selected_text(&mut font_sys);
        assert_eq!(sel_text.as_deref(), Some("Beta "));

        // Typing replaces the selection
        doc.insert_text("Delta ", &mut font_sys);
        assert_eq!(doc.buffer.line_to_string(0), "Alpha Delta Gamma");
    }

    #[test]
    fn test_document_scroll_flag_and_cached_line() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut doc = EditorDocument::new();

        doc.insert_text(
            "let count: usize = 100;\nlet message = \"hello\";\n",
            &mut font_sys,
        );
        assert!(doc.scroll_to_cursor_requested);

        // When cached_line is called, it caches line and tokens
        let cached = doc.cached_line(0, &mut font_sys);
        assert!(!cached.cluster_tokens.is_empty());
        assert_eq!(
            cached.shaped.cluster_count(),
            doc.shaped_line(0, &mut font_sys).cluster_count()
        );

        // Simulating render consume of scroll_to_cursor_requested
        doc.scroll_to_cursor_requested = false;

        // Manual viewport scroll does not set flag
        doc.scroll_y = 1;
        assert!(!doc.scroll_to_cursor_requested);

        // Navigation sets flag
        doc.move_down(false, &mut font_sys);
        assert!(doc.scroll_to_cursor_requested);
    }

    #[test]
    fn test_vim_word_motions_and_line_delete() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut doc = EditorDocument::new();

        doc.insert_text("alpha beta gamma\ndelta epsilon\n", &mut font_sys);
        doc.set_cursor_char_offset(0, &mut font_sys);

        doc.move_word_forward(1, false, &mut font_sys);
        assert_eq!(doc.cursor_char_offset(&mut font_sys), 6); // "beta"

        doc.move_word_backward(1, false, &mut font_sys);
        assert_eq!(doc.cursor_char_offset(&mut font_sys), 0);

        doc.delete_lines(0, 1, &mut font_sys);
        assert_eq!(doc.buffer.line_to_string(0), "delta epsilon\n");
        assert_eq!(doc.cursor.line, 0);
    }

    #[test]
    fn test_vim_cluster_delete_and_utf16_position() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut doc = EditorDocument::new();

        doc.insert_text("မြန်မာ", &mut font_sys);
        let (_, utf16_col) = doc.cursor_utf16_position(&mut font_sys);
        assert_eq!(utf16_col, "မြန်မာ".encode_utf16().count() as u32);

        doc.set_cursor_char_offset(0, &mut font_sys);
        doc.delete_next_cluster(&mut font_sys);
        assert_eq!(doc.buffer.line_to_string(0), "န်မာ");
    }

    #[test]
    fn test_vim_visual_line_range() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut doc = EditorDocument::new();

        doc.insert_text("one\ntwo\nthree\n", &mut font_sys);
        doc.set_cursor_char_offset(0, &mut font_sys);
        doc.selection_anchor = Some(Cursor::new(0, 0));
        doc.cursor = Cursor::new(1, 0);
        doc.selection_kind = SelectionKind::Line;

        let (start, end) = doc.visual_char_range(&mut font_sys).unwrap();
        assert_eq!(doc.buffer.slice_to_string(start, end), "one\ntwo\n");
    }
}
