pub mod keybind;
pub mod theme;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use keybind::{Keybind, apply_keybind_list, default_keybinds, parse_keybind};
use theme::{Rgb, ThemePalette, dirs_config_dir};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricAdjust {
    Px(i32),
    Percent(f32),
}

impl Default for MetricAdjust {
    fn default() -> Self {
        Self::Px(0)
    }
}

impl MetricAdjust {
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim();
        if let Some(pct) = s.strip_suffix('%') {
            Some(Self::Percent(pct.trim().parse::<f32>().ok()? / 100.0))
        } else {
            let px = s.strip_suffix("px").unwrap_or(s).trim();
            Some(Self::Px(px.parse::<i32>().ok()?))
        }
    }

    pub fn apply(self, base: f32) -> f32 {
        match self {
            Self::Px(px) => (base + px as f32).max(2.0),
            Self::Percent(p) => (base * (1.0 + p)).max(2.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
    BlockHollow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBarMode {
    Auto,
    Always,
    Never,
}

pub const STARTER_CONFIG: &str = include_str!("../../config.example");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendMode {
    #[default]
    Auto,
    Wayland,
    X11,
}

impl std::str::FromStr for BackendMode {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "x11" | "xwayland" | "x" => Ok(Self::X11),
            "wayland" | "wayland-native" | "wl" => Ok(Self::Wayland),
            "auto" | "default" => Ok(Self::Auto),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for BackendMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Wayland => write!(f, "wayland"),
            Self::X11 => write!(f, "x11"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZawgyiMode {
    Off,
    Auto,
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKeymap {
    Modeless,
    Vim,
}

impl EditorKeymap {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Modeless => "modeless",
            Self::Vim => "vim",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    // Font configuration
    pub font_families: Vec<String>,
    pub font_family_bold: Option<String>,
    pub font_family_italic: Option<String>,
    pub font_family_bold_italic: Option<String>,
    pub font_family_myanmar: Option<String>,
    pub font_size: f32,
    pub font_thicken: bool,
    pub font_features: Vec<String>,
    pub myanmar_font_scale: f32,
    pub myanmar_zawgyi: ZawgyiMode,
    pub adjust_cell_width: MetricAdjust,
    pub adjust_cell_height: MetricAdjust,
    pub adjust_cursor_thickness: u32,

    // Colors & theme
    pub theme_name: String,
    pub colors: ThemePalette,
    pub background_opacity: f32,
    pub cursor_opacity: f32,
    pub bold_is_bright: bool,
    pub minimum_contrast: f32,

    // Cursor
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,

    // Window & UI
    pub title: Option<String>,
    pub window_padding_x: (u32, u32),
    pub window_padding_y: (u32, u32),
    pub window_padding_balance: bool,
    pub window_decoration: bool,
    pub window_titlebar: bool,
    pub window_width: u16,
    pub window_height: u16,
    pub window_vsync: bool,
    pub window_show_tab_bar: TabBarMode,
    pub window_show_status_bar: bool,
    pub editor_keymap: EditorKeymap,
    pub backend: BackendMode,

    // Terminal / Shell / Mouse / Clipboard
    pub command: Option<String>,
    pub working_directory: Option<String>,
    pub term: String,
    pub env: Vec<(String, String)>,
    pub scrollback_limit: usize,
    pub mouse_hide_while_typing: bool,
    pub mouse_scroll_multiplier: f32,
    pub copy_on_select: bool,
    pub clipboard_trim_trailing_spaces: bool,
    pub link_url: bool,
    pub auto_update_check: crate::update::UpdateCheckMode,

    // Keybinds
    pub keybinds: Vec<Keybind>,

    // Diagnostics & reload tracking
    pub loaded_path: Option<PathBuf>,
    pub last_modified: Option<SystemTime>,
    pub warnings: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let colors = ThemePalette::from_name("ocean-dark").expect("builtin ocean-dark theme");
        Self {
            font_families: Vec::new(),
            font_family_bold: None,
            font_family_italic: None,
            font_family_bold_italic: None,
            font_family_myanmar: None,
            font_size: 13.5,
            font_thicken: false,
            font_features: Vec::new(),
            myanmar_font_scale: 0.96,
            myanmar_zawgyi: ZawgyiMode::Off,
            adjust_cell_width: MetricAdjust::Px(0),
            adjust_cell_height: MetricAdjust::Px(0),
            adjust_cursor_thickness: 2,

            theme_name: "ocean-dark".to_string(),
            colors,
            background_opacity: 1.0,
            cursor_opacity: 1.0,
            bold_is_bright: false,
            minimum_contrast: 1.0,

            cursor_style: CursorStyle::Block,
            cursor_blink: false,

            title: None,
            window_padding_x: (10, 10),
            window_padding_y: (8, 8),
            window_padding_balance: true,
            window_decoration: true,
            window_titlebar: true,
            window_width: 110,
            window_height: 32,
            window_vsync: true,
            window_show_tab_bar: TabBarMode::Auto,
            window_show_status_bar: true,
            editor_keymap: EditorKeymap::Modeless,
            backend: BackendMode::Auto,

            command: None,
            working_directory: None,
            term: "xterm-256color".to_string(),
            env: Vec::new(),
            scrollback_limit: 10_000,
            mouse_hide_while_typing: true,
            mouse_scroll_multiplier: 3.0,
            copy_on_select: false,
            clipboard_trim_trailing_spaces: true,
            link_url: true,
            auto_update_check: crate::update::UpdateCheckMode::Daily,

            keybinds: default_keybinds(),
            loaded_path: None,
            last_modified: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Default)]
struct ColorOverrides {
    background: Option<Rgb>,
    foreground: Option<Rgb>,
    cursor_color: Option<Rgb>,
    cursor_text: Option<Rgb>,
    selection_background: Option<Rgb>,
    selection_foreground: Option<Rgb>,
    palette: Vec<(usize, Rgb)>,
}

impl Config {
    pub fn effective_decorations(&self) -> bool {
        self.window_decoration && self.window_titlebar
    }

    pub fn default_config_path() -> Option<PathBuf> {
        dirs_config_dir().map(|d| d.join("ocean/config"))
    }

    pub fn load(explicit_path: Option<&Path>) -> Self {
        let mut cfg = Self::default();
        let target_path = explicit_path
            .map(Path::to_path_buf)
            .or_else(Self::default_config_path);

        if let Some(path) = target_path {
            if path.exists() {
                let mut overrides = ColorOverrides::default();
                cfg.load_file_recursive(&path, &mut overrides, 0, explicit_path.is_none());
                cfg.apply_color_overrides(overrides);
                cfg.loaded_path = Some(path.clone());
                cfg.last_modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
            } else if explicit_path.is_some() {
                cfg.warnings
                    .push(format!("Config file not found: {}", path.display()));
            }
        }
        cfg
    }

    #[cfg(test)]
    pub fn load_from_str(content: &str) -> Self {
        let mut cfg = Self::default();
        let mut overrides = ColorOverrides::default();
        cfg.parse_str(content, None, &mut overrides, 0);
        cfg.apply_color_overrides(overrides);
        cfg
    }

    pub fn check_modified(&self) -> bool {
        let Some(path) = &self.loaded_path else {
            return false;
        };
        let Ok(meta) = fs::metadata(path) else {
            return false;
        };
        let Ok(mtime) = meta.modified() else {
            return false;
        };
        self.last_modified != Some(mtime)
    }

    /// Ensure the configuration file exists on disk (creates ~/.config/ocean/config from template if absent).
    pub fn ensure_config_file(&mut self) -> Result<PathBuf, String> {
        let target_path = self
            .loaded_path
            .clone()
            .or_else(Self::default_config_path)
            .ok_or_else(|| "Could not determine config path".to_string())?;

        if !target_path.exists() {
            if let Some(parent) = target_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(&target_path, STARTER_CONFIG).map_err(|e| {
                format!(
                    "Failed to create config file at {}: {e}",
                    target_path.display()
                )
            })?;
            self.loaded_path = Some(target_path.clone());
            self.last_modified = fs::metadata(&target_path).and_then(|m| m.modified()).ok();
        }

        Ok(target_path)
    }

    /// Update or append `theme = <theme_name>` in the active configuration file and persist it to disk.
    /// Preserves all existing comments, options, and formatting.
    pub fn save_theme(&mut self, theme_name: &str) -> Result<PathBuf, String> {
        let target_path = self
            .loaded_path
            .clone()
            .or_else(Self::default_config_path)
            .ok_or_else(|| "Could not determine config path".to_string())?;

        let mut lines = Vec::new();
        let mut theme_updated = false;

        if target_path.exists() {
            let content = fs::read_to_string(&target_path)
                .map_err(|e| format!("Failed to read {}: {e}", target_path.display()))?;

            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.starts_with('#') {
                    if let Some((k, _)) = trimmed.split_once('=') {
                        if k.trim() == "theme" {
                            lines.push(format!("theme = {theme_name}"));
                            theme_updated = true;
                            continue;
                        }
                    }
                }
                lines.push(line.to_string());
            }
        }

        if !theme_updated {
            lines.push(format!("theme = {theme_name}"));
        }

        if let Some(parent) = target_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let mut new_content = lines.join("\n");
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }

        fs::write(&target_path, new_content)
            .map_err(|e| format!("Failed to write {}: {e}", target_path.display()))?;

        self.theme_name = theme_name.to_string();
        if let Some(pal) = ThemePalette::from_name(theme_name) {
            self.colors = pal;
        }
        self.loaded_path = Some(target_path.clone());
        self.last_modified = fs::metadata(&target_path).and_then(|m| m.modified()).ok();

        Ok(target_path)
    }

    fn apply_color_overrides(&mut self, ov: ColorOverrides) {
        if let Some(c) = ov.background {
            self.colors.background = c;
        }
        if let Some(c) = ov.foreground {
            self.colors.foreground = c;
        }
        if let Some(c) = ov.cursor_color {
            self.colors.cursor_color = c;
        }
        if let Some(c) = ov.cursor_text {
            self.colors.cursor_text = c;
        }
        if let Some(c) = ov.selection_background {
            self.colors.selection_background = c;
        }
        if let Some(c) = ov.selection_foreground {
            self.colors.selection_foreground = Some(c);
        }
        for (idx, col) in ov.palette {
            if idx < 256 {
                self.colors.ansi[idx] = col;
            }
        }
    }

    fn load_file_recursive(
        &mut self,
        path: &Path,
        overrides: &mut ColorOverrides,
        depth: usize,
        optional: bool,
    ) {
        if depth > 8 {
            self.warnings
                .push("Maximum `config-file` include depth (8) exceeded".to_string());
            return;
        }
        match fs::read_to_string(path) {
            Ok(content) => {
                self.parse_str(&content, path.parent(), overrides, depth);
            }
            Err(err) => {
                if !optional {
                    self.warnings
                        .push(format!("Failed reading {}: {}", path.display(), err));
                }
            }
        }
    }

    fn parse_str(
        &mut self,
        content: &str,
        base_dir: Option<&Path>,
        overrides: &mut ColorOverrides,
        depth: usize,
    ) {
        for (line_no, raw_line) in content.lines().enumerate() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((raw_key, raw_val)) = trimmed.split_once('=') else {
                self.warnings.push(format!(
                    "Line {}: expected `key = value`, got `{}`",
                    line_no + 1,
                    trimmed
                ));
                continue;
            };
            let key = raw_key.trim();
            let val = unquote(raw_val.trim());
            self.apply_kv(key, val, base_dir, overrides, depth, line_no + 1);
        }
    }

    pub fn apply_cli_kv(&mut self, key: &str, val: &str) {
        let mut overrides = ColorOverrides::default();
        self.apply_kv(key, val, None, &mut overrides, 0, 0);
        self.apply_color_overrides(overrides);
    }

    fn apply_kv(
        &mut self,
        key: &str,
        val: &str,
        base_dir: Option<&Path>,
        overrides: &mut ColorOverrides,
        depth: usize,
        line_no: usize,
    ) {
        match key {
            "config-file" => {
                let (optional, rel) = match val.strip_prefix('?') {
                    Some(rest) => (true, rest.trim()),
                    None => (false, val),
                };
                if rel.is_empty() {
                    return;
                }
                let expanded = expand_tilde(rel);
                let resolved = if expanded.is_relative() {
                    if let Some(dir) = base_dir {
                        dir.join(&expanded)
                    } else if let Some(cfg_dir) = dirs_config_dir() {
                        cfg_dir.join("ocean").join(&expanded)
                    } else {
                        expanded
                    }
                } else {
                    expanded
                };
                self.load_file_recursive(&resolved, overrides, depth + 1, optional);
            }
            "font-family" => {
                if val.is_empty() {
                    self.font_families.clear();
                } else {
                    self.font_families.push(val.to_string());
                }
            }
            "font-family-bold" => {
                self.font_family_bold = non_empty(val);
            }
            "font-family-italic" => {
                self.font_family_italic = non_empty(val);
            }
            "font-family-bold-italic" => {
                self.font_family_bold_italic = non_empty(val);
            }
            "font-family-myanmar" | "myanmar-font-family" => {
                self.font_family_myanmar = non_empty(val);
            }
            "font-size" => {
                if let Ok(sz) = val.parse::<f32>()
                    && (4.0..=120.0).contains(&sz)
                {
                    self.font_size = sz;
                } else {
                    self.warn(line_no, key, val);
                }
            }
            "font-thicken" => match parse_bool(val) {
                Some(b) => self.font_thicken = b,
                None => self.warn(line_no, key, val),
            },
            "font-feature" => {
                if val.is_empty() {
                    self.font_features.clear();
                } else {
                    for part in val.split(',') {
                        let ft = part.trim();
                        if !ft.is_empty() {
                            self.font_features.push(ft.to_string());
                        }
                    }
                }
            }
            "myanmar-font-scale" => {
                if let Ok(sc) = val.parse::<f32>()
                    && (0.5..=2.0).contains(&sc)
                {
                    self.myanmar_font_scale = sc;
                } else {
                    self.warn(line_no, key, val);
                }
            }
            "myanmar-zawgyi" | "myanmar-zawgyi-convert" => {
                match val.to_ascii_lowercase().as_str() {
                    "false" | "off" | "none" | "unicode" | "0" => {
                        self.myanmar_zawgyi = ZawgyiMode::Off;
                    }
                    "auto" | "detect" => {
                        self.myanmar_zawgyi = ZawgyiMode::Auto;
                    }
                    "true" | "on" | "force" | "1" => {
                        self.myanmar_zawgyi = ZawgyiMode::Force;
                    }
                    _ => self.warn(line_no, key, val),
                }
            }
            "adjust-cell-width" => match MetricAdjust::parse(val) {
                Some(m) => self.adjust_cell_width = m,
                None => self.warn(line_no, key, val),
            },
            "adjust-cell-height" => match MetricAdjust::parse(val) {
                Some(m) => self.adjust_cell_height = m,
                None => self.warn(line_no, key, val),
            },
            "adjust-cursor-thickness" => match val.parse::<u32>() {
                Ok(n) if (1..=32).contains(&n) => self.adjust_cursor_thickness = n,
                _ => self.warn(line_no, key, val),
            },
            "theme" => {
                // Support Ghostty `light:foo,dark:bar` syntax (prefer dark theme by default)
                let chosen = resolve_theme_spec(val);
                if let Some(pal) = ThemePalette::from_name(chosen) {
                    self.theme_name = pal.name.clone();
                    self.colors = pal;
                } else {
                    self.warnings
                        .push(format!("Line {}: unknown theme `{}`", line_no, val));
                }
            }
            "background" => match Rgb::parse(val) {
                Some(c) => overrides.background = Some(c),
                None => self.warn(line_no, key, val),
            },
            "foreground" => match Rgb::parse(val) {
                Some(c) => overrides.foreground = Some(c),
                None => self.warn(line_no, key, val),
            },
            "cursor-color" => match Rgb::parse(val) {
                Some(c) => overrides.cursor_color = Some(c),
                None => self.warn(line_no, key, val),
            },
            "cursor-text" => match Rgb::parse(val) {
                Some(c) => overrides.cursor_text = Some(c),
                None => self.warn(line_no, key, val),
            },
            "selection-background" => match Rgb::parse(val) {
                Some(c) => overrides.selection_background = Some(c),
                None => self.warn(line_no, key, val),
            },
            "selection-foreground" => match Rgb::parse(val) {
                Some(c) => overrides.selection_foreground = Some(c),
                None => self.warn(line_no, key, val),
            },
            "palette" => {
                if let Some((idx_s, col_s)) = val.split_once('=')
                    && let Ok(idx) = idx_s.trim().parse::<usize>()
                    && idx < 256
                    && let Some(c) = Rgb::parse(col_s)
                {
                    overrides.palette.push((idx, c));
                } else {
                    self.warn(line_no, key, val);
                }
            }
            "background-opacity" => match val.parse::<f32>() {
                Ok(v) => self.background_opacity = v.clamp(0.05, 1.0),
                Err(_) => self.warn(line_no, key, val),
            },
            "cursor-opacity" => match val.parse::<f32>() {
                Ok(v) => self.cursor_opacity = v.clamp(0.05, 1.0),
                Err(_) => self.warn(line_no, key, val),
            },
            "bold-is-bright" => match parse_bool(val) {
                Some(b) => self.bold_is_bright = b,
                None => self.warn(line_no, key, val),
            },
            "minimum-contrast" => match val.parse::<f32>() {
                Ok(v) if (1.0..=21.0).contains(&v) => self.minimum_contrast = v,
                _ => self.warn(line_no, key, val),
            },
            "cursor-style" => match val.to_ascii_lowercase().as_str() {
                "block" => self.cursor_style = CursorStyle::Block,
                "bar" | "beam" => self.cursor_style = CursorStyle::Bar,
                "underline" => self.cursor_style = CursorStyle::Underline,
                "block_hollow" | "hollow" => self.cursor_style = CursorStyle::BlockHollow,
                _ => self.warn(line_no, key, val),
            },
            "cursor-style-blink" => match parse_bool(val) {
                Some(b) => self.cursor_blink = b,
                None => self.warn(line_no, key, val),
            },
            "title" => {
                self.title = non_empty(val);
            }
            "window-padding-x" => match parse_padding_pair(val) {
                Some(p) => self.window_padding_x = p,
                None => self.warn(line_no, key, val),
            },
            "window-padding-y" => match parse_padding_pair(val) {
                Some(p) => self.window_padding_y = p,
                None => self.warn(line_no, key, val),
            },
            "window-padding-balance" => match parse_bool(val) {
                Some(b) => self.window_padding_balance = b,
                None => self.warn(line_no, key, val),
            },
            "window-decoration" => match val.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" | "auto" | "client" | "server" => {
                    self.window_decoration = true;
                }
                "false" | "0" | "no" | "off" | "none" | "hidden" => {
                    self.window_decoration = false;
                }
                _ => self.warn(line_no, key, val),
            },
            "window-titlebar" | "gtk-titlebar" | "titlebar" | "system-titlebar" => {
                match val.to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" | "on" | "show" | "auto" => {
                        self.window_titlebar = true;
                    }
                    "false" | "0" | "no" | "off" | "hide" | "hidden" | "none" => {
                        self.window_titlebar = false;
                    }
                    _ => self.warn(line_no, key, val),
                }
            }
            "hide-title-bar"
            | "hide-titlebar"
            | "hide-system-title-bar"
            | "hide-system-titlebar"
            | "window-hide-titlebar" => match parse_bool(val) {
                Some(b) => self.window_titlebar = !b,
                None => self.warn(line_no, key, val),
            },
            "macos-titlebar-style" => {
                if val.eq_ignore_ascii_case("hidden") {
                    self.window_titlebar = false;
                } else if !val.is_empty() {
                    self.window_titlebar = true;
                }
            }
            "window-width" => match val.parse::<u16>() {
                Ok(w) if (10..=1000).contains(&w) => self.window_width = w,
                _ => self.warn(line_no, key, val),
            },
            "window-height" => match val.parse::<u16>() {
                Ok(h) if (4..=500).contains(&h) => self.window_height = h,
                _ => self.warn(line_no, key, val),
            },
            "window-vsync" => match parse_bool(val) {
                Some(b) => self.window_vsync = b,
                None => self.warn(line_no, key, val),
            },
            "window-show-tab-bar" | "gtk-tabs-location" => {
                match val.to_ascii_lowercase().as_str() {
                    "auto" | "top" | "bottom" => self.window_show_tab_bar = TabBarMode::Auto,
                    "always" => self.window_show_tab_bar = TabBarMode::Always,
                    "never" | "hidden" | "false" => self.window_show_tab_bar = TabBarMode::Never,
                    _ => self.warn(line_no, key, val),
                }
            }
            "window-show-status-bar"
            | "show-status-bar"
            | "status-bar"
            | "shortcut-bar"
            | "window-show-shortcut-bar" => match val.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" | "always" | "auto" | "show" | "bottom" => {
                    self.window_show_status_bar = true;
                }
                "false" | "0" | "no" | "off" | "never" | "hidden" | "hide" | "none" => {
                    self.window_show_status_bar = false;
                }
                _ => self.warn(line_no, key, val),
            },
            "editor-keymap" | "editor-mode" | "editor-editing-mode" => {
                match val.to_ascii_lowercase().as_str() {
                    "modeless" | "default" | "standard" => {
                        self.editor_keymap = EditorKeymap::Modeless;
                    }
                    "vim" | "modal" | "vi" => self.editor_keymap = EditorKeymap::Vim,
                    _ => self.warn(line_no, key, val),
                }
            }
            "backend" | "linux-display-server" | "display-server" => {
                if let Ok(bm) = val.parse::<BackendMode>() {
                    self.backend = bm;
                } else {
                    self.warn(line_no, key, val);
                }
            }
            "enable-wayland" => {
                if let Some(b) = parse_bool(val) {
                    self.backend = if b {
                        BackendMode::Wayland
                    } else {
                        BackendMode::X11
                    };
                } else {
                    self.warn(line_no, key, val);
                }
            }
            "auto-update-check" | "update-check" | "check-update" => {
                match crate::update::UpdateCheckMode::parse(val) {
                    Some(m) => self.auto_update_check = m,
                    None => self.warn(line_no, key, val),
                }
            }
            "command" => {
                self.command = non_empty(val);
            }
            "working-directory" => {
                if val.eq_ignore_ascii_case("inherit") || val.is_empty() {
                    self.working_directory = None;
                } else {
                    self.working_directory = Some(expand_tilde(val).to_string_lossy().into_owned());
                }
            }
            "term" => {
                if !val.is_empty() {
                    self.term = val.to_string();
                }
            }
            "env" => {
                if val.is_empty() {
                    self.env.clear();
                } else if let Some((k, v)) = val.split_once('=') {
                    self.env.push((k.trim().to_string(), v.to_string()));
                } else {
                    self.warn(line_no, key, val);
                }
            }
            "scrollback-limit" => match val.parse::<usize>() {
                Ok(n) => self.scrollback_limit = n.min(1_000_000),
                Err(_) => self.warn(line_no, key, val),
            },
            "mouse-hide-while-typing" => match parse_bool(val) {
                Some(b) => self.mouse_hide_while_typing = b,
                None => self.warn(line_no, key, val),
            },
            "mouse-scroll-multiplier" => match val.parse::<f32>() {
                Ok(m) if (0.1..=50.0).contains(&m) => self.mouse_scroll_multiplier = m,
                _ => self.warn(line_no, key, val),
            },
            "copy-on-select" => match val.to_ascii_lowercase().as_str() {
                "true" | "clipboard" | "1" | "yes" => self.copy_on_select = true,
                "false" | "0" | "no" => self.copy_on_select = false,
                _ => self.warn(line_no, key, val),
            },
            "clipboard-trim-trailing-spaces" => match parse_bool(val) {
                Some(b) => self.clipboard_trim_trailing_spaces = b,
                None => self.warn(line_no, key, val),
            },
            "link-url" => match parse_bool(val) {
                Some(b) => self.link_url = b,
                None => self.warn(line_no, key, val),
            },
            "keybind" => {
                if val.eq_ignore_ascii_case("clear") {
                    self.keybinds.clear();
                } else if let Some(kb) = parse_keybind(val) {
                    apply_keybind_list(&mut self.keybinds, kb);
                } else {
                    self.warn(line_no, key, val);
                }
            }
            // Quietly accept common Ghostty platform keys so a shared Ghostty config works cleanly
            "clipboard-read"
            | "clipboard-write"
            | "clipboard-paste-protection"
            | "confirm-close-surface"
            | "resize-overlay"
            | "grapheme-width-method"
            | "macos-option-as-alt"
            | "gtk-single-instance"
            | "shell-integration"
            | "shell-integration-features" => {}
            _ => {
                self.warnings.push(format!(
                    "Line {}: unknown configuration key `{}`",
                    line_no, key
                ));
            }
        }
    }

    fn warn(&mut self, line_no: usize, key: &str, val: &str) {
        self.warnings.push(format!(
            "Line {}: invalid value `{}` for `{}`",
            line_no, val, key
        ));
    }

    pub fn format_show_config(&self) -> String {
        let mut out = String::new();
        out.push_str("# Ocean resolved configuration (Ghostty format)\n");
        if let Some(p) = &self.loaded_path {
            out.push_str(&format!("# Loaded from: {}\n", p.display()));
        } else {
            out.push_str("# Loaded from: built-in defaults\n");
        }
        for f in &self.font_families {
            out.push_str(&format!("font-family = {}\n", f));
        }
        if let Some(mm) = &self.font_family_myanmar {
            out.push_str(&format!("font-family-myanmar = {}\n", mm));
        }
        out.push_str(&format!("font-size = {}\n", self.font_size));
        out.push_str(&format!("font-thicken = {}\n", self.font_thicken));
        out.push_str(&format!(
            "myanmar-font-scale = {}\n",
            self.myanmar_font_scale
        ));
        out.push_str(&format!(
            "myanmar-zawgyi = {}\n",
            match self.myanmar_zawgyi {
                ZawgyiMode::Off => "off",
                ZawgyiMode::Auto => "auto",
                ZawgyiMode::Force => "force",
            }
        ));
        out.push_str(&format!("theme = {}\n", self.theme_name));
        out.push_str(&format!(
            "background = {}\n",
            self.colors.background.to_hex()
        ));
        out.push_str(&format!(
            "foreground = {}\n",
            self.colors.foreground.to_hex()
        ));
        out.push_str(&format!(
            "background-opacity = {}\n",
            self.background_opacity
        ));
        out.push_str(&format!(
            "cursor-color = {}\n",
            self.colors.cursor_color.to_hex()
        ));
        out.push_str(&format!(
            "cursor-style = {}\n",
            match self.cursor_style {
                CursorStyle::Block => "block",
                CursorStyle::Bar => "bar",
                CursorStyle::Underline => "underline",
                CursorStyle::BlockHollow => "block_hollow",
            }
        ));
        out.push_str(&format!("cursor-style-blink = {}\n", self.cursor_blink));
        out.push_str(&format!(
            "window-padding-x = {},{}\n",
            self.window_padding_x.0, self.window_padding_x.1
        ));
        out.push_str(&format!(
            "window-padding-y = {},{}\n",
            self.window_padding_y.0, self.window_padding_y.1
        ));
        out.push_str(&format!("window-width = {}\n", self.window_width));
        out.push_str(&format!("window-height = {}\n", self.window_height));
        out.push_str(&format!("window-titlebar = {}\n", self.window_titlebar));
        out.push_str(&format!("window-decoration = {}\n", self.window_decoration));
        out.push_str(&format!(
            "window-show-status-bar = {}\n",
            self.window_show_status_bar
        ));
        out.push_str(&format!(
            "editor-keymap = {}\n",
            self.editor_keymap.as_str()
        ));
        out.push_str(&format!("backend = {}\n", self.backend));
        out.push_str(&format!(
            "auto-update-check = {}\n",
            self.auto_update_check.as_str()
        ));
        out.push_str(&format!("scrollback-limit = {}\n", self.scrollback_limit));
        out.push_str(&format!("term = {}\n", self.term));
        out
    }
}

