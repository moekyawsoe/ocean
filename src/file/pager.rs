use std::io::{self, Read, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Backspace,
    Tab,
    Esc,
}

pub struct RawTerminal {
    #[cfg(unix)]
    orig: libc::termios,
    in_alt_screen: bool,
}

impl RawTerminal {
    pub fn enter() -> io::Result<Self> {
        #[cfg(unix)]
        {
            unsafe {
                let mut orig = std::mem::zeroed();
                if libc::tcgetattr(libc::STDIN_FILENO, &mut orig) != 0 {
                    return Err(io::Error::last_os_error());
                }
                let mut raw = orig;
                libc::cfmakeraw(&mut raw);
                if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &mut raw) != 0 {
                    return Err(io::Error::last_os_error());
                }

                let mut term = Self {
                    orig,
                    in_alt_screen: false,
                };
                term.enter_alt_screen()?;
                Ok(term)
            }
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                in_alt_screen: false,
            })
        }
    }

    pub fn enter_alt_screen(&mut self) -> io::Result<()> {
        if !self.in_alt_screen {
            let mut stdout = io::stdout();
            stdout.write_all(b"\x1b[?1049h\x1b[?25l\x1b[H")?;
            stdout.flush()?;
            self.in_alt_screen = true;
        }
        Ok(())
    }

    pub fn leave_alt_screen(&mut self) -> io::Result<()> {
        if self.in_alt_screen {
            let mut stdout = io::stdout();
            stdout.write_all(b"\x1b[?25h\x1b[?1049l")?;
            stdout.flush()?;
            self.in_alt_screen = false;
        }
        Ok(())
    }

    pub fn terminal_size() -> (usize, usize) {
        #[cfg(unix)]
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0
                && ws.ws_col > 0
                && ws.ws_row > 0
            {
                return (ws.ws_col as usize, ws.ws_row as usize);
            }
        }
        (80, 24)
    }

    pub fn read_key(&mut self) -> Option<Key> {
        let mut buf = [0u8; 16];
        let mut stdin = io::stdin();
        let n = stdin.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }

        match buf[0] {
            3 => Some(Key::Ctrl('c')),
            4 => Some(Key::Ctrl('d')),
            21 => Some(Key::Ctrl('u')),
            13 | 10 => Some(Key::Enter),
            127 | 8 => Some(Key::Backspace),
            9 => Some(Key::Tab),
            27 => {
                if n == 1 {
                    return Some(Key::Esc);
                }
                if buf[1] == b'[' {
                    match buf[2] {
                        b'A' => return Some(Key::Up),
                        b'B' => return Some(Key::Down),
                        b'C' => return Some(Key::Right),
                        b'D' => return Some(Key::Left),
                        b'H' => return Some(Key::Home),
                        b'F' => return Some(Key::End),
                        b'1'..=b'9' if n >= 4 && buf[3] == b'~' => match buf[2] {
                            b'1' => return Some(Key::Home),
                            b'4' => return Some(Key::End),
                            b'5' => return Some(Key::PageUp),
                            b'6' => return Some(Key::PageDown),
                            _ => return Some(Key::Esc),
                        },
                        _ => return Some(Key::Esc),
                    }
                }
                Some(Key::Esc)
            }
            _b => {
                let s = std::str::from_utf8(&buf[..n]).ok()?;
                s.chars().next().map(Key::Char)
            }
        }
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        let _ = self.leave_alt_screen();
        #[cfg(unix)]
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &mut self.orig);
        }
    }
}

pub struct Pager {
    pub lines: Vec<String>,
    pub scroll: usize,
    pub title: String,
    pub file_type_badge: String,
    pub search_query: Option<String>,
}

