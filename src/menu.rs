use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::config::keybind::{Action, Keybind, SplitDirection, shortcut_for_action};
use crate::config::theme::{ThemeInfo, ThemePalette};

#[derive(Debug, Clone)]
pub enum MenuEntry {
    Item(MenuItem),
    ThemeSubmenu { label: &'static str },
    Separator,
}

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: &'static str,
    pub shortcut: String,
    pub action: Action,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub enum ThemeSubEntry {
    Header(&'static str),
    Theme(ThemeInfo),
    Separator,
}

#[derive(Debug, Clone)]
pub struct MenuRowRect {
    pub entry_index: usize,
    pub y: f32,
    pub h: f32,
    pub is_separator: bool,
}

#[derive(Debug, Clone)]
pub struct SubmenuGeometry {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub pad_x: f32,
    pub rows: Vec<MenuRowRect>,
}

impl SubmenuGeometry {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Debug, Clone)]
pub struct MenuGeometry {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub pad_x: f32,
    pub rows: Vec<MenuRowRect>,
    pub submenu: Option<SubmenuGeometry>,
}

impl MenuGeometry {
    pub fn contains_main(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MenuClickOutcome {
    Execute(Action),
    ConsumedKeepOpen,
    Outside,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MenuKeyOutcome {
    NotHandled,
    Redraw,
    Close,
    Execute(Action),
}

#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub anchor_x: f32,
    pub anchor_y: f32,
    pub entries: Vec<MenuEntry>,
    pub hovered: Option<usize>,
    pub submenu_open: bool,
    pub sub_entries: Vec<ThemeSubEntry>,
    pub sub_hovered: Option<usize>,
    pub committed_theme: Option<String>,
}

impl ContextMenu {
    pub fn build(
        anchor_x: f32,
        anchor_y: f32,
        has_selection: bool,
        num_panes_in_tab: usize,
        status_bar_visible: bool,
        keybinds: &[Keybind],
        available_update: Option<&crate::update::UpdateInfo>,
    ) -> Self {
        let sc = |act: Action, fallback: &str| -> String {
            shortcut_for_action(keybinds, &act).unwrap_or_else(|| fallback.to_string())
        };

        let mut entries = vec![
            MenuEntry::Item(MenuItem {
                label: "Copy",
                shortcut: sc(Action::CopyToClipboard, "Ctrl+Shift+C"),
                action: Action::CopyToClipboard,
                enabled: has_selection,
            }),
            MenuEntry::Item(MenuItem {
                label: "Paste",
                shortcut: sc(Action::PasteFromClipboard, "Ctrl+Shift+V"),
                action: Action::PasteFromClipboard,
                enabled: true,
            }),
            MenuEntry::Separator,
            MenuEntry::Item(MenuItem {
                label: "Split Right",
                shortcut: sc(Action::NewSplit(SplitDirection::Right), "Ctrl+Shift+D"),
                action: Action::NewSplit(SplitDirection::Right),
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                label: "Split Down",
                shortcut: sc(Action::NewSplit(SplitDirection::Down), "Ctrl+Shift+E"),
                action: Action::NewSplit(SplitDirection::Down),
                enabled: true,
            }),
        ];

        if num_panes_in_tab > 1 {
            entries.push(MenuEntry::Item(MenuItem {
                label: "Equalize Splits",
                shortcut: sc(Action::EqualizeSplits, "Alt+Shift+="),
                action: Action::EqualizeSplits,
                enabled: true,
            }));
        }

        entries.extend([
            MenuEntry::Item(MenuItem {
                label: "New Tab",
                shortcut: sc(Action::NewTab, "Ctrl+Shift+T"),
                action: Action::NewTab,
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                label: "Toggle Code Editor",
                shortcut: sc(Action::ToggleEditor, "Ctrl+Shift+I"),
                action: Action::ToggleEditor,
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                label: "Toggle Split Mode",
                shortcut: sc(Action::ToggleSplitMode, "Ctrl+\\"),
                action: Action::ToggleSplitMode,
                enabled: true,
            }),
            MenuEntry::Separator,
            MenuEntry::ThemeSubmenu {
                label: "Switch Theme",
            },
            MenuEntry::Item(MenuItem {
                label: if status_bar_visible {
                    "Hide Shortcut Bar"
                } else {
                    "Show Shortcut Bar"
                },
                shortcut: sc(Action::ToggleStatusBar, "Ctrl+Shift+B"),
                action: Action::ToggleStatusBar,
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                label: "Help & Docs",
                shortcut: sc(Action::ShowHelp, "Ctrl+Shift+?"),
                action: Action::ShowHelp,
                enabled: true,
            }),
            MenuEntry::Item(MenuItem {
                label: "Edit Config (nano)",
                shortcut: "e in help".to_string(),
                action: Action::EditConfig,
                enabled: true,
            }),
            MenuEntry::Separator,
        ]);

        if num_panes_in_tab > 1 {
            entries.push(MenuEntry::Item(MenuItem {
                label: "Close Split",
                shortcut: sc(Action::CloseSurface, "Ctrl+Shift+W"),
                action: Action::CloseSurface,
                enabled: true,
            }));
            entries.push(MenuEntry::Item(MenuItem {
                label: "Close Tab",
                shortcut: sc(Action::CloseTab, ""),
                action: Action::CloseTab,
                enabled: true,
            }));
        } else {
            entries.push(MenuEntry::Item(MenuItem {
                label: "Close Tab",
                shortcut: sc(Action::CloseSurface, "Ctrl+Shift+W"),
                action: Action::CloseSurface,
                enabled: true,
            }));
        }

        if let Some(up) = available_update {
            let label =
                Box::leak(format!("Update to {} (Click to Install)", up.tag_name).into_boxed_str());
            entries.push(MenuEntry::Separator);
            entries.push(MenuEntry::Item(MenuItem {
                label,
                shortcut: "u".to_string(),
                action: Action::TriggerUpdate,
                enabled: true,
            }));
        }

        let curated = ThemePalette::curated_themes();
        let mut sub_entries = Vec::with_capacity(13);
        sub_entries.push(ThemeSubEntry::Header("Dark Themes (5)"));
        for info in curated.iter().filter(|t| t.is_dark) {
            sub_entries.push(ThemeSubEntry::Theme(*info));
        }
        sub_entries.push(ThemeSubEntry::Separator);
        sub_entries.push(ThemeSubEntry::Header("Light Themes (5)"));
        for info in curated.iter().filter(|t| !t.is_dark) {
            sub_entries.push(ThemeSubEntry::Theme(*info));
        }

        Self {
            anchor_x,
            anchor_y,
            entries,
            hovered: None,
            submenu_open: false,
            sub_entries,
            sub_hovered: None,
            committed_theme: None,
        }
    }

    pub fn hovered_theme_id(&self) -> Option<&'static str> {
        if self.submenu_open
            && let Some(sub_idx) = self.sub_hovered
            && let Some(ThemeSubEntry::Theme(info)) = self.sub_entries.get(sub_idx) {
                return Some(info.id);
            }
        None
    }

    pub fn select_first_enabled(&mut self) {
        self.hovered = self.enabled_main_indices().into_iter().next();
        self.sub_hovered = None;
        if let Some(idx) = self.hovered {
            self.submenu_open = matches!(self.entries[idx], MenuEntry::ThemeSubmenu { .. });
        }
    }

    pub fn open_theme_submenu(&mut self, active_theme: &str) {
        self.committed_theme = Some(active_theme.to_string());
        if let Some(idx) = self.theme_submenu_entry_index() {
            self.hovered = Some(idx);
            self.open_submenu_with_keyboard(active_theme);
        }
    }

    pub fn theme_submenu_entry_index(&self) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| matches!(e, MenuEntry::ThemeSubmenu { .. }))
    }