fn unquote(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_padding_pair(s: &str) -> Option<(u32, u32)> {
    if let Some((a, b)) = s.split_once(',') {
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    } else {
        let v = s.trim().parse().ok()?;
        Some((v, v))
    }
}

fn resolve_theme_spec(val: &str) -> &str {
    if val.contains("dark:") || val.contains("light:") {
        for part in val.split(',') {
            if let Some(d) = part.trim().strip_prefix("dark:") {
                return d.trim();
            }
        }
        for part in val.split(',') {
            if let Some(l) = part.trim().strip_prefix("light:") {
                return l.trim();
            }
        }
    }
    val.trim()
}

pub fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ghostty_style_config_cleanly() {
        let sample = r#"
            # Ghostty-compatible Ocean config
            font-family = Cascadia Code NF
            font-family = Padauk
            font-size = 15
            theme = catppuccin-mocha
            background = #11161d
            background-opacity = 0.92
            palette = 1=#ff5555
            cursor-style = bar
            cursor-style-blink = true
            window-padding-x = 12,14
            window-padding-y = 10
            adjust-cell-height = 10%
            keybind = ctrl+shift+c=copy_to_clipboard
            keybind = alt+1=goto_tab:1
            myanmar-zawgyi = auto
        "#;
        let cfg = Config::load_from_str(sample);
        assert!(cfg.warnings.is_empty(), "warnings: {:?}", cfg.warnings);
        assert_eq!(cfg.font_families, vec!["Cascadia Code NF", "Padauk"]);
        assert_eq!(cfg.font_size, 15.0);
        assert_eq!(cfg.theme_name, "catppuccin-mocha");
        assert_eq!(cfg.colors.background, Rgb::new(0x11, 0x16, 0x1d));
        assert_eq!(cfg.colors.ansi[1], Rgb::new(0xff, 0x55, 0x55));
        assert_eq!(cfg.cursor_style, CursorStyle::Bar);
        assert!(cfg.cursor_blink);
        assert_eq!(cfg.window_padding_x, (12, 14));
        assert_eq!(cfg.window_padding_y, (10, 10));
        assert_eq!(cfg.myanmar_zawgyi, ZawgyiMode::Auto);
    }

    #[test]
    fn parses_editor_keymap() {
        let vim = Config::load_from_str("editor-keymap = vim");
        assert!(vim.warnings.is_empty(), "{:?}", vim.warnings);
        assert_eq!(vim.editor_keymap, EditorKeymap::Vim);

        let modeless = Config::load_from_str("editor-keymap = modeless");
        assert!(modeless.warnings.is_empty(), "{:?}", modeless.warnings);
        assert_eq!(modeless.editor_keymap, EditorKeymap::Modeless);
    }

    #[test]
    fn supports_hiding_system_title_bar_and_split_keybinds() {
        let starter = include_str!("../../config.example");
        let default_cfg = Config::load_from_str(starter);
        assert!(
            default_cfg.warnings.is_empty(),
            "config.example warnings: {:?}",
            default_cfg.warnings
        );
        assert!(default_cfg.effective_decorations());

        for snippet in [
            "window-titlebar = false",
            "window-titlebar = hidden",
            "gtk-titlebar = false",
            "window-decoration = false",
            "window-decoration = none",
            "hide-title-bar = true",
            "hide-system-titlebar = true",
            "macos-titlebar-style = hidden",
        ] {
            let cfg = Config::load_from_str(snippet);
            assert!(
                cfg.warnings.is_empty(),
                "unexpected warning for `{snippet}`: {:?}",
                cfg.warnings
            );
            assert!(
                !cfg.effective_decorations(),
                "`{snippet}` should hide system title bar"
            );
        }

        let kb_cfg = Config::load_from_str(
            r#"
            window-show-status-bar = false
            keybind = ctrl+shift+d=new_split:right
            keybind = ctrl+shift+e=new_split:down
            keybind = alt+shift+left=goto_split:left
            keybind = ctrl+alt+h=resize_split:left,40
            keybind = ctrl+shift+p=toggle_context_menu
            keybind = menu=toggle_context_menu
            keybind = ctrl+shift+b=toggle_status_bar
            "#,
        );
        assert!(kb_cfg.warnings.is_empty(), "{:?}", kb_cfg.warnings);
        assert!(!kb_cfg.window_show_status_bar);
        assert_eq!(
            keybind::shortcut_for_action(
                &Config::default().keybinds,
                &keybind::Action::ToggleContextMenu
            )
            .as_deref(),
            Some("Ctrl+Shift+P")
        );
    }

    #[test]
    fn verifies_save_theme_persists_and_updates_in_place() {
        let temp_dir = std::env::temp_dir().join(format!("ocean_test_cfg_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let cfg_path = temp_dir.join("config");

        let initial_content =
            "background-opacity = 0.96\ncursor-style = block\nwindow-titlebar = false\n";
        fs::write(&cfg_path, initial_content).unwrap();

        let mut cfg = Config::load(Some(&cfg_path));
        assert_eq!(cfg.theme_name, "ocean-dark");

        // Save new theme: should append `theme = tokyo-night`
        cfg.save_theme("tokyo-night").unwrap();

        let reloaded = Config::load(Some(&cfg_path));
        assert_eq!(reloaded.theme_name, "tokyo-night");
        let content_after = fs::read_to_string(&cfg_path).unwrap();
        assert!(content_after.contains("background-opacity = 0.96"));
        assert!(content_after.contains("theme = tokyo-night"));

        // Update theme in-place: should replace `theme = tokyo-night` with `theme = nord`
        cfg.save_theme("nord").unwrap();
        let reloaded2 = Config::load(Some(&cfg_path));
        assert_eq!(reloaded2.theme_name, "nord");
        let content_after2 = fs::read_to_string(&cfg_path).unwrap();
        assert_eq!(content_after2.matches("theme =").count(), 1);
        assert!(content_after2.contains("theme = nord"));
        assert!(!content_after2.contains("theme = tokyo-night"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn verifies_ensure_config_file_creates_and_preserves() {
        let temp_dir =
            std::env::temp_dir().join(format!("ocean_test_ensure_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let cfg_path = temp_dir.join("ocean/config");

        let mut cfg = Config::default();
        cfg.loaded_path = Some(cfg_path.clone());

        // First call: creates file from template
        assert!(!cfg_path.exists());
        let res_path = cfg.ensure_config_file().unwrap();
        assert_eq!(res_path, cfg_path);
        assert!(cfg_path.exists());
        let text = fs::read_to_string(&cfg_path).unwrap();
        assert!(text.contains("Ocean Terminal Configuration"));

        // Modifying the file and calling ensure_config_file again preserves existing content
        fs::write(&cfg_path, "theme = nord\n").unwrap();
        let res_path2 = cfg.ensure_config_file().unwrap();
        assert_eq!(res_path2, cfg_path);
        let text2 = fs::read_to_string(&cfg_path).unwrap();
        assert_eq!(text2, "theme = nord\n");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parses_backend_modes_correctly() {
        let mut cfg = Config::default();
        assert_eq!(cfg.backend, BackendMode::Auto);

        cfg.apply_cli_kv("backend", "x11");
        assert_eq!(cfg.backend, BackendMode::X11);

        cfg.apply_cli_kv("backend", "wayland");
        assert_eq!(cfg.backend, BackendMode::Wayland);

        cfg.apply_cli_kv("linux-display-server", "xwayland");
        assert_eq!(cfg.backend, BackendMode::X11);

        cfg.apply_cli_kv("enable-wayland", "false");
        assert_eq!(cfg.backend, BackendMode::X11);

        cfg.apply_cli_kv("enable-wayland", "true");
        assert_eq!(cfg.backend, BackendMode::Wayland);

        cfg.apply_cli_kv("backend", "auto");
        assert_eq!(cfg.backend, BackendMode::Auto);
    }
}
