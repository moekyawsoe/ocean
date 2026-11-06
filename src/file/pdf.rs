use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::pager::{Key, RawTerminal, strip_ansi};

pub fn view_pdf(path: &Path) -> io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document.pdf")
        .to_string();

    let mut pages = extract_pdf_pages(path);
    let page_count = pdf_page_count(path).unwrap_or(pages.len()).max(1);
    while pages.len() < page_count {
        pages.push(Vec::new());
    }
    pages.truncate(page_count);

    let image_tools = image_tools_available();
    let mut viewer = PdfViewer {
        file_name,
        pdf_path: path.to_path_buf(),
        pages,
        image_mode: image_tools,
        image_cache: HashMap::new(),
        visible_line_count: 1,
        current_page: 0,
        scroll_offset: 0,
        search_query: None,
    };

    viewer.run()
}

fn image_tools_available() -> bool {
    Command::new("pdftoppm").arg("-v").output().is_ok()
        && Command::new("chafa").arg("--version").output().is_ok()
}

fn pdf_page_count(path: &Path) -> Option<usize> {
    let output = Command::new("pdfinfo").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().find_map(|line| {
        line.strip_prefix("Pages:")
            .and_then(|rest| rest.trim().parse::<usize>().ok())
    })
}

fn render_pdf_page_image(
    path: &Path,
    page: usize,
    cols: usize,
    rows: usize,
) -> Option<Vec<String>> {
    if cols == 0 || rows == 0 {
        return None;
    }
    let prefix =
        std::env::temp_dir().join(format!("ocean-pdf-{}-{}", std::process::id(), page + 1));
    let png_path = prefix.with_extension("png");
    let page_arg = (page + 1).to_string();
    let rendered = Command::new("pdftoppm")
        .args([
            "-png",
            "-r",
            "140",
            "-f",
            &page_arg,
            "-l",
            &page_arg,
            "-singlefile",
        ])
        .arg(path)
        .arg(&prefix)
        .output();
    if !rendered.is_ok_and(|output| output.status.success() && png_path.exists()) {
        return None;
    }

    let size = format!("{cols}x{rows}");
    let output = Command::new("chafa")
        .args([
            "--format",
            "symbols",
            "--colors",
            "256",
            "--animate",
            "off",
            "--size",
            &size,
        ])
        .arg(&png_path)
        .output();
    let _ = std::fs::remove_file(&png_path);

    let output = output.ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().map(str::to_string).collect())
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
    pub pdf_path: PathBuf,
    pub pages: Vec<Vec<String>>,
    pub image_mode: bool,
    pub image_cache: HashMap<(usize, usize, usize), Vec<String>>,
    pub visible_line_count: usize,
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
                    Key::Char('t') | Key::Char('T') => {
                        self.image_mode = !self.image_mode;
                        self.scroll_offset = 0;
                        if self.image_mode && !image_tools_available() {
                            self.image_mode = false;
                        }
                    }
                    Key::Char('j') | Key::Down => {
                        let max_scroll = self
                            .visible_line_count
                            .saturating_sub(rows.saturating_sub(2));
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
                            self.scroll_offset = self.visible_line_count;
                        }
                    }
                    Key::Char('d') | Key::Ctrl('d') => {
                        let step = (rows / 2).max(1);
                        let max_scroll = self
                            .visible_line_count
                            .saturating_sub(rows.saturating_sub(2));
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
                        self.image_mode = false;
                        self.scroll_offset = 0;
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

    fn page_lines(&mut self, cols: usize, rows: usize) -> Vec<String> {
        let content_rows = rows.saturating_sub(2).max(1);
        if self.image_mode {
            let key = (self.current_page, cols, content_rows);
            if let Some(lines) = self.image_cache.get(&key) {
                return lines.clone();
            }
            if let Some(lines) =
                render_pdf_page_image(&self.pdf_path, self.current_page, cols, content_rows)
            {
                let lines: Vec<String> = lines.into_iter().take(content_rows).collect();
                self.image_cache.insert(key, lines.clone());
                return lines;
            }
            self.image_mode = false;
        }
        self.pages
            .get(self.current_page)
            .cloned()
            .unwrap_or_default()
    }

    fn render(&mut self, cols: usize, rows: usize) -> io::Result<()> {
        let mut out = String::with_capacity(cols * rows * 4);
        out.push_str("\x1b[H\x1b[?25l");

        let total_pages = self.pages.len();
        let curr_page_num = self.current_page + 1;
        let page_lines = self.page_lines(cols, rows);
        self.visible_line_count = page_lines.len();

        // Header
        let mode = if self.image_mode { "IMAGE" } else { "TEXT" };
        let header_left = format!(
            " 📄 PDF: {} | Page {}/{} | {} ",
            self.file_name, curr_page_num, total_pages, mode
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
                if !self.image_mode
                    && let Some(query) = &self.search_query
                {
                    out.push_str(&highlight_search(line, query));
                } else {
                    out.push_str(line);
                }
            }
            out.push_str("\r\n");
        }

        // Footer
        let footer_left = format!(
            " [Page {}/{}] (Line {}/{}) | [n/p] Page  [j/k] Scroll  [t] Image/Text  [/] Search  [g/G] First/Last  [q] Quit",
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
