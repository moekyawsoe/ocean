use std::path::PathBuf;

use winit::keyboard::{Key, NamedKey};

#[derive(Debug, Clone)]
pub enum MenuAction {
    NewFile,
    OpenFile,
    OpenEditorWindow,
    SaveFile,
    CloseFile,
    Quit,
    Undo,
    Redo,
    Find,
    CommandPalette,
    ToggleExplorer,
    ToggleProblems,
    GitChanges,
    ShowGitDiff,
    ToggleStatusBar,
    ToggleMenuBar,
    ToggleSplit,
    PureEditorMode,
    GoToFile,
    ShowHelp,
    OpenConfig,
    RecentFile(PathBuf),
}

#[derive(Debug, Clone)]
pub struct MenuEntry {
    pub label: String,
    pub shortcut: String,
    pub action: Option<MenuAction>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub title: &'static str,
    pub entries: Vec<MenuEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct MenuBar {
    pub menus: Vec<Menu>,
    pub open: Option<usize>,
    pub hovered_title: Option<usize>,
    pub selected: usize,
    pub hovered_item: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct MenuBarRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl MenuBarRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Debug, Clone)]
pub struct MenuBarLayout {
    pub bar: MenuBarRect,
    pub titles: Vec<(usize, MenuBarRect)>,
    pub dropdown: Option<MenuBarRect>,
    pub rows: Vec<(usize, MenuBarRect)>,
}

pub enum MenuKeyOutcome {
    Handled,
    Execute(MenuAction),
}

pub enum MenuMouseOutcome {
    Handled,
    Close,
    Execute(MenuAction),
}

fn entry(label: &str, shortcut: &str, action: MenuAction, enabled: bool) -> MenuEntry {
    MenuEntry {
        label: label.to_string(),
        shortcut: shortcut.to_string(),
        action: Some(action),
        enabled,
    }
}

fn header(label: &str) -> MenuEntry {
    MenuEntry {
        label: label.to_string(),
        shortcut: String::new(),
        action: None,
        enabled: false,
    }
}

impl MenuBar {
    pub fn build(recent: &[PathBuf]) -> Self {
        let mut file_entries = vec![
            entry("New File", "", MenuAction::NewFile, true),
            entry("Open File...", "Ctrl+P", MenuAction::OpenFile, true),
            entry(
                "Open Editor in New Window",
                "Ctrl+Shift+O",
                MenuAction::OpenEditorWindow,
                true,
            ),
            entry("Save", "Ctrl+S", MenuAction::SaveFile, true),
            entry("Close File", "Ctrl+W", MenuAction::CloseFile, true),
        ];
        if !recent.is_empty() {
            file_entries.push(header("Recent Files"));
            for path in recent.iter().take(8) {
                let label = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| path.display().to_string());
                file_entries.push(MenuEntry {
                    label,
                    shortcut: String::new(),
                    action: Some(MenuAction::RecentFile(path.clone())),
                    enabled: true,
                });
            }
        }
        file_entries.push(header(""));
        file_entries.push(entry("Quit", "Ctrl+Shift+Q", MenuAction::Quit, true));

        let menus = vec![
            Menu {
                title: "File",
                entries: file_entries,
            },
            Menu {
                title: "Edit",
                entries: vec![
                    entry("Undo", "Ctrl+Z", MenuAction::Undo, true),
                    entry("Redo", "Ctrl+Y", MenuAction::Redo, true),
                    header(""),
                    entry("Find in File", "Ctrl+F", MenuAction::Find, true),
                    entry(
                        "Command Palette",
                        "Ctrl+P",
                        MenuAction::CommandPalette,
                        true,
                    ),
                ],
            },
            Menu {
                title: "View",
                entries: vec![
                    entry("Explorer", "Ctrl+B", MenuAction::ToggleExplorer, true),
                    entry("Problems", "", MenuAction::ToggleProblems, true),
                    entry("Git Changes", "Ctrl+Shift+G", MenuAction::GitChanges, true),
                    entry("Show Git Diff", "", MenuAction::ShowGitDiff, true),
                    entry(
                        "Status Bar",
                        "Ctrl+Shift+B",
                        MenuAction::ToggleStatusBar,
                        true,
                    ),
                    entry("Menu Bar", "F10", MenuAction::ToggleMenuBar, true),
                    header(""),
                    entry(
                        "Split Editor/Terminal",
                        "Ctrl+\\",
                        MenuAction::ToggleSplit,
                        true,
                    ),
                    entry(
                        "Pure Editor",
                        "Ctrl+Shift+I",
                        MenuAction::PureEditorMode,
                        true,
                    ),
                ],
            },
            Menu {
                title: "Go",
                entries: vec![
                    entry("Go to File...", "Ctrl+P", MenuAction::GoToFile, true),
                    entry("Go to Line...", "Ctrl+G", MenuAction::CommandPalette, false),
                ],
            },
            Menu {
                title: "Help",
                entries: vec![
                    entry(
                        "Keyboard Shortcuts",
                        "Ctrl+Shift+?",
                        MenuAction::ShowHelp,
                        true,
                    ),
                    entry("Edit Config", "", MenuAction::OpenConfig, true),
                ],
            },
        ];

