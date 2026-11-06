use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use super::pager::{Key, RawTerminal, strip_ansi};

pub fn view_pdf(path: &Path) -> io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document.pdf")
        .to_string();

    let pages = extract_pdf_pages(path);
    if pages.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "No extractable text found in PDF document.",
        ));
    }

    let mut viewer = PdfViewer {
        file_name,
        pages,
        current_page: 0,
        scroll_offset: 0,
        search_query: None,
    };

    viewer.run()
}

pub fn extract_pdf_pages(path: &Path) -> Vec<Vec<String>> {
    // 1. Try pdftotext (standard poppler-utils) with -layout
    if let Ok(output) = Command::new("pdftotext")
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            let full_text = String::from_utf8_lossy(&output.stdout);
            let raw_pages: Vec<&str> = full_text.split('\x0c').collect();
            let mut result = Vec::new();
            for raw_page in raw_pages {
                let lines: Vec<String> = raw_page.lines().map(|s| s.to_string()).collect();
                if !lines.is_empty() && lines.iter().any(|l| !l.trim().is_empty()) {
                    result.push(lines);
                }
            }
            if !result.is_empty() {
                return result;
            }
        }
    }

    // 2. Try mutool draw -F txt
    if let Ok(output) = Command::new("mutool")
        .arg("draw")
        .arg("-F")
        .arg("txt")
        .arg("-o")
        .arg("-")
        .arg(path)
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            let full_text = String::from_utf8_lossy(&output.stdout);
            let raw_pages: Vec<&str> = full_text.split('\x0c').collect();
            let mut result = Vec::new();
            for raw_page in raw_pages {
                let lines: Vec<String> = raw_page.lines().map(|s| s.to_string()).collect();
                if !lines.is_empty() && lines.iter().any(|l| !l.trim().is_empty()) {
                    result.push(lines);
                }
            }
            if !result.is_empty() {
                return result;
            }
        }
    }

    // 3. Fallback: lightweight raw byte text scanner
    if let Ok(bytes) = std::fs::read(path) {
        let text = extract_raw_pdf_text(&bytes);
        if !text.is_empty() {
            return vec![text.lines().map(|s| s.to_string()).collect()];
        }
    }

    vec![vec![
        "(Could not extract text from this PDF file. It may be scanned or empty.)".to_string(),
    ]]
}

fn extract_raw_pdf_text(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let mut j = i + 1;
            let mut s = String::new();
            while j < bytes.len() && bytes[j] != b')' {
                if bytes[j] == b'\\' && j + 1 < bytes.len() {
                    j += 1;
                }
                if bytes[j].is_ascii_graphic() || bytes[j] == b' ' {
                    s.push(bytes[j] as char);
                }
                j += 1;
            }
            if s.len() > 3 {
                text.push_str(&s);
                text.push(' ');
            }
            i = j;
        }
        i += 1;
    }
    text
}

pub struct PdfViewer {
    pub file_name: String,
    pub pages: Vec<Vec<String>>,
    pub current_page: usize,
    pub scroll_offset: usize,
    pub search_query: Option<String>,
}