impl Pager {
    pub fn new(title: String, file_type_badge: String, lines: Vec<String>) -> Self {
        Self {
            lines,
            scroll: 0,
            title,
            file_type_badge,
            search_query: None,
        }
    }

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
                    Key::Char('j') | Key::Down => {
                        let max_scroll = self.lines.len().saturating_sub(rows.saturating_sub(2));
                        if self.scroll < max_scroll {
                            self.scroll += 1;
                        }
                    }
                    Key::Char('k') | Key::Up => {
                        self.scroll = self.scroll.saturating_sub(1);
                    }
                    Key::Char('d') | Key::Ctrl('d') => {
                        let step = (rows / 2).max(1);
                        let max_scroll = self.lines.len().saturating_sub(rows.saturating_sub(2));
                        self.scroll = (self.scroll + step).min(max_scroll);
                    }
                    Key::Char('u') | Key::Ctrl('u') => {
                        let step = (rows / 2).max(1);
                        self.scroll = self.scroll.saturating_sub(step);
                    }
                    Key::Char(' ') | Key::Char('f') | Key::PageDown => {
                        let step = rows.saturating_sub(3).max(1);
                        let max_scroll = self.lines.len().saturating_sub(rows.saturating_sub(2));
                        self.scroll = (self.scroll + step).min(max_scroll);
                    }
                    Key::Char('b') | Key::PageUp => {
                        let step = rows.saturating_sub(3).max(1);
                        self.scroll = self.scroll.saturating_sub(step);
                    }
                    Key::Char('g') | Key::Home => {
                        self.scroll = 0;
                    }
                    Key::Char('G') | Key::End => {
                        let max_scroll = self.lines.len().saturating_sub(rows.saturating_sub(2));
                        self.scroll = max_scroll;
                    }
                    Key::Char('/') => {
                        if let Some(query) = self.prompt_search(&mut term, cols, rows)? {
                            if !query.is_empty() {
                                self.search_query = Some(query.clone());
                                self.find_next_match(&query, true);
                            } else {
                                self.search_query = None;
                            }
                        }
                    }
                    Key::Char('n') => {
                        if let Some(query) = self.search_query.clone() {
                            self.find_next_match(&query, true);
                        }
                    }
                    Key::Char('N') => {
                        if let Some(query) = self.search_query.clone() {
                            self.find_next_match(&query, false);
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    fn find_next_match(&mut self, query: &str, forward: bool) {
        if self.lines.is_empty() {
            return;
        }
        let lower_query = query.to_lowercase();
        let len = self.lines.len();

        if forward {
            for i in 1..=len {
                let idx = (self.scroll + i) % len;
                if strip_ansi(&self.lines[idx])
                    .to_lowercase()
                    .contains(&lower_query)
                {
                    self.scroll = idx;
                    break;
                }
            }
        } else {
            for i in 1..=len {
                let idx = (self.scroll + len - (i % len)) % len;
                if strip_ansi(&self.lines[idx])
                    .to_lowercase()
                    .contains(&lower_query)
                {
                    self.scroll = idx;
                    break;
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

        // Top Header bar
        let badge = &self.file_type_badge;
        let title = &self.title;
        let header_text = format!(" {badge} {title} ");
        let pad = cols.saturating_sub(header_text.chars().count());
        out.push_str("\x1b[7m\x1b[1m");
        out.push_str(&header_text);
        out.push_str(&" ".repeat(pad));
        out.push_str("\x1b[0m\r\n");

        // Content Area
        let content_rows = rows.saturating_sub(2);
        for r in 0..content_rows {
            let line_idx = self.scroll + r;
            out.push_str("\x1b[K");
            if line_idx < self.lines.len() {
                let line = &self.lines[line_idx];
                if let Some(query) = &self.search_query {
                    out.push_str(&highlight_search(line, query));
                } else {
                    out.push_str(line);
                }
            }
            out.push_str("\r\n");
        }

        // Bottom Status bar
        let total = self.lines.len();
        let current = (self.scroll + 1).min(total);
        let pct = if total > 0 {
            (current * 100) / total
        } else {
            100
        };
        let status_left = format!(
            " Line {current}/{total} ({pct}%) | [/] Search  [j/k] Scroll  [g/G] Top/Bottom  [q] Quit"
        );
        let status_pad = cols.saturating_sub(status_left.chars().count());
        out.push_str("\x1b[38;5;250;48;5;236m");
        out.push_str(&status_left);
        out.push_str(&" ".repeat(status_pad));
        out.push_str("\x1b[0m");

        let mut stdout = io::stdout();
        stdout.write_all(out.as_bytes())?;
        stdout.flush()?;
        Ok(())
    }
}

pub fn strip_ansi(s: &str) -> String {
    let mut clean = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            clean.push(c);
        }
    }
    clean
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
