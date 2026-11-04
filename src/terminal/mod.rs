pub mod input;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event as TermEvent, EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config as TermConfig, TermMode, viewport_to_point};
use alacritty_terminal::tty::{self, Options as PtyOptions, Shell};
use alacritty_terminal::vte::ansi::{
    Color as AnsiColor, CursorShape, NamedColor, Processor, Rgb as VteRgb,
};
use winit::event_loop::EventLoopProxy;

use crate::config::theme::Rgb;
use crate::config::{Config, CursorStyle};

#[derive(Clone)]
pub enum AppEvent {
    Wakeup,
    TitleChanged(u64, Option<String>),
    ClipboardStore(String),
    PtyReply(u64, String),
    ColorQuery(
        u64,
        usize,
        Arc<dyn Fn(VteRgb) -> String + Sync + Send + 'static>,
    ),
    ChildExited(u64),
}

impl std::fmt::Debug for AppEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wakeup => write!(f, "Wakeup"),
            Self::TitleChanged(id, t) => write!(f, "TitleChanged({id}, {t:?})"),
            Self::ClipboardStore(_) => write!(f, "ClipboardStore"),
            Self::PtyReply(id, _) => write!(f, "PtyReply({id})"),
            Self::ColorQuery(id, idx, _) => write!(f, "ColorQuery({id}, {idx})"),
            Self::ChildExited(id) => write!(f, "ChildExited({id})"),
        }
    }
}

#[derive(Clone)]
pub struct OceanListener {
    pub tab_id: u64,
    pub proxy: EventLoopProxy<AppEvent>,
    pub wakeup_queued: Arc<AtomicBool>,
}