    fn enabled_main_indices(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                MenuEntry::Item(it) if it.enabled => Some(i),
                MenuEntry::ThemeSubmenu { .. } => Some(i),
                _ => None,
            })
            .collect()
    }

    fn theme_sub_indices(&self) -> Vec<usize> {
        self.sub_entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| matches!(e, ThemeSubEntry::Theme(_)).then_some(i))
            .collect()
    }

    pub fn compute_geometry(
        &self,
        cell_w: f32,
        cell_h: f32,
        win_w: u32,
        win_h: u32,
        scale_factor: f64,
        bottom_reserved: f32,
    ) -> MenuGeometry {
        let sf = scale_factor as f32;
        let cw = cell_w.max(6.0);
        let ch = cell_h.max(12.0);
        let pad_x = (12.0 * sf).round();
        let pad_y = (6.0 * sf).round();
        let row_h = (ch + 9.0 * sf).round();
        let sep_h = (8.0 * sf).round();

        let max_chars = self
            .entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item(it) => {
                    let label_len = it.label.chars().count();
                    let sc_len = if it.shortcut.is_empty() {
                        0
                    } else {
                        4 + it.shortcut.chars().count()
                    };
                    Some(label_len + sc_len)
                }
                MenuEntry::ThemeSubmenu { label } => Some(label.chars().count() + 6),
                MenuEntry::Separator => None,
            })
            .max()
            .unwrap_or(28)
            .max(28);

        let w = (pad_x * 2.0 + max_chars as f32 * cw)
            .round()
            .min((win_w as f32 - 8.0).max(120.0));

        let mut rows = Vec::with_capacity(self.entries.len());
        let mut rel_y = pad_y;
        let mut submenu_anchor_rel_y = pad_y;

        for (idx, entry) in self.entries.iter().enumerate() {
            match entry {
                MenuEntry::Item(_) => {
                    rows.push(MenuRowRect {
                        entry_index: idx,
                        y: rel_y,
                        h: row_h,
                        is_separator: false,
                    });
                    rel_y += row_h;
                }
                MenuEntry::ThemeSubmenu { .. } => {
                    submenu_anchor_rel_y = rel_y;
                    rows.push(MenuRowRect {
                        entry_index: idx,
                        y: rel_y,
                        h: row_h,
                        is_separator: false,
                    });
                    rel_y += row_h;
                }
                MenuEntry::Separator => {
                    rows.push(MenuRowRect {
                        entry_index: idx,
                        y: rel_y,
                        h: sep_h,
                        is_separator: true,
                    });
                    rel_y += sep_h;
                }
            }
        }
        let h = (rel_y + pad_y).round();

        let usable_h = if (win_h as f32 - bottom_reserved) >= h + 8.0 {
            win_h as f32 - bottom_reserved
        } else {
            win_h as f32
        };

        let mut x = self.anchor_x.round();
        if x + w + 4.0 > win_w as f32 {
            x = (self.anchor_x - w).round();
        }
        x = x.clamp(4.0, (win_w as f32 - w - 4.0).max(4.0));

        let mut y = self.anchor_y.round();
        if y + h + 4.0 > usable_h {
            y = (self.anchor_y - h - ch).round();
        }
        y = y.clamp(4.0, (usable_h - h - 4.0).max(4.0));

        for r in &mut rows {
            r.y += y;
        }

        let submenu = if self.submenu_open {
            let sub_header_h = (ch + 4.0 * sf).round();
            let sub_row_h = (ch + 7.0 * sf).round();
            let sub_w = (pad_x * 2.0 + 28.0 * cw)
                .round()
                .min((win_w as f32 - 8.0).max(120.0));
            let mut sub_rows = Vec::with_capacity(self.sub_entries.len());
            let mut s_rel_y = pad_y;
            for (s_idx, se) in self.sub_entries.iter().enumerate() {
                match se {
                    ThemeSubEntry::Header(_) => {
                        sub_rows.push(MenuRowRect {
                            entry_index: s_idx,
                            y: s_rel_y,
                            h: sub_header_h,
                            is_separator: false,
                        });
                        s_rel_y += sub_header_h;
                    }
                    ThemeSubEntry::Theme(_) => {
                        sub_rows.push(MenuRowRect {
                            entry_index: s_idx,
                            y: s_rel_y,
                            h: sub_row_h,
                            is_separator: false,
                        });
                        s_rel_y += sub_row_h;
                    }
                    ThemeSubEntry::Separator => {
                        sub_rows.push(MenuRowRect {
                            entry_index: s_idx,
                            y: s_rel_y,
                            h: sep_h,
                            is_separator: true,
                        });
                        s_rel_y += sep_h;
                    }
                }
            }
            let sub_h = (s_rel_y + pad_y).round();
            let sub_usable_h = if (win_h as f32 - bottom_reserved) >= sub_h + 8.0 {
                win_h as f32 - bottom_reserved
            } else {
                win_h as f32
            };

            let mut sx = (x + w + 3.0).round();
            if sx + sub_w + 4.0 > win_w as f32 {
                sx = (x - sub_w - 3.0).round();
            }
            sx = sx.clamp(4.0, (win_w as f32 - sub_w - 4.0).max(4.0));

            let mut sy = (y + submenu_anchor_rel_y - pad_y).round();
            sy = sy.clamp(4.0, (sub_usable_h - sub_h - 4.0).max(4.0));

            for r in &mut sub_rows {
                r.y += sy;
            }

            Some(SubmenuGeometry {
                x: sx,
                y: sy,
                w: sub_w,
                h: sub_h,
                pad_x,
                rows: sub_rows,
            })
        } else {
            None
        };

        MenuGeometry {
            x,
            y,
            w,
            h,
            pad_x,
            rows,
            submenu,
        }
    }

    pub fn enabled_main_entry_at(&self, geom: &MenuGeometry, px: f32, py: f32) -> Option<usize> {
        if !geom.contains_main(px, py) {
            return None;
        }
        for row in &geom.rows {
            if !row.is_separator && py >= row.y && py < row.y + row.h {
                return match &self.entries[row.entry_index] {
                    MenuEntry::Item(it) if it.enabled => Some(row.entry_index),
                    MenuEntry::ThemeSubmenu { .. } => Some(row.entry_index),
                    _ => None,
                };
            }
        }
        None
    }

    pub fn sub_theme_at(&self, sub_geom: &SubmenuGeometry, px: f32, py: f32) -> Option<usize> {
        if !sub_geom.contains(px, py) {
            return None;
        }
        for row in &sub_geom.rows {
            if !row.is_separator && py >= row.y && py < row.y + row.h {
                if matches!(self.sub_entries[row.entry_index], ThemeSubEntry::Theme(_)) {
                    return Some(row.entry_index);
                }
                return None;
            }
        }
        None
    }

    /// Update hover state from cursor coordinates. Returns `(needs_redraw, pointer_cursor)`.
    pub fn update_hover(&mut self, geom: &MenuGeometry, px: f32, py: f32) -> (bool, bool) {
        let prev = (self.hovered, self.submenu_open, self.sub_hovered);

        if let Some(sub_geom) = &geom.submenu
            && sub_geom.contains(px, py)
        {
            self.hovered = self.theme_submenu_entry_index();
            self.sub_hovered = self.sub_theme_at(sub_geom, px, py);
            let changed = (self.hovered, self.submenu_open, self.sub_hovered) != prev;
            return (changed, self.sub_hovered.is_some());
        }

        if geom.contains_main(px, py) {
            let main_hit = self.enabled_main_entry_at(geom, px, py);
            self.hovered = main_hit;
            self.sub_hovered = None;
            if let Some(idx) = main_hit {
                self.submenu_open = matches!(self.entries[idx], MenuEntry::ThemeSubmenu { .. });
            }
            let changed = (self.hovered, self.submenu_open, self.sub_hovered) != prev;
            return (changed, main_hit.is_some());
        }

        self.sub_hovered = None;
        if !self.submenu_open {
            self.hovered = None;
        }
        let changed = (self.hovered, self.submenu_open, self.sub_hovered) != prev;
        (changed, false)
    }

    pub fn handle_click(&mut self, geom: &MenuGeometry, px: f32, py: f32) -> MenuClickOutcome {
        if let Some(sub_geom) = &geom.submenu
            && sub_geom.contains(px, py)
        {
            if let Some(sub_idx) = self.sub_theme_at(sub_geom, px, py)
                && let ThemeSubEntry::Theme(info) = &self.sub_entries[sub_idx]
            {
                return MenuClickOutcome::Execute(Action::SetTheme(info.id.to_string()));
            }
            return MenuClickOutcome::ConsumedKeepOpen;
        }

        if geom.contains_main(px, py) {
            if let Some(idx) = self.enabled_main_entry_at(geom, px, py) {
                match &self.entries[idx] {
                    MenuEntry::Item(it) if it.enabled => {
                        return MenuClickOutcome::Execute(it.action.clone());
                    }
                    MenuEntry::ThemeSubmenu { .. } => {
                        self.hovered = Some(idx);
                        self.submenu_open = !self.submenu_open;
                        self.sub_hovered = None;
                        return MenuClickOutcome::ConsumedKeepOpen;
                    }
                    _ => {}
                }
            }
            return MenuClickOutcome::ConsumedKeepOpen;
        }

        MenuClickOutcome::Outside
    }

    pub fn handle_key(
        &mut self,
        key: &Key,
        mods: ModifiersState,
        active_theme: &str,
    ) -> MenuKeyOutcome {
        // Allow Ctrl+Shift / Alt / Super global shortcuts (e.g. Ctrl+Shift+P to toggle closed) to pass through.
        if mods.alt_key() || mods.super_key() {
            return MenuKeyOutcome::NotHandled;
        }
        if mods.control_key() {
            if !mods.shift_key()
                && let Key::Character(s) = key
            {
                if s.eq_ignore_ascii_case("n") || s.eq_ignore_ascii_case("j") {
                    if self.submenu_open && self.sub_hovered.is_some() {
                        self.move_sub_hover(1);
                    } else {
                        self.move_main_hover(1);
                    }
                    return MenuKeyOutcome::Redraw;
                }
                if s.eq_ignore_ascii_case("p") || s.eq_ignore_ascii_case("k") {
                    if self.submenu_open && self.sub_hovered.is_some() {
                        self.move_sub_hover(-1);
                    } else {
                        self.move_main_hover(-1);
                    }
                    return MenuKeyOutcome::Redraw;
                }
            }
            return MenuKeyOutcome::NotHandled;
        }

        let shift = mods.shift_key();

        // When keyboard focus is inside the Switch Theme submenu:
        if self.submenu_open && self.sub_hovered.is_some() {
            return match key {
                Key::Named(NamedKey::Escape | NamedKey::ArrowLeft | NamedKey::Backspace) => {
                    self.sub_hovered = None;
                    self.submenu_open = false;
                    MenuKeyOutcome::Redraw
                }
                Key::Character(s) if s == "h" => {
                    self.sub_hovered = None;
                    self.submenu_open = false;
                    MenuKeyOutcome::Redraw
                }
                Key::Character(s) if s.eq_ignore_ascii_case("q") => MenuKeyOutcome::Close,
                Key::Named(NamedKey::ArrowDown) => {
                    self.move_sub_hover(1);
                    MenuKeyOutcome::Redraw
                }
                Key::Character(s) if s == "j" => {
                    self.move_sub_hover(1);
                    MenuKeyOutcome::Redraw
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.move_sub_hover(-1);
                    MenuKeyOutcome::Redraw
                }
                Key::Character(s) if s == "k" => {
                    self.move_sub_hover(-1);
                    MenuKeyOutcome::Redraw
                }
                Key::Named(NamedKey::Tab) => {
                    self.move_sub_hover(if shift { -1 } else { 1 });
                    MenuKeyOutcome::Redraw
                }
                Key::Named(NamedKey::Home) => {
                    if let Some(&first) = self.theme_sub_indices().first() {
                        self.sub_hovered = Some(first);
                    }
                    MenuKeyOutcome::Redraw
                }
                Key::Character(s) if s == "g" && !shift => {
                    if let Some(&first) = self.theme_sub_indices().first() {
                        self.sub_hovered = Some(first);
                    }
                    MenuKeyOutcome::Redraw
                }
                Key::Named(NamedKey::End) => {
                    if let Some(&last) = self.theme_sub_indices().last() {
                        self.sub_hovered = Some(last);
                    }
                    MenuKeyOutcome::Redraw
                }
                Key::Character(s) if s == "G" || (s == "g" && shift) => {
                    if let Some(&last) = self.theme_sub_indices().last() {
                        self.sub_hovered = Some(last);
                    }
                    MenuKeyOutcome::Redraw
                }
                Key::Character(s) if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() => {
                    let d = s.as_bytes()[0];
                    let pos = if d == b'0' {
                        9usize
                    } else {
                        (d - b'1') as usize
                    };
                    let theme_indices = self.theme_sub_indices();
                    if let Some(&sub_idx) = theme_indices.get(pos) {
                        self.sub_hovered = Some(sub_idx);
                    }
                    MenuKeyOutcome::Redraw
                }
                Key::Named(NamedKey::Enter | NamedKey::Space) => self.activate_hovered_sub_theme(),
                Key::Character(s) if s == "l" => self.activate_hovered_sub_theme(),
                _ => MenuKeyOutcome::NotHandled,
            };
        }

        match key {
            Key::Named(NamedKey::Escape) => MenuKeyOutcome::Close,
            Key::Character(s) if s.eq_ignore_ascii_case("q") => MenuKeyOutcome::Close,
            Key::Named(NamedKey::ArrowDown) => {
                self.move_main_hover(1);
                MenuKeyOutcome::Redraw
            }
            Key::Character(s) if s == "j" => {
                self.move_main_hover(1);
                MenuKeyOutcome::Redraw
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.move_main_hover(-1);
                MenuKeyOutcome::Redraw
            }
            Key::Character(s) if s == "k" => {
                self.move_main_hover(-1);
                MenuKeyOutcome::Redraw
            }
            Key::Named(NamedKey::Tab) => {
                self.move_main_hover(if shift { -1 } else { 1 });
                MenuKeyOutcome::Redraw
            }
            Key::Named(NamedKey::Home) => {
                self.jump_main_edge(true);
                MenuKeyOutcome::Redraw
            }
            Key::Character(s) if s == "g" && !shift => {
                self.jump_main_edge(true);
                MenuKeyOutcome::Redraw
            }
            Key::Named(NamedKey::End) => {
                self.jump_main_edge(false);
                MenuKeyOutcome::Redraw
            }
            Key::Character(s) if s == "G" || (s == "g" && shift) => {
                self.jump_main_edge(false);
                MenuKeyOutcome::Redraw
            }
            Key::Named(NamedKey::ArrowRight) => {
                if let Some(idx) = self.hovered
                    && matches!(self.entries.get(idx), Some(MenuEntry::ThemeSubmenu { .. }))
                {
                    self.open_submenu_with_keyboard(active_theme);
                    return MenuKeyOutcome::Redraw;
                }
                MenuKeyOutcome::NotHandled
            }
            Key::Character(s) if s == "l" => {
                if let Some(idx) = self.hovered
                    && matches!(self.entries.get(idx), Some(MenuEntry::ThemeSubmenu { .. }))
                {
                    self.open_submenu_with_keyboard(active_theme);
                    return MenuKeyOutcome::Redraw;
                }
                MenuKeyOutcome::NotHandled
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                if self.hovered.is_none() {
                    self.select_first_enabled();
                }
                let Some(idx) = self.hovered else {
                    return MenuKeyOutcome::Close;
                };
                self.activate_main_entry(idx, active_theme)
            }
            Key::Character(s) => self.handle_main_mnemonic(s, active_theme),
            _ => MenuKeyOutcome::NotHandled,
        }
    }

    fn activate_hovered_sub_theme(&self) -> MenuKeyOutcome {
        if let Some(idx) = self.sub_hovered
            && let Some(ThemeSubEntry::Theme(info)) = self.sub_entries.get(idx)
        {
            MenuKeyOutcome::Execute(Action::SetTheme(info.id.to_string()))
        } else {
            MenuKeyOutcome::Close
        }
    }

    fn activate_main_entry(&mut self, idx: usize, active_theme: &str) -> MenuKeyOutcome {
        match self.entries.get(idx) {
            Some(MenuEntry::Item(it)) if it.enabled => MenuKeyOutcome::Execute(it.action.clone()),
            Some(MenuEntry::ThemeSubmenu { .. }) => {
                self.hovered = Some(idx);
                self.open_submenu_with_keyboard(active_theme);
                MenuKeyOutcome::Redraw
            }
            _ => MenuKeyOutcome::Close,
        }
    }

    fn handle_main_mnemonic(&mut self, s: &str, active_theme: &str) -> MenuKeyOutcome {
        if s.len() == 1 {
            let b = s.as_bytes()[0];
            if (b'1'..=b'9').contains(&b) {
                let pos = (b - b'1') as usize;
                let enabled = self.enabled_main_indices();
                if let Some(&entry_idx) = enabled.get(pos) {
                    return self.activate_main_entry(entry_idx, active_theme);
                }
                return MenuKeyOutcome::Redraw;
            }
        }

        let target_action = match s {
            "t" | "T" => {
                self.open_theme_submenu(active_theme);
                return MenuKeyOutcome::Redraw;
            }
            "c" | "C" => Some(Action::CopyToClipboard),
            "v" | "V" | "p" | "P" => Some(Action::PasteFromClipboard),
            "d" | "D" => Some(Action::NewSplit(SplitDirection::Right)),
            "e" | "E" => Some(Action::NewSplit(SplitDirection::Down)),
            "=" | "+" => Some(Action::EqualizeSplits),
            "n" | "N" => Some(Action::NewTab),
            "b" | "B" => Some(Action::ToggleStatusBar),
            "i" | "I" => Some(Action::ToggleEditor),
            "\\" | "|" => Some(Action::ToggleSplitMode),
            "?" | "/" => Some(Action::ShowHelp),
            "o" | "O" => Some(Action::EditConfig),
            "u" | "U" => Some(Action::TriggerUpdate),
            "w" | "W" | "x" | "X" => Some(Action::CloseSurface),
            _ => None,
        };

        if let Some(target) = target_action {
            for entry in &self.entries {
                if let MenuEntry::Item(it) = entry
                    && it.enabled
                    && it.action == target
                {
                    return MenuKeyOutcome::Execute(it.action.clone());
                }
            }
        }

        MenuKeyOutcome::NotHandled
    }

    fn open_submenu_with_keyboard(&mut self, active_theme: &str) {
        self.committed_theme = Some(active_theme.to_string());
        self.submenu_open = true;
        let active_idx = self
            .sub_entries
            .iter()
            .position(|e| matches!(e, ThemeSubEntry::Theme(t) if t.id == active_theme))
            .or_else(|| {
                self.sub_entries
                    .iter()
                    .position(|e| matches!(e, ThemeSubEntry::Theme(_)))
            });
        self.sub_hovered = active_idx;
    }

    fn jump_main_edge(&mut self, first: bool) {
        let enabled = self.enabled_main_indices();
        let chosen = if first {
            enabled.first().copied()
        } else {
            enabled.last().copied()
        };
        self.hovered = chosen;
        self.sub_hovered = None;
        if let Some(idx) = chosen {
            self.submenu_open = matches!(self.entries[idx], MenuEntry::ThemeSubmenu { .. });
        }
    }

    fn move_main_hover(&mut self, delta: isize) {
        let enabled_indices = self.enabled_main_indices();
        if enabled_indices.is_empty() {
            self.hovered = None;
            return;
        }
        let next_pos = match self
            .hovered
            .and_then(|cur| enabled_indices.iter().position(|&i| i == cur))
        {
            Some(pos) => {
                let len = enabled_indices.len() as isize;
                ((pos as isize + delta).rem_euclid(len)) as usize
            }
            None => {
                if delta >= 0 {
                    0
                } else {
                    enabled_indices.len() - 1
                }
            }
        };
        let chosen = enabled_indices[next_pos];
        self.hovered = Some(chosen);
        self.sub_hovered = None;
        self.submenu_open = matches!(self.entries[chosen], MenuEntry::ThemeSubmenu { .. });
    }

    fn move_sub_hover(&mut self, delta: isize) {
        let theme_indices = self.theme_sub_indices();
        if theme_indices.is_empty() {
            self.sub_hovered = None;
            return;
        }
        let next_pos = match self
            .sub_hovered
            .and_then(|cur| theme_indices.iter().position(|&i| i == cur))
        {
            Some(pos) => {
                let len = theme_indices.len() as isize;
                ((pos as isize + delta).rem_euclid(len)) as usize
            }
            None => 0,
        };
        self.sub_hovered = Some(theme_indices[next_pos]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::keybind::default_keybinds;

    #[test]
    fn context_menu_has_10_themes_5_dark_5_light_and_vim_keyboard_nav() {
        let binds = default_keybinds();
        let mut menu = ContextMenu::build(790.0, 590.0, false, 2, true, &binds, None);
        menu.select_first_enabled();
        assert!(menu.hovered.is_some());

        let dark_count = menu
            .sub_entries
            .iter()
            .filter(|e| matches!(e, ThemeSubEntry::Theme(t) if t.is_dark))
            .count();
        let light_count = menu
            .sub_entries
            .iter()
            .filter(|e| matches!(e, ThemeSubEntry::Theme(t) if !t.is_dark))
            .count();
        assert_eq!(dark_count, 5);
        assert_eq!(light_count, 5);

        menu.submenu_open = true;
        let geom = menu.compute_geometry(9.0, 20.0, 960, 600, 1.0, 26.0);
        assert!(geom.x + geom.w <= 956.0);
        assert!(geom.y + geom.h <= 600.0 - 26.0);
        let sub = geom.submenu.expect("submenu geometry present");
        assert!(sub.x >= 4.0 && sub.x + sub.w <= 956.0);
        assert!(sub.y >= 4.0 && sub.y + sub.h <= 600.0 - 26.0);

        // Verify Vim `t` quick jump into Switch Theme submenu + `j` + `Enter`
        menu.submenu_open = false;
        menu.sub_hovered = None;
        let mods = ModifiersState::default();
        assert_eq!(
            menu.handle_key(&Key::Character("t".into()), mods, "ocean-dark"),
            MenuKeyOutcome::Redraw
        );
        assert!(menu.submenu_open && menu.sub_hovered.is_some());
        assert_eq!(
            menu.handle_key(&Key::Character("j".into()), mods, "ocean-dark"),
            MenuKeyOutcome::Redraw
        );
        assert_eq!(
            menu.handle_key(&Key::Named(NamedKey::Enter), mods, "ocean-dark"),
            MenuKeyOutcome::Execute(Action::SetTheme("tokyo-night".to_string()))
        );

        // Verify `h` exits submenu and `b` triggers ToggleStatusBar
        assert_eq!(
            menu.handle_key(&Key::Character("h".into()), mods, "ocean-dark"),
            MenuKeyOutcome::Redraw
        );
        assert!(!menu.submenu_open);
        assert_eq!(
            menu.handle_key(&Key::Character("b".into()), mods, "ocean-dark"),
            MenuKeyOutcome::Execute(Action::ToggleStatusBar)
        );
    }

    #[test]
    fn theme_submenu_hover_preview_and_commit_behavior() {
        let binds = default_keybinds();
        let mut menu = ContextMenu::build(100.0, 100.0, false, 1, true, &binds, None);
        menu.open_theme_submenu("ocean-dark");

        assert_eq!(menu.committed_theme.as_deref(), Some("ocean-dark"));
        assert_eq!(menu.hovered_theme_id(), Some("ocean-dark"));

        let mods = ModifiersState::default();
        // Moving down (j) hovers tokyo-night for live preview without committing yet
        let outcome = menu.handle_key(&Key::Character("j".into()), mods, "ocean-dark");
        assert_eq!(outcome, MenuKeyOutcome::Redraw);
        assert_eq!(menu.hovered_theme_id(), Some("tokyo-night"));

        // Digit shortcut '6' (first light theme) previews ocean-light
        let outcome = menu.handle_key(&Key::Character("6".into()), mods, "ocean-dark");
        assert_eq!(outcome, MenuKeyOutcome::Redraw);
        assert_eq!(menu.hovered_theme_id(), Some("ocean-light"));

        // Escape cancels submenu, clearing hovered theme ID so parent reverts preview
        let outcome = menu.handle_key(&Key::Named(NamedKey::Escape), mods, "ocean-dark");
        assert_eq!(outcome, MenuKeyOutcome::Redraw);
        assert!(!menu.submenu_open);
        assert_eq!(menu.hovered_theme_id(), None);

        // Re-open themes submenu and hit Enter on hovered theme -> commits theme
        menu.open_theme_submenu("ocean-dark");
        assert_eq!(menu.hovered_theme_id(), Some("ocean-dark"));
        let outcome = menu.handle_key(&Key::Named(NamedKey::Enter), mods, "ocean-dark");
        assert_eq!(
            outcome,
            MenuKeyOutcome::Execute(Action::SetTheme("ocean-dark".to_string()))
        );

        // Test mouse click directly on submenu row
        let geom = menu.compute_geometry(9.0, 20.0, 800, 600, 1.0, 26.0);
        let sub = geom.submenu.as_ref().expect("submenu geometry present");
        let first_theme_row = sub
            .rows
            .iter()
            .find(|r| {
                !r.is_separator
                    && matches!(menu.sub_entries[r.entry_index], ThemeSubEntry::Theme(_))
            })
            .expect("theme row present");
        let click_y = first_theme_row.y + 2.0;
        let click_x = sub.x + 10.0;
        let click_outcome = menu.handle_click(&geom, click_x, click_y);
        assert!(matches!(
            click_outcome,
            MenuClickOutcome::Execute(Action::SetTheme(_))
        ));
    }

    #[test]
    fn context_menu_includes_update_action_when_available() {
        let binds = default_keybinds();
        let update = crate::update::UpdateInfo {
            version: "1.0.0".to_string(),
            tag_name: "v1.0.0".to_string(),
            release_name: "Ocean v1.0.0".to_string(),
            html_url: "https://github.com/moekyawsoe/ocean/releases/tag/v1.0.0".to_string(),
            published_at: "2026-11-04".to_string(),
            deb_url: None,
            rpm_url: None,
            tar_url: None,
            sha256_url: None,
            body: "".to_string(),
        };

        let menu = ContextMenu::build(100.0, 100.0, false, 1, true, &binds, Some(&update));
        let update_entry = menu.entries.iter().find(|e| {
            matches!(e, MenuEntry::Item(it) if it.action == Action::TriggerUpdate && it.label.contains("v1.0.0"))
        });
        assert!(
            update_entry.is_some(),
            "Update entry should be present in menu"
        );
    }
}