        Self {
            menus,
            open: None,
            hovered_title: None,
            selected: 0,
            hovered_item: None,
        }
    }

    pub fn height(cell_h: f32, scale_factor: f32) -> f32 {
        (cell_h + 8.0 * scale_factor).round()
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn open_menu(&mut self, index: usize) {
        self.open = Some(index.min(self.menus.len().saturating_sub(1)));
        self.selected = self.first_enabled().unwrap_or(0);
        self.hovered_item = None;
    }

    pub fn close(&mut self) {
        self.open = None;
        self.hovered_item = None;
    }

    fn first_enabled(&self) -> Option<usize> {
        let index = self.open?;
        self.menus
            .get(index)?
            .entries
            .iter()
            .position(|entry| entry.enabled)
    }

    fn move_selection(&mut self, delta: isize) {
        let Some(index) = self.open else {
            return;
        };
        let Some(menu) = self.menus.get(index) else {
            return;
        };
        let len = menu.entries.len();
        if len == 0 {
            return;
        }
        let mut next = self.selected as isize;
        for _ in 0..len {
            next = (next + delta).rem_euclid(len as isize);
            if menu.entries[next as usize].enabled {
                self.selected = next as usize;
                return;
            }
        }
    }

    pub fn handle_key(&mut self, key: &Key) -> MenuKeyOutcome {
        let Some(index) = self.open else {
            return MenuKeyOutcome::Handled;
        };

        match key {
            Key::Named(NamedKey::Escape) => {
                self.close();
            }
            Key::Named(NamedKey::ArrowLeft) => {
                let next = (index + self.menus.len() - 1) % self.menus.len();
                self.open_menu(next);
            }
            Key::Named(NamedKey::ArrowRight) => {
                let next = (index + 1) % self.menus.len();
                self.open_menu(next);
            }
            Key::Named(NamedKey::ArrowUp) => self.move_selection(-1),
            Key::Named(NamedKey::ArrowDown) => self.move_selection(1),
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if let Some(menu) = self.menus.get(index)
                    && let Some(entry) = menu.entries.get(self.selected)
                    && entry.enabled
                    && let Some(action) = &entry.action
                {
                    let action = action.clone();
                    self.close();
                    return MenuKeyOutcome::Execute(action);
                }
            }
            Key::Character(text) => {
                let ch = text.chars().next().unwrap_or('\0').to_ascii_lowercase();
                if let Some(menu) = self.menus.get(index)
                    && let Some((pos, entry)) =
                        menu.entries.iter().enumerate().find(|(_, entry)| {
                            entry.enabled
                                && entry
                                    .label
                                    .chars()
                                    .next()
                                    .map(|c| c.to_ascii_lowercase() == ch)
                                    .unwrap_or(false)
                        })
                    && let Some(action) = &entry.action
                {
                    let action = action.clone();
                    self.selected = pos;
                    self.close();
                    return MenuKeyOutcome::Execute(action);
                }
            }
            _ => {}
        }
        MenuKeyOutcome::Handled
    }

    pub fn layout(
        &self,
        x: f32,
        y: f32,
        width: f32,
        cell_w: f32,
        cell_h: f32,
        scale_factor: f32,
    ) -> MenuBarLayout {
        let sf = scale_factor.max(0.5);
        let pad = (12.0 * sf).round();
        let bar_h = Self::height(cell_h, sf);
        let mut titles = Vec::new();
        let mut title_x = x + (6.0 * sf).round();
        for (index, menu) in self.menus.iter().enumerate() {
            let w = (menu.title.chars().count() as f32 * cell_w + pad * 2.0).round();
            titles.push((
                index,
                MenuBarRect {
                    x: title_x,
                    y,
                    w,
                    h: bar_h,
                },
            ));
            title_x += w;
        }

        let mut dropdown = None;
        let mut rows = Vec::new();
        if let Some(index) = self.open
            && let Some(menu) = self.menus.get(index)
            && let Some((_, title_rect)) = titles.get(index)
        {
            let max_label = menu
                .entries
                .iter()
                .map(|entry| entry.label.chars().count())
                .max()
                .unwrap_or(8);
            let max_shortcut = menu
                .entries
                .iter()
                .map(|entry| entry.shortcut.chars().count())
                .max()
                .unwrap_or(0);
            let panel_w = ((max_label + max_shortcut + 6) as f32 * cell_w + pad * 2.0)
                .max(200.0 * sf)
                .min((width - (title_rect.x - x) - 8.0).max(160.0));
            let row_h = (cell_h + 6.0 * sf).round();
            let panel_h = (menu.entries.len() as f32 * row_h + 8.0 * sf).round();
            let panel = MenuBarRect {
                x: title_rect.x,
                y: y + bar_h,
                w: panel_w,
                h: panel_h,
            };
            dropdown = Some(panel);
            for row in 0..menu.entries.len() {
                rows.push((
                    row,
                    MenuBarRect {
                        x: panel.x + 4.0 * sf,
                        y: panel.y + 4.0 * sf + row as f32 * row_h,
                        w: panel.w - 8.0 * sf,
                        h: row_h,
                    },
                ));
            }
        }

        MenuBarLayout {
            bar: MenuBarRect {
                x,
                y,
                w: width,
                h: bar_h,
            },
            titles,
            dropdown,
            rows,
        }
    }

    pub fn handle_mouse(
        &mut self,
        layout: &MenuBarLayout,
        px: f32,
        py: f32,
        pressed: bool,
    ) -> MenuMouseOutcome {
        for (index, rect) in &layout.titles {
            if rect.contains(px, py) {
                self.hovered_title = Some(*index);
                if pressed {
                    if self.open == Some(*index) {
                        self.close();
                    } else {
                        self.open_menu(*index);
                    }
                } else if self.open.is_some() && self.open != Some(*index) {
                    self.open_menu(*index);
                }
                return MenuMouseOutcome::Handled;
            }
        }
        self.hovered_title = None;

        if let Some(panel) = layout.dropdown
            && panel.contains(px, py)
        {
            for (row, rect) in &layout.rows {
                if rect.contains(px, py) {
                    self.hovered_item = Some(*row);
                    if pressed
                        && let Some(menu) = self.open.and_then(|index| self.menus.get(index))
                        && let Some(entry) = menu.entries.get(*row)
                        && entry.enabled
                        && let Some(action) = &entry.action
                    {
                        let action = action.clone();
                        self.close();
                        return MenuMouseOutcome::Execute(action);
                    }
                    return MenuMouseOutcome::Handled;
                }
            }
            self.hovered_item = None;
            return MenuMouseOutcome::Handled;
        }

        if self.open.is_some() && pressed {
            self.close();
            return MenuMouseOutcome::Close;
        }
        MenuMouseOutcome::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_recent_file_entries() {
        let recent = vec![PathBuf::from("/tmp/a.rs"), PathBuf::from("/tmp/b.ts")];
        let bar = MenuBar::build(&recent);
        let file = &bar.menus[0];
        assert_eq!(file.title, "File");
        assert!(file.entries.iter().any(|entry| entry.label == "a.rs"));
        assert!(file.entries.iter().any(|entry| entry.label == "b.ts"));
    }

    #[test]
    fn opens_and_navigates_menus() {
        let mut bar = MenuBar::build(&[]);
        bar.open_menu(0);
        assert_eq!(bar.open, Some(0));
        bar.handle_key(&Key::Named(NamedKey::ArrowRight));
        assert_eq!(bar.open, Some(1));
        bar.handle_key(&Key::Named(NamedKey::Escape));
        assert_eq!(bar.open, None);
    }

    #[test]
    fn layout_contains_titles_and_rows() {
        let mut bar = MenuBar::build(&[]);
        bar.open_menu(0);
        let layout = bar.layout(0.0, 0.0, 1000.0, 8.0, 18.0, 1.0);
        assert_eq!(layout.titles.len(), bar.menus.len());
        assert!(layout.dropdown.is_some());
        assert!(!layout.rows.is_empty());
    }
}
