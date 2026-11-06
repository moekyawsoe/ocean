use std::path::PathBuf;
use std::time::Instant;

use crate::config::theme::ThemePalette;
use crate::config::{Config, CursorStyle, EditorKeymap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    General,
    Editor,
    Appearance,
    Sidebar,
    Terminal,
    Diff,
}

impl SettingsCategory {
    pub const ALL: [SettingsCategory; 6] = [
        SettingsCategory::General,
        SettingsCategory::Editor,
        SettingsCategory::Appearance,
        SettingsCategory::Sidebar,
        SettingsCategory::Terminal,
        SettingsCategory::Diff,
    ];

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Editor => "Editor",
            Self::Appearance => "Appearance",
            Self::Sidebar => "Sidebar",
            Self::Terminal => "Terminal",
            Self::Diff => "Diff",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsButton {
    Cancel,
    Apply,
}

#[derive(Debug, Clone)]
pub struct SettingsDraft {
    // General
    pub welcome_page: bool,
    pub recent_folder_history: bool,
    pub enable_plugins: bool,
    pub debug_mode: bool,

    // Editor
    pub font_size: f32,
    pub editor_font_scale: f32,
    pub tab_size: usize,
    pub insert_spaces: bool,
    pub line_numbers: bool,
    pub relative_line_numbers: bool,
    pub auto_close_brackets: bool,
    pub format_on_save: bool,
    pub trim_trailing_whitespace: bool,
    pub vim_mode: bool,

    // Appearance
    pub theme_name: String,
    pub background_opacity: f32,
    pub show_status_bar: bool,
    pub show_menu_bar: bool,
    pub window_decorations: bool,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,

    // Sidebar
    pub show_hidden_files: bool,
    pub single_click_open: bool,
    pub compact_folders: bool,
    pub git_changes_enabled: bool,
    pub search_enabled: bool,
    pub activity_bar_visible: bool,

    // Terminal
    pub shell: String,
    pub scrollback_limit: usize,
    pub copy_on_select: bool,
    pub mouse_hide_typing: bool,
    pub link_url: bool,