impl EventListener for OceanListener {
    fn send_event(&self, event: TermEvent) {
        match event {
            TermEvent::Wakeup | TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {
                if !self.wakeup_queued.swap(true, Ordering::AcqRel) {
                    let _ = self.proxy.send_event(AppEvent::Wakeup);
                }
            }
            TermEvent::Title(t) => {
                let _ = self.proxy.send_event(AppEvent::TitleChanged(self.tab_id, Some(t)));
            }
            TermEvent::ResetTitle => {
                let _ = self.proxy.send_event(AppEvent::TitleChanged(self.tab_id, None));
            }
            TermEvent::ClipboardStore(_, text) => {
                let _ = self.proxy.send_event(AppEvent::ClipboardStore(text));
            }
            TermEvent::PtyWrite(reply) => {
                let _ = self.proxy.send_event(AppEvent::PtyReply(self.tab_id, reply));
            }
            TermEvent::ColorRequest(index, fmt) => {
                let _ = self
                    .proxy
                    .send_event(AppEvent::ColorQuery(self.tab_id, index, fmt));
            }
            TermEvent::ChildExit(_) | TermEvent::Exit => {
                let _ = self.proxy.send_event(AppEvent::ChildExited(self.tab_id));
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
pub struct GridDims {
    pub cols: usize,
    pub rows: usize,
}

impl Dimensions for GridDims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Debug, Clone)]
pub struct RenderCell {
    pub c: char,
    pub zerowidth: Vec<char>,
    pub fg: Rgb,
    pub bg: Rgb,
    pub fg_alpha: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub wide: bool,
    pub wide_spacer: bool,
}

impl Default for RenderCell {
    fn default() -> Self {
        Self {
            c: ' ',
            zerowidth: Vec::new(),
            fg: Rgb::new(230, 237, 243),
            bg: Rgb::new(13, 19, 26),
            fg_alpha: 1.0,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            wide: false,
            wide_spacer: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TerminalSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<RenderCell>,
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub cursor_visible: bool,
    pub cursor_style: CursorStyle,
    pub display_offset: usize,
    pub history_size: usize,
    pub mode: TermMode,
}

pub struct TerminalSession {
    pub id: u64,
    pub title: String,
    pub shell_pid: Option<u32>,
    pub term: Arc<FairMutex<Term<OceanListener>>>,
    pub notifier: Notifier,
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSession {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        id: u64,
        config: &Config,
        cli_command: Option<(String, Vec<String>)>,
        cli_cwd: Option<PathBuf>,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
        proxy: EventLoopProxy<AppEvent>,
        wakeup_queued: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let mut term_cfg = TermConfig::default();
        term_cfg.scrolling_history = config.scrollback_limit;

        let listener = OceanListener {
            tab_id: id,
            proxy,
            wakeup_queued,
        };

        let dims = GridDims {
            cols: cols as usize,
            rows: rows as usize,
        };
        let term = Arc::new(FairMutex::new(Term::new(term_cfg, &dims, listener.clone())));

        let shell = if let Some((prog, args)) = cli_command {
            Some(Shell::new(prog, args))
        } else if let Some(cmd_str) = &config.command {
            let mut parts = split_command_words(cmd_str);
            if parts.is_empty() {
                None
            } else {
                let prog = parts.remove(0);
                Some(Shell::new(prog, parts))
            }
        } else {
            None
        };

        let working_directory = cli_cwd.or_else(|| {
            config
                .working_directory
                .as_ref()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
        });

        let mut env = HashMap::new();
        env.insert("TERM".to_string(), config.term.clone());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());
        env.insert("TERM_PROGRAM".to_string(), "ocean".to_string());
        env.insert(
            "TERM_PROGRAM_VERSION".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );

        // Guarantee UTF-8 locale so Myanmar/Burmese multibyte I/O works in bash/readline/zsh/python/nvim
        let has_utf8_lang = std::env::var("LANG")
            .map(|v| v.to_ascii_uppercase().contains("UTF"))
            .unwrap_or(false);
        if !has_utf8_lang {
            env.insert("LANG".to_string(), "C.UTF-8".to_string());
        }
        for (k, v) in &config.env {
            env.insert(k.clone(), v.clone());
        }

        let pty_opts = PtyOptions {
            shell,
            working_directory,
            drain_on_exit: true,
            env,
        };

        let win_size = WindowSize {
            num_lines: rows,
            num_cols: cols,
            cell_width: cell_w.max(1),
            cell_height: cell_h.max(1),
        };

        let pty = tty::new(&pty_opts, win_size, id).map_err(|e| format!("PTY spawn failed: {e}"))?;
        #[cfg(unix)]
        let shell_pid = Some(pty.child().id());
        #[cfg(not(unix))]
        let shell_pid = None;
        let event_loop = EventLoop::new(Arc::clone(&term), listener, pty, true, false)
            .map_err(|e| format!("PTY event loop init failed: {e}"))?;
        let notifier = Notifier(event_loop.channel());
        let _io_thread = event_loop.spawn();

        let title = config
            .title
            .clone()
            .unwrap_or_else(|| "ocean".to_string());

        Ok(Self {
            id,
            title,
            shell_pid,
            term,
            notifier,
            cols,
            rows,
        })
    }

    pub fn write_bytes(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.notifier.notify(bytes);
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_w: u16, cell_h: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        let win_size = WindowSize {
            num_lines: rows,
            num_cols: cols,
            cell_width: cell_w.max(1),
            cell_height: cell_h.max(1),
        };
        let _ = self.notifier.0.send(Msg::Resize(win_size));
        let mut t = self.term.lock();
        t.resize(GridDims {
            cols: cols as usize,
            rows: rows as usize,
        });
    }

    pub fn update_config(&self, config: &Config) {
        let mut term_cfg = TermConfig::default();
        term_cfg.scrolling_history = config.scrollback_limit;
        self.term.lock().set_options(term_cfg);
    }

    pub fn scroll(&self, scroll: Scroll) {
        self.term.lock().scroll_display(scroll);
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    pub fn start_selection(&self, col: usize, row: usize, side: Side, ty: SelectionType) {
        let mut term = self.term.lock();
        let offset = term.grid().display_offset();
        let pt = viewport_to_point(offset, Point::new(row, Column(col)));
        term.selection = Some(Selection::new(ty, pt, side));
    }

    pub fn update_selection(&self, col: usize, row: usize, side: Side) {
        let mut term = self.term.lock();
        let offset = term.grid().display_offset();
        let pt = viewport_to_point(offset, Point::new(row, Column(col)));
        if let Some(sel) = term.selection.as_mut() {
            sel.update(pt, side);
        }
    }

    pub fn has_selection(&self) -> bool {
        let term = self.term.lock();
        term.selection_to_string().is_some_and(|s| !s.is_empty())
    }

    pub fn current_working_directory(&self) -> Option<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            if let Some(pid) = self.shell_pid
                && let Ok(cwd) = std::fs::read_link(format!("/proc/{pid}/cwd"))
                && cwd.is_dir()
            {
                return Some(cwd);
            }
        }
        None
    }

    pub fn selected_text(&self, trim_trailing: bool) -> Option<String> {
        let term = self.term.lock();
        let raw = term.selection_to_string()?;
        if raw.is_empty() {
            return None;
        }
        let cleaned = crate::font::myanmar::strip_myanmar_format_artifacts(&raw);
        if trim_trailing {
            let trimmed: Vec<&str> = cleaned
                .split('\n')
                .map(|l| l.trim_end_matches(' '))
                .collect();
            Some(trimmed.join("\n"))
        } else {
            Some(cleaned)
        }
    }

    pub fn extract_url_at(&self, col: usize, row: usize) -> Option<String> {
        let term = self.term.lock();
        let grid = term.grid();
        let offset = grid.display_offset();
        let line = Line(row as i32 - offset as i32);
        let num_cols = grid.columns();
        if col >= num_cols {
            return None;
        }

        // First check explicit OSC 8 hyperlink on the cell
        if let Some(link) = grid[line][Column(col)].hyperlink() {
            return Some(link.uri().to_string());
        }

        // Reconstruct text on row and locate URL spanning `col`
        let mut chars_with_col = Vec::with_capacity(num_cols);
        for c in 0..num_cols {
            let cell = &grid[line][Column(c)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            chars_with_col.push((cell.c, c));
        }
        let line_str: String = chars_with_col.iter().map(|(ch, _)| *ch).collect();

        for prefix in ["https://", "http://", "file://"] {
            let mut search_from = 0;
            while let Some(pos) = line_str[search_from..].find(prefix) {
                let byte_start = search_from + pos;
                let rest = &line_str[byte_start..];
                let url_end_len = rest
                    .find(|c: char| {
                        c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']')
                    })
                    .unwrap_or(rest.len());
                let candidate = rest[..url_end_len]
                    .trim_end_matches(['.', ',', ';', ':', '!', '?']);
                let char_start = line_str[..byte_start].chars().count();
                let char_end = char_start + candidate.chars().count();
                if char_end > char_start
                    && let (Some(&(_, start_c)), Some(&(_, end_c))) = (
                        chars_with_col.get(char_start),
                        chars_with_col.get(char_end - 1),
                    )
                    && col >= start_c
                    && col <= end_c
                {
                    return Some(candidate.to_string());
                }
                search_from = byte_start + prefix.len();
            }
        }
        None
    }

    pub fn snapshot(&self, config: &Config, snap: &mut TerminalSnapshot) {
        let term = self.term.lock();
        snapshot_from_term(&term, config, snap);
    }

    pub fn shutdown(&self) {
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn snapshot_from_term<T: EventListener>(
    term: &Term<T>,
    config: &Config,
    snap: &mut TerminalSnapshot,
) {
    let grid = term.grid();
    let cols = grid.columns();
    let rows = grid.screen_lines();
    let display_offset = grid.display_offset();
    let history_size = term.history_size();
    let mode = *term.mode();
    let sel_range = term.selection.as_ref().and_then(|s| s.to_range(term));

    let cursor_pt = grid.cursor.point;
    let cursor_shape = term.cursor_style().shape;
    let cursor_visible = mode.contains(TermMode::SHOW_CURSOR) && cursor_shape != CursorShape::Hidden;
    let cursor_style = match cursor_shape {
        CursorShape::Block => config.cursor_style,
        CursorShape::Beam => CursorStyle::Bar,
        CursorShape::Underline => CursorStyle::Underline,
        CursorShape::HollowBlock => CursorStyle::BlockHollow,
        CursorShape::Hidden => config.cursor_style,
    };

    snap.cols = cols;
    snap.rows = rows;
    snap.cursor_col = cursor_pt.column.0.min(cols.saturating_sub(1));
    snap.cursor_row = (cursor_pt.line.0.max(0) as usize).min(rows.saturating_sub(1));
    snap.cursor_visible = cursor_visible;
    snap.cursor_style = cursor_style;
    snap.display_offset = display_offset;
    snap.history_size = history_size;
    snap.mode = mode;

    snap.cells.clear();
    snap.cells.reserve(rows * cols);

    for vrow in 0..rows {
        let grid_line = Line(vrow as i32 - display_offset as i32);
        let row_ref = &grid[grid_line];

        for col in 0..cols {
            let cell = &row_ref[Column(col)];
            let flags = cell.flags;
            let bold = flags.contains(Flags::BOLD);
            let dim = flags.contains(Flags::DIM);

            let mut fg = resolve_ansi_color(cell.fg, config, bold, true);
            let mut bg = resolve_ansi_color(cell.bg, config, false, false);

            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }

            let pt = Point::new(grid_line, Column(col));
            if let Some(sr) = &sel_range
                && sr.contains(pt)
            {
                bg = config.colors.selection_background;
                if let Some(sfg) = config.colors.selection_foreground {
                    fg = sfg;
                }
            }

            let fg_alpha = if flags.contains(Flags::HIDDEN) {
                0.0
            } else if dim {
                0.65
            } else {
                1.0
            };

            let mut zw = Vec::new();
            if let Some(zslice) = cell.zerowidth()
                && !zslice.is_empty()
            {
                zw.extend_from_slice(zslice);
            }

            snap.cells.push(RenderCell {
                c: cell.c,
                zerowidth: zw,
                fg,
                bg,
                fg_alpha,
                bold,
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strikeout: flags.contains(Flags::STRIKEOUT),
                wide: flags.contains(Flags::WIDE_CHAR),
                wide_spacer: flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
            });
        }
    }
}

/// Feed raw ANSI/UTF-8 bytes into an in-memory `Term` and return a `TerminalSnapshot`
/// (used for headless screenshot rendering and automated Myanmar terminal integration tests).
pub fn snapshot_from_bytes(
    bytes: &[u8],
    config: &Config,
    cols: usize,
    rows: usize,
) -> TerminalSnapshot {
    use alacritty_terminal::event::VoidListener;
    let term_cfg = TermConfig::default();
    let dims = GridDims { cols, rows };
    let mut term = Term::new(term_cfg, &dims, VoidListener);
    let mut parser: Processor = Processor::new();
    parser.advance(&mut term, bytes);
    let mut snap = TerminalSnapshot::default();
    snapshot_from_term(&term, config, &mut snap);
    snap
}

fn resolve_ansi_color(c: AnsiColor, config: &Config, is_bold: bool, is_fg: bool) -> Rgb {
    let pal = &config.colors;
    match c {
        AnsiColor::Spec(rgb) => Rgb::new(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => {
            let mut i = idx as usize;
            if is_fg && is_bold && config.bold_is_bright && i < 8 {
                i += 8;
            }
            pal.ansi[i]
        }
        AnsiColor::Named(named) => match named {
            NamedColor::Foreground | NamedColor::BrightForeground => pal.foreground,
            NamedColor::DimForeground => pal.foreground,
            NamedColor::Background => pal.background,
            NamedColor::Cursor => pal.cursor_color,
            _ => {
                let code = named as usize;
                if code < 16 {
                    let idx = if is_fg && is_bold && config.bold_is_bright && code < 8 {
                        code + 8
                    } else {
                        code
                    };
                    pal.ansi[idx]
                } else if (16..256).contains(&code) {
                    pal.ansi[code]
                } else if is_fg {
                    pal.foreground
                } else {
                    pal.background
                }
            }
        },
    }
}

fn split_command_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' if !in_single => {
                if let Some(nc) = chars.next() {
                    cur.push(nc);
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

