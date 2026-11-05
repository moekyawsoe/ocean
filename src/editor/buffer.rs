use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ropey::Rope;

#[derive(Debug, Clone)]
pub enum EditOp {
    Insert { char_idx: usize, text: String },
    Delete { char_idx: usize, text: String },
}

impl EditOp {
    pub fn inverted(&self) -> Self {
        match self {
            Self::Insert { char_idx, text } => Self::Delete {
                char_idx: *char_idx,
                text: text.clone(),
            },
            Self::Delete { char_idx, text } => Self::Insert {
                char_idx: *char_idx,
                text: text.clone(),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct UndoGroup {
    pub ops: Vec<EditOp>,
    pub cursor_before: (usize, usize),
    pub cursor_after: (usize, usize),
    pub timestamp: Instant,
}

pub struct TextBuffer {
    pub rope: Rope,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub revision: u64,
    undo_stack: Vec<UndoGroup>,
    redo_stack: Vec<UndoGroup>,
    last_edit_time: Instant,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            path: None,
            dirty: false,
            revision: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit_time: Instant::now(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            path: None,
            dirty: false,
            revision: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit_time: Instant::now(),
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let p = path.as_ref();
        let content = if p.exists() {
            fs::read_to_string(p)?
        } else {
            String::new()
        };
        Ok(Self {
            rope: Rope::from_str(&content),
            path: Some(p.to_path_buf()),
            dirty: false,
            revision: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit_time: Instant::now(),
        })
    }

    pub fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "[Untitled]".to_string())
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn char_at(&self, char_idx: usize) -> Option<char> {
        self.rope.get_char(char_idx)
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    pub fn line(&self, line_idx: usize) -> Option<String> {
        if line_idx < self.rope.len_lines() {
            Some(self.rope.line(line_idx).to_string())
        } else {
            None
        }
    }

    pub fn line_without_newline(&self, line_idx: usize) -> String {
        if let Some(s) = self.line(line_idx) {
            s.trim_end_matches(&['\r', '\n'][..]).to_string()
        } else {
            String::new()
        }
    }

    pub fn char_to_line_col(&self, char_idx: usize) -> (usize, usize) {
        let char_idx = char_idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(char_idx);
        let line_start_char = self.rope.line_to_char(line);
        let col = char_idx.saturating_sub(line_start_char);
        (line, col)
    }

    pub fn line_col_to_char(&self, line: usize, col: usize) -> usize {
        let total_lines = self.rope.len_lines();
        if line >= total_lines {
            return self.rope.len_chars();
        }
        let line_start_char = self.rope.line_to_char(line);
        let line_len = self.rope.line(line).len_chars();
        line_start_char + col.min(line_len)
    }

    pub fn slice_to_string(&self, start_char: usize, end_char: usize) -> String {
        let max_len = self.rope.len_chars();
        let s = start_char.min(max_len);
        let e = end_char.min(max_len);
        if s >= e {
            String::new()
        } else {
            self.rope.slice(s..e).to_string()
        }
    }

    pub fn line_to_string(&self, line_idx: usize) -> String {
        if line_idx >= self.len_lines() {
            return String::new();
        }
        self.rope.line(line_idx).to_string()
    }

    pub fn slice_bytes(&self, start_byte: usize, len_bytes: usize) -> String {
        let max_bytes = self.rope.len_bytes();
        let s = start_byte.min(max_bytes);
        let e = (s + len_bytes).min(max_bytes);
        if s >= e {
            return String::new();
        }
        let start_char = self.rope.byte_to_char(s);
        let end_char = self.rope.byte_to_char(e);
        self.slice_to_string(start_char, end_char)
    }

    pub fn insert(
        &mut self,
        char_idx: usize,
        text: &str,
        cursor_before: (usize, usize),
        cursor_after: (usize, usize),
    ) {
        if text.is_empty() {
            return;
        }
        let char_idx = char_idx.min(self.rope.len_chars());
        self.rope.insert(char_idx, text);
        self.dirty = true;
        self.revision += 1;

        let op = EditOp::Insert {
            char_idx,
            text: text.to_string(),
        };

        self.push_op(op, cursor_before, cursor_after);
        self.redo_stack.clear();
    }

    pub fn delete(
        &mut self,
        start_char: usize,
        end_char: usize,
        cursor_before: (usize, usize),
        cursor_after: (usize, usize),
    ) -> String {
        let max_len = self.rope.len_chars();
        let s = start_char.min(max_len);
        let e = end_char.min(max_len);
        if s >= e {
            return String::new();
        }
        let removed = self.rope.slice(s..e).to_string();
        self.rope.remove(s..e);
        self.dirty = true;
        self.revision += 1;

        let op = EditOp::Delete {
            char_idx: s,
            text: removed.clone(),
        };

        self.push_op(op, cursor_before, cursor_after);
        self.redo_stack.clear();
        removed
    }

    fn push_op(&mut self, op: EditOp, cursor_before: (usize, usize), cursor_after: (usize, usize)) {
        let now = Instant::now();
        let is_recent = now.duration_since(self.last_edit_time).as_millis() < 400;
        self.last_edit_time = now;

        if is_recent
            && let Some(last_group) = self.undo_stack.last_mut()
            && let Some(last_op) = last_group.ops.last_mut()
        {
            match (last_op, &op) {
                (
                    EditOp::Insert {
                        char_idx: c1,
                        text: t1,
                    },
                    EditOp::Insert {
                        char_idx: c2,
                        text: t2,
                    },
                ) if *c1 + t1.chars().count() == *c2 && !t2.contains('\n') => {
                    t1.push_str(t2);
                    last_group.cursor_after = cursor_after;
                    return;
                }
                (
                    EditOp::Delete {
                        char_idx: c1,
                        text: t1,
                    },
                    EditOp::Delete {
                        char_idx: c2,
                        text: t2,
                    },
                ) if *c2 + t2.chars().count() == *c1 && !t2.contains('\n') => {
                    let mut combined = t2.clone();
                    combined.push_str(t1);
                    *t1 = combined;
                    *c1 = *c2;
                    last_group.cursor_after = cursor_after;
                    return;
                }
                _ => {}
            }
        }

        self.undo_stack.push(UndoGroup {
            ops: vec![op],
            cursor_before,
            cursor_after,
            timestamp: now,
        });
    }

    pub fn undo(&mut self) -> Option<(usize, usize)> {
        let group = self.undo_stack.pop()?;
        for op in group.ops.iter().rev() {
            match op {
                EditOp::Insert { char_idx, text } => {
                    let end = *char_idx + text.chars().count();
                    if end <= self.rope.len_chars() {
                        self.rope.remove(*char_idx..end);
                    }
                }
                EditOp::Delete { char_idx, text } => {
                    let idx = (*char_idx).min(self.rope.len_chars());
                    self.rope.insert(idx, text);
                }
            }
        }
        self.dirty = true;
        self.revision += 1;
        let restore_cursor = group.cursor_before;
        self.redo_stack.push(group);
        Some(restore_cursor)
    }

    pub fn redo(&mut self) -> Option<(usize, usize)> {
        let group = self.redo_stack.pop()?;
        for op in &group.ops {
            match op {
                EditOp::Insert { char_idx, text } => {
                    let idx = (*char_idx).min(self.rope.len_chars());
                    self.rope.insert(idx, text);
                }
                EditOp::Delete { char_idx, text } => {
                    let end = *char_idx + text.chars().count();
                    if end <= self.rope.len_chars() {
                        self.rope.remove(*char_idx..end);
                    }
                }
            }
        }
        self.dirty = true;
        self.revision += 1;
        let restore_cursor = group.cursor_after;
        self.undo_stack.push(group);
        Some(restore_cursor)
    }

    pub fn save(&mut self) -> Result<(), std::io::Error> {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            let text = self.rope.to_string();
            fs::write(path, text)?;
            self.dirty = false;
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No file path specified for saving",
            ))
        }
    }

    pub fn save_to<P: AsRef<Path>>(&mut self, path: P) -> Result<(), std::io::Error> {
        let p = path.as_ref();
        if let Some(parent) = p.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let text = self.rope.to_string();
        fs::write(p, text)?;
        self.path = Some(p.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    pub fn search(&self, query: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }
        let full_text = self.rope.to_string();
        let mut results = Vec::new();

        if case_sensitive {
            let mut start = 0;
            while let Some(idx) = full_text[start..].find(query) {
                let byte_idx = start + idx;
                let char_start = self.rope.byte_to_char(byte_idx);
                let char_end = self.rope.byte_to_char(byte_idx + query.len());
                results.push((char_start, char_end));
                start = byte_idx + query.len().max(1);
            }
        } else {
            let q_lower = query.to_lowercase();
            let text_lower = full_text.to_lowercase();
            let mut start = 0;
            while let Some(idx) = text_lower[start..].find(&q_lower) {
                let byte_idx = start + idx;
                let char_start = self.rope.byte_to_char(byte_idx);
                let char_end = self.rope.byte_to_char(byte_idx + query.len());
                results.push((char_start, char_end));
                start = byte_idx + query.len().max(1);
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_insert_and_delete() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World", (0, 0), (0, 11));
        assert_eq!(buf.rope.to_string(), "Hello World");
        assert_eq!(buf.len_lines(), 1);

        // Delete " World"
        buf.delete(5, 11, (0, 11), (0, 5));
        assert_eq!(buf.rope.to_string(), "Hello");

        // Insert newline and line 2
        buf.insert(5, "\nOcean", (0, 5), (1, 5));
        assert_eq!(buf.len_lines(), 2);
        assert_eq!(buf.slice_to_string(0, 6), "Hello\n");
        assert_eq!(buf.slice_to_string(6, 11), "Ocean");
    }

    #[test]
    fn test_buffer_undo_redo() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Initial text", (0, 0), (0, 12));
        assert_eq!(buf.rope.to_string(), "Initial text");

        // Trigger undo
        let restored_cursor = buf.undo();
        assert!(restored_cursor.is_some());
        assert_eq!(buf.rope.to_string(), "");

        // Trigger redo
        let restored_redo = buf.redo();
        assert!(restored_redo.is_some());
        assert_eq!(buf.rope.to_string(), "Initial text");
    }

    #[test]
    fn test_buffer_line_col_conversion() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\nLine 2 is longer\nL3", (0, 0), (2, 2));
        assert_eq!(buf.len_lines(), 3);

        let (l, c) = buf.char_to_line_col(7); // start of Line 2
        assert_eq!(l, 1);
        assert_eq!(c, 0);

        let char_idx = buf.line_col_to_char(1, 4);
        assert_eq!(char_idx, 11); // ' ' after "Line" in Line 2
    }

    #[test]
    fn test_buffer_search() {
        let mut buf = TextBuffer::new();
        buf.insert(
            0,
            "The quick brown fox jumps over the lazy dog",
            (0, 0),
            (0, 43),
        );
        let matches = buf.search("the", false);
        assert_eq!(matches.len(), 2);

        let matches_case = buf.search("the", true);
        assert_eq!(matches_case.len(), 1);
    }

    #[test]
    fn test_buffer_unicode_myanmar() {
        let mut buf = TextBuffer::new();
        let myanmar_text = "မင်္ဂလာပါ မြန်မာစာ";
        buf.insert(0, myanmar_text, (0, 0), (0, myanmar_text.chars().count()));
        assert_eq!(buf.rope.to_string(), myanmar_text);
        assert_eq!(buf.rope.len_chars(), myanmar_text.chars().count());
    }
}