    // Diff
    pub inline_git_blame: bool,
    pub gutter_indicators: bool,
    pub show_staged_status: bool,
    pub auto_refresh_git: bool,
}

impl SettingsDraft {
    pub fn from_config(config: &Config) -> Self {
        let shell = config
            .command
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/bash".to_string());

        Self {
            welcome_page: config.restore_session,
            recent_folder_history: config.crash_recovery,
            enable_plugins: true,
            debug_mode: false,

            font_size: config.font_size,
            editor_font_scale: config.editor_font_scale,
            tab_size: 2,
            insert_spaces: true,
            line_numbers: true,
            relative_line_numbers: false,
            auto_close_brackets: true,
            format_on_save: false,
            trim_trailing_whitespace: config.clipboard_trim_trailing_spaces,
            vim_mode: matches!(config.editor_keymap, EditorKeymap::Vim),

            theme_name: config.theme_name.clone(),
            background_opacity: config.background_opacity,
            show_status_bar: config.window_show_status_bar,
            show_menu_bar: config.window_show_menu_bar,
            window_decorations: config.window_decoration,
            cursor_style: config.cursor_style,
            cursor_blink: config.cursor_blink,

            show_hidden_files: true,
            single_click_open: true,
            compact_folders: false,
            git_changes_enabled: true,
            search_enabled: true,
            activity_bar_visible: false,

            shell,
            scrollback_limit: config.scrollback_limit,
            copy_on_select: config.copy_on_select,
            mouse_hide_typing: config.mouse_hide_while_typing,
            link_url: config.link_url,

            inline_git_blame: true,
            gutter_indicators: true,
            show_staged_status: true,
            auto_refresh_git: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsState {
    pub active_category: SettingsCategory,
    pub draft: SettingsDraft,
    pub scroll_y: f32,
    pub hovered_category: Option<SettingsCategory>,
    pub hovered_item: Option<usize>,
    pub hovered_button: Option<SettingsButton>,
    pub applied_feedback: Option<Instant>,
}

impl Default for SettingsState {
    fn default() -> Self {
        let cfg = Config::default();
        Self::new(&cfg)
    }
}

impl SettingsState {
    pub fn new(config: &Config) -> Self {
        Self {
            active_category: SettingsCategory::General,
            draft: SettingsDraft::from_config(config),
            scroll_y: 0.0,
            hovered_category: None,
            hovered_item: None,
            hovered_button: None,
            applied_feedback: None,
        }
    }

    pub fn reload_from_config(&mut self, config: &Config) {
        self.draft = SettingsDraft::from_config(config);
    }

    pub fn toggle_option(&mut self, category: SettingsCategory, item_idx: usize) {
        match category {
            SettingsCategory::General => match item_idx {
                0 => self.draft.welcome_page = !self.draft.welcome_page,
                1 => self.draft.recent_folder_history = !self.draft.recent_folder_history,
                2 => self.draft.enable_plugins = !self.draft.enable_plugins,
                3 => self.draft.debug_mode = !self.draft.debug_mode,
                _ => {}
            },
            SettingsCategory::Editor => match item_idx {
                0 => self.cycle_tab_size(),
                1 => self.draft.insert_spaces = !self.draft.insert_spaces,
                2 => self.draft.line_numbers = !self.draft.line_numbers,
                3 => self.draft.relative_line_numbers = !self.draft.relative_line_numbers,
                4 => self.draft.auto_close_brackets = !self.draft.auto_close_brackets,
                5 => self.draft.format_on_save = !self.draft.format_on_save,
                6 => self.draft.trim_trailing_whitespace = !self.draft.trim_trailing_whitespace,
                7 => self.draft.vim_mode = !self.draft.vim_mode,
                _ => {}
            },
            SettingsCategory::Appearance => match item_idx {
                0 => self.cycle_theme(),
                1 => self.cycle_background_opacity(),
                2 => self.draft.show_status_bar = !self.draft.show_status_bar,
                3 => self.draft.show_menu_bar = !self.draft.show_menu_bar,
                4 => self.draft.window_decorations = !self.draft.window_decorations,
                5 => self.cycle_cursor_style(),
                6 => self.draft.cursor_blink = !self.draft.cursor_blink,
                _ => {}
            },
            SettingsCategory::Sidebar => match item_idx {
                0 => self.draft.show_hidden_files = !self.draft.show_hidden_files,
                1 => self.draft.single_click_open = !self.draft.single_click_open,
                2 => self.draft.compact_folders = !self.draft.compact_folders,
                3 => self.draft.git_changes_enabled = !self.draft.git_changes_enabled,
                4 => self.draft.search_enabled = !self.draft.search_enabled,
                5 => self.draft.activity_bar_visible = !self.draft.activity_bar_visible,
                _ => {}
            },
            SettingsCategory::Terminal => match item_idx {
                0 => self.draft.copy_on_select = !self.draft.copy_on_select,
                1 => self.draft.mouse_hide_typing = !self.draft.mouse_hide_typing,
                2 => self.draft.link_url = !self.draft.link_url,
                _ => {}
            },
            SettingsCategory::Diff => match item_idx {
                0 => self.draft.inline_git_blame = !self.draft.inline_git_blame,
                1 => self.draft.gutter_indicators = !self.draft.gutter_indicators,
                2 => self.draft.show_staged_status = !self.draft.show_staged_status,
                3 => self.draft.auto_refresh_git = !self.draft.auto_refresh_git,
                _ => {}
            },
        }
    }

    pub fn cycle_theme(&mut self) {
        let themes = [
            "ocean-dark",
            "tokyo-night",
            "catppuccin-mocha",
            "gruvbox-dark",
            "nord",
            "ocean-light",
            "tokyo-night-light",
            "catppuccin-latte",
        ];
        if let Some(pos) = themes.iter().position(|t| *t == self.draft.theme_name) {
            let next = (pos + 1) % themes.len();
            self.draft.theme_name = themes[next].to_string();
        } else {
            self.draft.theme_name = "ocean-dark".to_string();
        }
    }

    pub fn cycle_cursor_style(&mut self) {
        self.draft.cursor_style = match self.draft.cursor_style {
            CursorStyle::Block => CursorStyle::Bar,
            CursorStyle::Bar => CursorStyle::Underline,
            CursorStyle::Underline => CursorStyle::BlockHollow,
            CursorStyle::BlockHollow => CursorStyle::Block,
        };
    }

    pub fn cycle_tab_size(&mut self) {
        self.draft.tab_size = match self.draft.tab_size {
            2 => 4,
            4 => 8,
            _ => 2,
        };
    }

    pub fn cycle_background_opacity(&mut self) {
        if self.draft.background_opacity >= 0.99 {
            self.draft.background_opacity = 0.90;
        } else if self.draft.background_opacity >= 0.89 {
            self.draft.background_opacity = 0.80;
        } else {
            self.draft.background_opacity = 1.0;
        }
    }

    pub fn apply(&mut self, config: &mut Config) -> Result<PathBuf, String> {
        let updates: Vec<(&str, String)> = vec![
            ("theme", self.draft.theme_name.clone()),
            ("font-size", format!("{:.1}", self.draft.font_size)),
            (
                "editor-font-scale",
                format!("{:.2}", self.draft.editor_font_scale),
            ),
            (
                "background-opacity",
                format!("{:.2}", self.draft.background_opacity),
            ),
            (
                "cursor-style",
                match self.draft.cursor_style {
                    CursorStyle::Block => "block".into(),
                    CursorStyle::Bar => "bar".into(),
                    CursorStyle::Underline => "underline".into(),
                    CursorStyle::BlockHollow => "block_hollow".into(),
                },
            ),
            (
                "cursor-style-blink",
                if self.draft.cursor_blink {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            (
                "window-show-status-bar",
                if self.draft.show_status_bar {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            (
                "window-show-menu-bar",
                if self.draft.show_menu_bar {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            (
                "window-decoration",
                if self.draft.window_decorations {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            ("scrollback-limit", self.draft.scrollback_limit.to_string()),
            (
                "copy-on-select",
                if self.draft.copy_on_select {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            (
                "mouse-hide-while-typing",
                if self.draft.mouse_hide_typing {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            (
                "link-url",
                if self.draft.link_url {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            (
                "editor-keymap",
                if self.draft.vim_mode {
                    "vim".into()
                } else {
                    "modeless".into()
                },
            ),
            (
                "restore-session",
                if self.draft.welcome_page {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
            (
                "crash-recovery",
                if self.draft.recent_folder_history {
                    "true".into()
                } else {
                    "false".into()
                },
            ),
        ];

        let path = config.save_setting_key_values(&updates)?;

        config.theme_name = self.draft.theme_name.clone();
        if let Some(pal) = ThemePalette::from_name(&self.draft.theme_name) {
            config.colors = pal;
        }
        config.font_size = self.draft.font_size;
        config.editor_font_scale = self.draft.editor_font_scale;
        config.background_opacity = self.draft.background_opacity;
        config.cursor_style = self.draft.cursor_style;
        config.cursor_blink = self.draft.cursor_blink;
        config.window_show_status_bar = self.draft.show_status_bar;
        config.window_show_menu_bar = self.draft.show_menu_bar;
        config.window_decoration = self.draft.window_decorations;
        config.scrollback_limit = self.draft.scrollback_limit;
        config.copy_on_select = self.draft.copy_on_select;
        config.mouse_hide_while_typing = self.draft.mouse_hide_typing;
        config.link_url = self.draft.link_url;
        config.editor_keymap = if self.draft.vim_mode {
            EditorKeymap::Vim
        } else {
            EditorKeymap::Modeless
        };
        config.restore_session = self.draft.welcome_page;
        config.crash_recovery = self.draft.recent_folder_history;

        self.applied_feedback = Some(Instant::now());
        Ok(path)
    }
}
