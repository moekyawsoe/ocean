use unicode_width::UnicodeWidthChar;
use winit::keyboard::{Key, NamedKey};

pub const SECTION_TITLES: [&str; 5] = [
    "How to Use",
    "Splits & Tabs",
    "Shortcuts",
    "Config & Themes",
    "Myanmar",
];

#[derive(Debug, Clone, Copy)]
enum RawDocItem {
    Heading(&'static str),
    Body(&'static str),
    Bullet(&'static str),
    KeyValue(&'static str, &'static str),
    Code(&'static str),
    Spacer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyledLine {
    Heading(String),
    Body(String),
    Bullet {
        first: bool,
        text: String,
    },
    TableHeader {
        key_cols: usize,
        left: &'static str,
        right: &'static str,
    },
    KeyRow {
        key: String,
        val: String,
        key_cols: usize,
    },
    Code(String),
    Spacer,
}

#[derive(Debug, Clone, Copy)]
pub struct HelpRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl HelpRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HelpTabRect {
    pub index: usize,
    pub rect: HelpRect,
}

#[derive(Debug, Clone)]
pub struct HelpDialogGeometry {
    pub modal: HelpRect,
    pub close_btn: HelpRect,
    pub edit_btn: HelpRect,
    pub tabs: Vec<HelpTabRect>,
    pub header_h: f32,
    pub tabs_y: f32,
    pub tabs_h: f32,
    pub body_x: f32,
    pub body_y: f32,
    pub body_w: f32,
    pub body_h: f32,
    pub footer_y: f32,
    pub footer_h: f32,
    pub line_h: f32,
    pub max_cols: usize,
    pub visible_rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpClickOutcome {
    Close,
    Redraw,
    Consumed,
    EditConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpKeyOutcome {
    Close,
    Redraw,
    Consumed,
    EditConfig,
}

#[derive(Debug, Clone)]
pub struct HelpDialog {
    pub active_tab: usize,
    pub scroll_line: usize,
    pub hovered_tab: Option<usize>,
    pub close_hovered: bool,
    pub edit_hovered: bool,
    pub version: String,
}

impl Default for HelpDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpDialog {
    pub fn new() -> Self {
        Self {
            active_tab: 2,
            scroll_line: 0,
            hovered_tab: None,
            close_hovered: false,
            edit_hovered: false,
            version: format!("v{}", env!("CARGO_PKG_VERSION")),
        }
    }

    pub fn compute_geometry(
        &self,
        cell_w: f32,
        cell_h: f32,
        win_w: u32,
        win_h: u32,
        scale_factor: f64,
        bottom_reserved: f32,
    ) -> HelpDialogGeometry {
        let sf = scale_factor as f32;
        let cw = cell_w.max(6.0);
        let ch = cell_h.max(12.0);

        let margin = (16.0 * sf).round();
        let pad_x = (18.0 * sf).round();
        let right_gutter = (22.0 * sf).round();

        let w = (win_w as f32 - margin * 2.0).max(220.0).round();

        let header_h = (ch + 16.0 * sf).round();
        let tabs_h = (ch + 12.0 * sf).round();
        let footer_h = (ch + 12.0 * sf).round();
        let line_h = (ch + 3.0 * sf).round();

        let usable_h = if (win_h as f32 - bottom_reserved) >= 220.0 {
            win_h as f32 - bottom_reserved
        } else {
            win_h as f32
        };

        let h = (usable_h - margin * 2.0).max(180.0).round();

        let x = ((win_w as f32 - w) * 0.5).floor().max(4.0);
        let y = ((usable_h - h) * 0.5).floor().max(4.0);

        let close_w = (cw * 3.6 + 8.0 * sf).round();
        let close_h = (ch + 6.0 * sf).round();
        let close_btn = HelpRect {
            x: (x + w - close_w - 10.0 * sf).round(),
            y: (y + (header_h - close_h) * 0.5).round(),
            w: close_w,
            h: close_h,
        };

        let edit_label_w = (cw * 20.0 + 14.0 * sf).round();
        let edit_w = edit_label_w.min((w - close_w - 60.0 * sf).max(40.0));
        let edit_btn = HelpRect {
            x: (close_btn.x - edit_w - 8.0 * sf).round(),
            y: close_btn.y,
            w: edit_w,
            h: close_h,
        };

        let tabs_y = y + header_h;
        let mut tabs = Vec::with_capacity(SECTION_TITLES.len());
        let mut tx = x + (10.0 * sf).round();
        let tab_gap = (4.0 * sf).round();
        let avail_tabs_w = (w - 20.0 * sf).max(120.0);
        let natural_total: f32 = SECTION_TITLES
            .iter()
            .map(|t| (t.chars().count() as f32 * cw + 18.0 * sf).round())
            .sum::<f32>()
            + tab_gap * (SECTION_TITLES.len().saturating_sub(1) as f32);
        let shrink = if natural_total > avail_tabs_w {
            avail_tabs_w / natural_total
        } else {
            1.0
        };

        for (idx, title) in SECTION_TITLES.iter().enumerate() {
            let tw = ((title.chars().count() as f32 * cw + 18.0 * sf) * shrink)
                .floor()
                .max(36.0);
            tabs.push(HelpTabRect {
                index: idx,
                rect: HelpRect {
                    x: tx,
                    y: tabs_y + (2.0 * sf).round(),
                    w: tw,
                    h: (tabs_h - 3.0 * sf).max(ch),
                },
            });
            tx += tw + (tab_gap * shrink).floor();
        }

        let body_pad_v = (8.0 * sf).round();
        let body_x = x + pad_x;
        let body_y = tabs_y + tabs_h + body_pad_v;
        let footer_y = (y + h - footer_h).round();
        let body_w = (w - pad_x - right_gutter).max(cw * 16.0);
        let body_h = (footer_y - body_pad_v - body_y).max(line_h);

        let max_cols = (body_w / cw).floor().max(18.0) as usize;
        let visible_rows = (body_h / line_h).floor().max(1.0) as usize;

        HelpDialogGeometry {
            modal: HelpRect { x, y, w, h },
            close_btn,
            edit_btn,
            tabs,
            header_h,
            tabs_y,
            tabs_h,
            body_x,
            body_y,
            body_w,
            body_h,
            footer_y,
            footer_h,
            line_h,
            max_cols,
            visible_rows,
        }
    }

    pub fn wrapped_lines(&self, max_cols: usize) -> Vec<StyledLine> {
        let cols = max_cols.max(20);
        let items = section_content(self.active_tab);
        let mut out = Vec::with_capacity(items.len() * 2);
        let mut table_open = false;

        for item in items {
            match item {
                RawDocItem::Heading(text) => {
                    for line in wrap_words(text, cols) {
                        out.push(StyledLine::Heading(line));
                    }
                }
                RawDocItem::Body(text) => {
                    for line in wrap_words(text, cols) {
                        out.push(StyledLine::Body(line));
                    }
                }
                RawDocItem::Bullet(text) => {
                    let inner_cols = cols.saturating_sub(2).max(12);
                    for (i, line) in wrap_words(text, inner_cols).into_iter().enumerate() {
                        out.push(StyledLine::Bullet {
                            first: i == 0,
                            text: line,
                        });
                    }
                }
                RawDocItem::KeyValue(key, desc) => {
                    let key_cols = 34usize;
                    if cols >= 60 && display_cols(key) + 2 <= key_cols {
                        if !table_open {
                            let (left, right) = if self.active_tab == 2 {
                                ("Shortcut", "Action")
                            } else {
                                ("Setting", "Value")
                            };
                            out.push(StyledLine::TableHeader {
                                key_cols,
                                left,
                                right,
                            });
                            table_open = true;
                        }
                        let val_cols = cols.saturating_sub(key_cols).max(14);
                        for (i, line) in wrap_words(desc, val_cols).into_iter().enumerate() {
                            out.push(StyledLine::KeyRow {
                                key: if i == 0 {
                                    key.to_string()
                                } else {
                                    String::new()
                                },
                                val: line,
                                key_cols,
                            });
                        }
                    } else {
                        table_open = false;
                        out.push(StyledLine::KeyRow {
                            key: key.to_string(),
                            val: String::new(),
                            key_cols: cols,
                        });
                        let indent_cols = cols.saturating_sub(2).max(12);
                        for line in wrap_words(desc, indent_cols) {
                            out.push(StyledLine::Bullet {
                                first: false,
                                text: line,
                            });
                        }
                    }
                }
                RawDocItem::Code(text) => {
                    let inner = cols.saturating_sub(2).max(12);
                    for line in wrap_words(text, inner) {
                        out.push(StyledLine::Code(format!(" {line}")));
                    }
                }
                RawDocItem::Spacer => {
                    table_open = false;
                    out.push(StyledLine::Spacer);
                }
            }

            if !matches!(item, RawDocItem::KeyValue(_, _)) {
                table_open = false;
            }
        }

        out
    }

    pub fn clamped_scroll(&self, total_lines: usize, visible_rows: usize) -> (usize, usize) {
        let max_scroll = total_lines.saturating_sub(visible_rows);
        (self.scroll_line.min(max_scroll), max_scroll)
    }

    /// Update mouse hover state. Returns `(needs_redraw, pointer_cursor)`.
    pub fn update_hover(&mut self, geom: &HelpDialogGeometry, px: f32, py: f32) -> (bool, bool) {
        let prev = (self.hovered_tab, self.close_hovered, self.edit_hovered);
        self.close_hovered = geom.close_btn.contains(px, py);
        self.edit_hovered = geom.edit_btn.contains(px, py);
        self.hovered_tab = if self.close_hovered || self.edit_hovered {
            None
        } else {
            geom.tabs
                .iter()
                .find(|t| t.rect.contains(px, py))
                .map(|t| t.index)
        };
        let changed = (self.hovered_tab, self.close_hovered, self.edit_hovered) != prev;
        let pointer = self.close_hovered || self.edit_hovered || self.hovered_tab.is_some();
        (changed, pointer)
    }

    pub fn handle_click(
        &mut self,
        geom: &HelpDialogGeometry,
        px: f32,
        py: f32,
    ) -> HelpClickOutcome {
        if !geom.modal.contains(px, py) || geom.close_btn.contains(px, py) {
            return HelpClickOutcome::Close;
        }
        if geom.edit_btn.contains(px, py) {
            return HelpClickOutcome::EditConfig;
        }
        if let Some(tab) = geom.tabs.iter().find(|t| t.rect.contains(px, py)) {
            if self.active_tab != tab.index {
                self.active_tab = tab.index;
                self.scroll_line = 0;
                return HelpClickOutcome::Redraw;
            }
            return HelpClickOutcome::Consumed;
        }
        HelpClickOutcome::Consumed
    }

    pub fn scroll_lines(
        &mut self,
        delta_down: i32,
        total_lines: usize,
        visible_rows: usize,
    ) -> bool {
        let (cur, max_scroll) = self.clamped_scroll(total_lines, visible_rows);
        let next = (cur as i32 + delta_down).clamp(0, max_scroll as i32) as usize;
        if next != self.scroll_line {
            self.scroll_line = next;
            true
        } else {
            false
        }
    }

    pub fn handle_key(
        &mut self,
        key: &Key,
        shift_held: bool,
        total_lines: usize,
        visible_rows: usize,
    ) -> HelpKeyOutcome {
        let (cur, max_scroll) = self.clamped_scroll(total_lines, visible_rows);
        let page = visible_rows.saturating_sub(2).max(1) as i32;

        match key {
            Key::Named(NamedKey::Escape) => HelpKeyOutcome::Close,
            Key::Character(s) if s.eq_ignore_ascii_case("q") => HelpKeyOutcome::Close,
            Key::Character(s) if s.eq_ignore_ascii_case("e") => HelpKeyOutcome::EditConfig,
            Key::Named(NamedKey::Enter) if self.edit_hovered => HelpKeyOutcome::EditConfig,
            Key::Named(NamedKey::ArrowRight) => {
                self.active_tab = (self.active_tab + 1) % SECTION_TITLES.len();
                self.scroll_line = 0;
                HelpKeyOutcome::Redraw
            }
            Key::Character(s) if s == "l" => {
                self.active_tab = (self.active_tab + 1) % SECTION_TITLES.len();
                self.scroll_line = 0;
                HelpKeyOutcome::Redraw
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.active_tab =
                    (self.active_tab + SECTION_TITLES.len() - 1) % SECTION_TITLES.len();
                self.scroll_line = 0;
                HelpKeyOutcome::Redraw
            }
            Key::Character(s) if s == "h" => {
                self.active_tab =
                    (self.active_tab + SECTION_TITLES.len() - 1) % SECTION_TITLES.len();
                self.scroll_line = 0;
                HelpKeyOutcome::Redraw
            }
            Key::Named(NamedKey::Tab) => {
                if shift_held {
                    self.active_tab =
                        (self.active_tab + SECTION_TITLES.len() - 1) % SECTION_TITLES.len();
                } else {
                    self.active_tab = (self.active_tab + 1) % SECTION_TITLES.len();
                }
                self.scroll_line = 0;
                HelpKeyOutcome::Redraw
            }
            Key::Character(s) if matches!(s.as_str(), "1" | "2" | "3" | "4" | "5") => {
                let idx = (s.as_bytes()[0] - b'1') as usize;
                if idx != self.active_tab {
                    self.active_tab = idx;
                    self.scroll_line = 0;
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.scroll_line = cur;
                if self.scroll_lines(1, total_lines, visible_rows) {
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Character(s) if s == "j" => {
                self.scroll_line = cur;
                if self.scroll_lines(1, total_lines, visible_rows) {
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.scroll_line = cur;
                if self.scroll_lines(-1, total_lines, visible_rows) {
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Character(s) if s == "k" => {
                self.scroll_line = cur;
                if self.scroll_lines(-1, total_lines, visible_rows) {
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Named(NamedKey::PageDown | NamedKey::Space) => {
                self.scroll_line = cur;
                let step = if shift_held && matches!(key, Key::Named(NamedKey::Space)) {
                    -page
                } else {
                    page
                };
                if self.scroll_lines(step, total_lines, visible_rows) {
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Named(NamedKey::PageUp) => {
                self.scroll_line = cur;
                if self.scroll_lines(-page, total_lines, visible_rows) {
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Named(NamedKey::Home) => {
                if cur != 0 {
                    self.scroll_line = 0;
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Character(s) if s == "g" && !shift_held => {
                if cur != 0 {
                    self.scroll_line = 0;
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Named(NamedKey::End) => {
                if cur != max_scroll {
                    self.scroll_line = max_scroll;
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            Key::Character(s) if s == "G" || (s == "g" && shift_held) => {
                if cur != max_scroll {
                    self.scroll_line = max_scroll;
                    HelpKeyOutcome::Redraw
                } else {
                    HelpKeyOutcome::Consumed
                }
            }
            _ => HelpKeyOutcome::Consumed,
        }
    }
}

fn display_cols(s: &str) -> usize {
    s.chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(1))
        .sum()
}

fn wrap_words(text: &str, max_cols: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let limit = max_cols.max(8);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut cur_w = 0usize;

    for word in text.split_whitespace() {
        let word_w = display_cols(word);
        if cur_w == 0 {
            if word_w <= limit {
                current.push_str(word);
                cur_w = word_w;
            } else {
                push_hard_wrapped(word, limit, &mut lines, &mut current, &mut cur_w);
            }
        } else if cur_w + 1 + word_w <= limit {
            current.push(' ');
            current.push_str(word);
            cur_w += 1 + word_w;
        } else {
            lines.push(std::mem::take(&mut current));
            cur_w = 0;
            if word_w <= limit {
                current.push_str(word);
                cur_w = word_w;
            } else {
                push_hard_wrapped(word, limit, &mut lines, &mut current, &mut cur_w);
            }
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn push_hard_wrapped(
    word: &str,
    limit: usize,
    lines: &mut Vec<String>,
    current: &mut String,
    cur_w: &mut usize,
) {
    for ch in word.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
        if *cur_w + cw > limit && *cur_w > 0 {
            lines.push(std::mem::take(current));
            *cur_w = 0;
        }
        current.push(ch);
        *cur_w += cw;
    }
}

fn section_content(tab: usize) -> &'static [RawDocItem] {
    match tab {
        0 => &[
            RawDocItem::Heading("Welcome to Ocean Terminal"),
            RawDocItem::Body(
                "Ocean is a fast, single-draw-call GPU terminal emulator built for keyboard-first developers, with multi-pane splits, tabs, a Neovim-style bottom shortcut bar, 10 built-in dark & light themes, and native OpenType Myanmar/Burmese shaping.",
            ),
            RawDocItem::Spacer,
            RawDocItem::Heading("Keyboard-First Context Menu & Bottom Bar"),
            RawDocItem::Bullet(
                "Press Ctrl+Shift+P (or Shift+F10 / Menu key, or Right-Click) anytime to open the Context Menu right at your terminal cursor — no mouse needed!",
            ),
            RawDocItem::Bullet(
                "Inside the Context Menu, navigate with j/k or Up/Down, press Enter to select, press 't' to jump straight into Switch Theme (1-0 quick-picks any of the 10 themes), or press single-key mnemonics (c, p, d, e, n, b, ?, w).",
            ),
            RawDocItem::Bullet(
                "The Neovim-style bottom status bar displays your current mode (OCEAN, TUI, SCROLL, MENU, THEME, HELP) and context-aware keyboard shortcuts. Toggle it anytime with Ctrl+Shift+B.",
            ),
            RawDocItem::Spacer,
            RawDocItem::Heading("Mouse Selection, Clipboard & Links"),
            RawDocItem::Bullet(
                "Copy with Ctrl+Shift+C and Paste with Ctrl+Shift+V. Inside TUI apps that capture the mouse (nvim, tmux, htop), Ctrl+Shift+P opens Ocean's menu via keyboard, or hold Shift + Right-Click / Left-Drag.",
            ),
            RawDocItem::Bullet(
                "Ctrl + Left-Click any http/https URL in terminal output to open it in your default browser.",
            ),
        ],
        1 => &[
            RawDocItem::Heading("Multi-Pane Splits (100% Keyboard Driven)"),
            RawDocItem::Bullet(
                "Split Right (side-by-side): press Ctrl+Shift+D (or Ctrl+Shift+P -> d).",
            ),
            RawDocItem::Bullet(
                "Split Down (top-and-bottom): press Ctrl+Shift+E (or Ctrl+Shift+P -> e).",
            ),
            RawDocItem::Bullet(
                "Focus Splits (Vim HJKL or Arrows): press Alt+Shift+H / J / K / L or Alt+Shift+Left / Down / Up / Right.",
            ),
            RawDocItem::Bullet(
                "Resize Splits via Keyboard: press Ctrl+Alt+H / J / K / L or Ctrl+Alt+Arrows (or drag divider bars). Equalize all splits with Alt+Shift+=.",
            ),
            RawDocItem::Bullet(
                "Working Directory: new splits and tabs automatically inherit the active shell's current directory.",
            ),
            RawDocItem::Spacer,
            RawDocItem::Heading("Tabs & Tab Bar"),
            RawDocItem::Bullet("New Tab: press Ctrl+Shift+T (or click '+' on the tab bar)."),
            RawDocItem::Bullet(
                "Switch Tabs: cycle with Ctrl+Tab / Ctrl+Shift+Tab or Ctrl+PgDn / Ctrl+PgUp.",
            ),
            RawDocItem::Bullet(
                "Close Split vs Close Tab: Ctrl+Shift+W closes the active split pane (or the tab when only 1 pane remains).",
            ),
            RawDocItem::Bullet(
                "Tab & Shortcut Bars: `window-show-tab-bar = auto` hides the top bar when 1 tab is open; `window-show-status-bar = true` shows the bottom shortcut bar.",
            ),
        ],
        2 => &[
            RawDocItem::Heading("Default Developer Keyboard Shortcuts"),
            RawDocItem::KeyValue(
                "Ctrl+Shift+P / Menu",
                "Open/close Context Menu at cursor (j/k/t/1-9)",
            ),
            RawDocItem::KeyValue(
                "Ctrl+Shift+B",
                "Show or hide bottom Neovim-style shortcut bar",
            ),
            RawDocItem::KeyValue("Ctrl+Shift+C / Ctrl+Ins", "Copy selection to clipboard"),
            RawDocItem::KeyValue("Ctrl+Shift+V / Shift+Ins", "Paste from clipboard"),
            RawDocItem::KeyValue("Ctrl+Shift+D / E", "Split active pane Right / Down"),
            RawDocItem::KeyValue("Alt+Shift+HJKL / Arrows", "Move focus between split panes"),
            RawDocItem::KeyValue("Ctrl+Alt+HJKL / Arrows", "Resize active split pane divider"),
            RawDocItem::KeyValue("Alt+Shift+=", "Equalize all split pane sizes"),
            RawDocItem::KeyValue("Ctrl+Shift+T / W", "New tab / Close active split or tab"),
            RawDocItem::KeyValue("Ctrl+Tab / Ctrl+Shift+Tab", "Switch to next / previous tab"),
            RawDocItem::KeyValue("Ctrl+Shift+N", "Spawn a new Ocean window"),
            RawDocItem::KeyValue(
                "Ctrl+= / Ctrl+- / Ctrl+0",
                "Increase / decrease / reset font size",
            ),
            RawDocItem::KeyValue(
                "Shift+PgUp / Shift+PgDn",
                "Scroll history up / down one page",
            ),
            RawDocItem::KeyValue(
                "Shift+Home / Shift+End",
                "Jump to top / bottom of scrollback",
            ),
            RawDocItem::KeyValue(
                "Ctrl+Shift+R / Ctrl+Shift+,",
                "Reload ~/.config/ocean/config live",
            ),
            RawDocItem::KeyValue("Ctrl+Shift+M", "Toggle legacy Zawgyi-One display mode"),
            RawDocItem::KeyValue(
                "Ctrl+Shift+I / Ctrl+\\",
                "Toggle built-in editor / editor+terminal split",
            ),
            RawDocItem::KeyValue(
                "edit [path] / Ctrl+Shift+O",
                "Open the code editor in a separate window and keep the terminal window",
            ),
            RawDocItem::KeyValue(
                "edit --here [path] (in Ocean shell)",
                "Switch this window into editor mode without opening a new window",
            ),
            RawDocItem::KeyValue(
                "Dirty close / quit",
                "Prompts Save / Discard / Cancel; :q! is the explicit force-discard",
            ),
            RawDocItem::KeyValue(
                "F10 / Edit menu bar",
                "File / Edit / View / Go / Help menus with recent files and quick open",
            ),
            RawDocItem::KeyValue(
                "Ctrl+P / Ctrl+F",
                "Bottom `:` command palette / bottom `/` in-file search line",
            ),
            RawDocItem::KeyValue(
                "Ctrl+Shift+F / Ctrl+Enter",
                "Workspace search panel; Ctrl+Enter replaces all after confirmation",
            ),
            RawDocItem::KeyValue(
                "Ctrl+Shift+G",
                "Git changes panel: branch, modified/untracked files, gutter markers and inline blame",
            ),
            RawDocItem::KeyValue(
                "Alt+Click / Ctrl+Alt+Up/Down",
                "Add cursors; typing and edits apply to every cursor (Esc clears)",
            ),
            RawDocItem::KeyValue(
                "ocean +plugin-install / +plugin-list",
                "Install, list, enable/disable, verify, update, or remove WASM plugins",
            ),
            RawDocItem::KeyValue(
                "restore-session / autosave-interval",
                "Restore open docs and snapshot dirty buffers for crash recovery",
            ),
            RawDocItem::KeyValue(
                "editor-keymap = vim",
                "Enable Normal/Insert/Visual modes (i, Esc, v, V, d/y/c, :w/:q)",
            ),
            RawDocItem::KeyValue(
                "LSP servers on PATH",
                "HTML/CSS/JSON/YAML/TOML/Shell/Markdown use their language servers when installed",
            ),
            RawDocItem::KeyValue("Ctrl+Shift+?", "Open or close this Help & Docs dialog"),
            RawDocItem::KeyValue(
                "e (inside Help) / nano",
                "View and edit ~/.config/ocean/config in nano",
            ),
            RawDocItem::KeyValue(
                "Context Menu -> u",
                "Upgrade to latest release when update badge appears",
            ),
            RawDocItem::KeyValue("F11 / Ctrl+Shift+Q", "Toggle fullscreen / Quit Ocean"),
        ],
        3 => &[
            RawDocItem::Heading("Configuration (~/.config/ocean/config)"),
            RawDocItem::Body(
                "Ocean uses Ghostty-compatible `key = value` configuration and hot-reloads automatically when you save the file or press Ctrl+Shift+R.",
            ),
            RawDocItem::Spacer,
            RawDocItem::Heading("Editor Typography"),
            RawDocItem::Code(
                "editor-font-scale = 0.92   # fraction of font-size for editor windows",
            ),
            RawDocItem::Code(
                "font-size = 13.5           # shared base size for terminal and editor",
            ),
            RawDocItem::Spacer,
            RawDocItem::Heading("View & Edit Config in Terminal (nano)"),
            RawDocItem::Bullet(
                "Press 'e' right now in this dialog (or click [Edit Config (nano)] above) to open ~/.config/ocean/config directly in nano on the terminal.",
            ),
            RawDocItem::Bullet(
                "Save changes in nano with Ctrl+O (Ocean hot-reloads live instantly). Exit nano anytime with Ctrl+X to return to your previous session.",
            ),
            RawDocItem::Code("ocean +edit-config   # launch nano on config from shell"),
            RawDocItem::Code("ocean +init-config   # create starter ~/.config/ocean/config"),
            RawDocItem::Code("ocean +show-config   # inspect active resolved settings"),
            RawDocItem::Spacer,
            RawDocItem::Heading("Bottom Shortcut Bar & Borderless Mode"),
            RawDocItem::Code("window-show-status-bar = true   # toggle live: Ctrl+Shift+B"),
            RawDocItem::Code("window-titlebar = false         # hide system title bar"),
            RawDocItem::Bullet(
                "Customize any shortcut in ~/.config/ocean/config and both the bottom bar and context menu update automatically!",
            ),
            RawDocItem::Spacer,
            RawDocItem::Heading("10 Built-In Themes (5 Dark + 5 Light)"),
            RawDocItem::Body(
                "Switch with keyboard via Ctrl+Shift+P -> t -> 1..0 (or j/k + Enter), or set `theme = <name>` in config:",
            ),
            RawDocItem::KeyValue(
                "5 Dark Themes",
                "ocean-dark, tokyo-night, catppuccin-mocha, gruvbox-dark, nord",
            ),
            RawDocItem::KeyValue(
                "5 Light Themes",
                "ocean-light, tokyo-night-light, catppuccin-latte, gruvbox-light, solarized-light",
            ),
            RawDocItem::Spacer,
            RawDocItem::Heading("Auto Updates & GitHub Releases"),
            RawDocItem::Bullet(
                "Ocean automatically checks for latest releases on GitHub in the background without blocking startup or input latency.",
            ),
            RawDocItem::Code("auto-update-check = daily   # daily | always | off"),
            RawDocItem::Code("ocean +check-update         # check for newer version online"),
            RawDocItem::Code("ocean +update               # upgrade directly to latest version"),
        ],
        _ => &[
            RawDocItem::Heading("Myanmar / Burmese Script Support (မြန်မာဘာသာ)"),
            RawDocItem::Body(
                "Ocean shapes Myanmar Unicode runs (U+1000..U+109F, U+AA60..U+AA7F, U+A9E0..U+A9FF) through HarfBuzz (`rustybuzz`) with OpenType `mym2`/`mymr` tables.",
            ),
            RawDocItem::Bullet(
                "Renders Kinzi, pre-base vowel E, medial Ra/Ya/Wa/Ha, stacked consonants, tall AA, and Myanmar digits without dotted-circle artifacts.",
            ),
            RawDocItem::Bullet(
                "UTN #11 combining-mark normalization automatically fixes common typing-order variations.",
            ),
            RawDocItem::Bullet(
                "Full Wayland & X11 IME support (IBus, Fcitx5, Keyman) with live shaped preedit at the cursor.",
            ),
            RawDocItem::Spacer,
            RawDocItem::Heading("Myanmar Font & Legacy Zawgyi Settings"),
            RawDocItem::Code("font-family-myanmar = Padauk"),
            RawDocItem::Code("myanmar-font-scale  = 1.12"),
            RawDocItem::Code("myanmar-zawgyi      = off   # toggle live with Ctrl+Shift+M"),
            RawDocItem::Spacer,
            RawDocItem::Bullet(
                "Run `ocean +myanmar-check` or `ocean +list-fonts` in your shell to verify installed Burmese fonts and shaping.",
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_dialog_wraps_all_sections_within_column_budget() {
        let mut dlg = HelpDialog::new();
        assert!(dlg.version.starts_with('v'));
        let geom = dlg.compute_geometry(9.0, 20.0, 960, 600, 1.0, 26.0);
        assert_eq!(geom.tabs.len(), 5);
        assert!(geom.visible_rows >= 10);
        assert!(geom.modal.w >= 900.0);
        assert!(geom.modal.y + geom.modal.h <= 600.0 - 26.0);
        dlg.active_tab = 2;
        assert!(
            dlg.wrapped_lines(geom.max_cols)
                .iter()
                .any(|line| matches!(line, StyledLine::TableHeader { .. }))
        );

        for section in 0..SECTION_TITLES.len() {
            dlg.active_tab = section;
            let lines = dlg.wrapped_lines(geom.max_cols);
            assert!(!lines.is_empty());
            for line in &lines {
                match line {
                    StyledLine::Heading(s) | StyledLine::Body(s) | StyledLine::Code(s) => {
                        assert!(display_cols(s) <= geom.max_cols);
                    }
                    StyledLine::Bullet { text, .. } => {
                        assert!(display_cols(text) + 2 <= geom.max_cols);
                    }
                    StyledLine::TableHeader { key_cols, .. } => {
                        assert!(*key_cols < geom.max_cols);
                    }
                    StyledLine::KeyRow { key, val, key_cols } => {
                        if val.is_empty() {
                            assert!(display_cols(key) <= geom.max_cols);
                        } else {
                            assert!(key_cols + display_cols(val) <= geom.max_cols);
                        }
                    }
                    StyledLine::Spacer => {}
                }
            }
        }

        // Verify Vim `l`/`h` + Arrow keyboard tab switching and Escape close
        dlg.active_tab = 0;
        let total = dlg.wrapped_lines(geom.max_cols).len();
        assert_eq!(
            dlg.handle_key(&Key::Character("l".into()), false, total, geom.visible_rows),
            HelpKeyOutcome::Redraw
        );
        assert_eq!(dlg.active_tab, 1);
        assert_eq!(
            dlg.handle_key(&Key::Character("h".into()), false, total, geom.visible_rows),
            HelpKeyOutcome::Redraw
        );
        assert_eq!(dlg.active_tab, 0);
        assert_eq!(
            dlg.handle_key(
                &Key::Named(NamedKey::Escape),
                false,
                total,
                geom.visible_rows
            ),
            HelpKeyOutcome::Close
        );

        // Verify [Edit Config (nano)] button geometry, hover, click, and 'e' key
        assert!(geom.edit_btn.w > 40.0);
        assert!(geom.edit_btn.x < geom.close_btn.x);
        let eb_cx = geom.edit_btn.x + geom.edit_btn.w * 0.5;
        let eb_cy = geom.edit_btn.y + geom.edit_btn.h * 0.5;
        let (_, pointer) = dlg.update_hover(&geom, eb_cx, eb_cy);
        assert!(pointer && dlg.edit_hovered);
        assert_eq!(
            dlg.handle_click(&geom, eb_cx, eb_cy),
            HelpClickOutcome::EditConfig
        );
        assert_eq!(
            dlg.handle_key(&Key::Character("e".into()), false, total, geom.visible_rows),
            HelpKeyOutcome::EditConfig
        );
    }
}