impl PdfViewer {
    pub fn run(&mut self) -> io::Result<()> {
        let mut term = RawTerminal::enter()?;

        loop {
            let (cols, rows) = RawTerminal::terminal_size();
            self.render(cols, rows)?;

            if let Some(key) = term.read_key() {
                match key {
                    Key::Char('q') | Key::Char('Q') | Key::Esc | Key::Ctrl('c') => {
                        break;
                    }
                    Key::Char('n') | Key::PageDown => {
                        if self.current_page + 1 < self.pages.len() {
                            self.current_page += 1;
                            self.scroll_offset = 0;
                        }
                    }
                    Key::Char('p') | Key::PageUp => {
                        if self.current_page > 0 {
                            self.current_page -= 1;
                            self.scroll_offset = 0;
                        }
                    }
                    Key::Char('j') | Key::Down => {
                        let page_lines = self.pages[self.current_page].len();
                        let max_scroll = page_lines.saturating_sub(rows.saturating_sub(2));
                        if self.scroll_offset < max_scroll {
                            self.scroll_offset += 1;
                        } else if self.current_page + 1 < self.pages.len() {
                            self.current_page += 1;
                            self.scroll_offset = 0;
                        }
                    }
                    Key::Char('k') | Key::Up => {
                        if self.scroll_offset > 0 {
                            self.scroll_offset -= 1;
                        } else if self.current_page > 0 {
                            self.current_page -= 1;
                            let prev_len = self.pages[self.current_page].len();
                            self.scroll_offset = prev_len.saturating_sub(rows.saturating_sub(2));
                        }
                    }
                    Key::Char('d') | Key::Ctrl('d') => {
                        let step = (rows / 2).max(1);
                        let page_lines = self.pages[self.current_page].len();
                        let max_scroll = page_lines.saturating_sub(rows.saturating_sub(2));
                        self.scroll_offset = (self.scroll_offset + step).min(max_scroll);
                    }
                    Key::Char('u') | Key::Ctrl('u') => {
                        let step = (rows / 2).max(1);
                        self.scroll_offset = self.scroll_offset.saturating_sub(step);
                    }
                    Key::Char('g') | Key::Home => {
                        self.current_page = 0;
                        self.scroll_offset = 0;
                    }
                    Key::Char('G') | Key::End => {
                        self.current_page = self.pages.len().saturating_sub(1);
                        self.scroll_offset = 0;
                    }
                    Key::Char('/') => {
                        if let Some(query) = self.prompt_search(&mut term, cols, rows)? {
                            if !query.is_empty() {
                                self.search_query = Some(query.clone());
                                self.find_next_match(&query);
                            } else {
                                self.search_query = None;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    fn find_next_match(&mut self, query: &str) {
        let lower = query.to_lowercase();
        for page_idx in self.current_page..self.pages.len() {
            let start_line = if page_idx == self.current_page {
                self.scroll_offset + 1
            } else {
                0
            };
            for (line_idx, line) in self.pages[page_idx].iter().enumerate().skip(start_line) {
                if strip_ansi(line).to_lowercase().contains(&lower) {
                    self.current_page = page_idx;
                    self.scroll_offset = line_idx;
                    return;
                }
            }
        }
        // Wrap around from beginning
        for page_idx in 0..=self.current_page {
            for (line_idx, line) in self.pages[page_idx].iter().enumerate() {
                if strip_ansi(line).to_lowercase().contains(&lower) {
                    self.current_page = page_idx;
                    self.scroll_offset = line_idx;
                    return;
                }
            }
        }
    }

    fn prompt_search(
        &mut self,
        term: &mut RawTerminal,
        _cols: usize,
        rows: usize,
    ) -> io::Result<Option<String>> {
        let mut query = String::new();
        loop {
            let mut frame = String::with_capacity(256);
            frame.push_str(&format!(
                "\x1b[{rows};1H\x1b[K\x1b[7m/{query}\x1b[0m\x1b[?25h"
            ));
            let mut stdout = io::stdout();
            stdout.write_all(frame.as_bytes())?;
            stdout.flush()?;

            if let Some(key) = term.read_key() {
                match key {
                    Key::Enter => {
                        stdout.write_all(b"\x1b[?25l")?;
                        stdout.flush()?;
                        return Ok(Some(query));
                    }
                    Key::Esc | Key::Ctrl('c') => {
                        stdout.write_all(b"\x1b[?25l")?;
                        stdout.flush()?;
                        return Ok(None);
                    }
                    Key::Backspace => {
                        query.pop();
                    }
                    Key::Char(c) if !c.is_control() => {
                        query.push(c);
                    }
                    _ => {}
                }
            }
        }
    }

    fn render(&self, cols: usize, rows: usize) -> io::Result<()> {
        let mut out = String::with_capacity(cols * rows * 4);
        out.push_str("\x1b[H\x1b[?25l");

        let total_pages = self.pages.len();
        let curr_page_num = self.current_page + 1;
        let page_lines = &self.pages[self.current_page];

        // Header
        let header_left = format!(
            " 📄 PDF: {} | Page {}/{} ",
            self.file_name, curr_page_num, total_pages
        );
        let pad = cols.saturating_sub(header_left.chars().count());
        out.push_str("\x1b[7m\x1b[1m");
        out.push_str(&header_left);
        out.push_str(&" ".repeat(pad));
        out.push_str("\x1b[0m\r\n");

        // Content
        let content_rows = rows.saturating_sub(2);
        for r in 0..content_rows {
            let line_idx = self.scroll_offset + r;
            out.push_str("\x1b[K");
            if line_idx < page_lines.len() {
                let line = &page_lines[line_idx];
                if let Some(query) = &self.search_query {
                    out.push_str(&highlight_search(line, query));
                } else {
                    out.push_str(line);
                }
            }
            out.push_str("\r\n");
        }

        // Footer
        let footer_left = format!(
            " [Page {}/{}] (Line {}/{}) | [n/p] Page  [j/k] Scroll  [/] Search  [g/G] First/Last  [q] Quit",
            curr_page_num,
            total_pages,
            (self.scroll_offset + 1).min(page_lines.len()),
            page_lines.len()
        );
        let pad = cols.saturating_sub(footer_left.chars().count());
        out.push_str("\x1b[38;5;250;48;5;236m");
        out.push_str(&footer_left);
        out.push_str(&" ".repeat(pad));
        out.push_str("\x1b[0m");

        let mut stdout = io::stdout();
        stdout.write_all(out.as_bytes())?;
        stdout.flush()?;
        Ok(())
    }
}

fn highlight_search(line: &str, query: &str) -> String {
    if query.is_empty() {
        return line.to_string();
    }
    let lower_line = line.to_lowercase();
    let lower_q = query.to_lowercase();
    if !lower_line.contains(&lower_q) {
        return line.to_string();
    }

    let mut result = String::with_capacity(line.len() + 32);
    let mut last_idx = 0;
    while let Some(pos) = lower_line[last_idx..].find(&lower_q) {
        let abs_pos = last_idx + pos;
        result.push_str(&line[last_idx..abs_pos]);
        result.push_str("\x1b[48;5;220;38;5;16m");
        result.push_str(&line[abs_pos..abs_pos + query.len()]);
        result.push_str("\x1b[0m");
        last_idx = abs_pos + query.len();
    }
    result.push_str(&line[last_idx..]);
    result
}
