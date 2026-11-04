use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse `#rrggbb`, `rrggbb`, `#rgb`, or `rgb:rr/gg/bb` (Ghostty-compatible).
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim().trim_matches('"').trim_matches('\'');
        if let Some(rest) = s.strip_prefix("rgb:") {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() == 3 {
                let parse_comp = |p: &str| -> Option<u8> {
                    match p.len() {
                        2 => u8::from_str_radix(p, 16).ok(),
                        4 => u16::from_str_radix(p, 16).ok().map(|v| (v >> 8) as u8),
                        1 => u8::from_str_radix(p, 16).ok().map(|v| v * 17),
                        _ => None,
                    }
                };
                return Some(Self::new(
                    parse_comp(parts[0])?,
                    parse_comp(parts[1])?,
                    parse_comp(parts[2])?,
                ));
            }
            return None;
        }

        let hex = s.strip_prefix('#').unwrap_or(s);
        match hex.len() {
            6 | 8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Self::new(r, g, b))
            }
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                Some(Self::new(r, g, b))
            }
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn to_f32_rgba(self, alpha: f32) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            alpha,
        ]
    }

    pub fn relative_luminance(self) -> f32 {
        fn chan(c: u8) -> f32 {
            let v = c as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * chan(self.r) + 0.7152 * chan(self.g) + 0.0722 * chan(self.b)
    }

    pub fn contrast_ratio(self, other: Self) -> f32 {
        let l1 = self.relative_luminance();
        let l2 = other.relative_luminance();
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Nudge foreground lightness toward white/black until minimum contrast is satisfied.
    pub fn ensure_contrast(self, bg: Self, min_ratio: f32) -> Self {
        if min_ratio <= 1.0 || self.contrast_ratio(bg) >= min_ratio {
            return self;
        }
        let bg_lum = bg.relative_luminance();
        let target = if bg_lum < 0.5 {
            Rgb::new(255, 255, 255)
        } else {
            Rgb::new(0, 0, 0)
        };
        let mut lo = 0.0_f32;
        let mut hi = 1.0_f32;
        let mut best = target;
        for _ in 0..10 {
            let mid = (lo + hi) * 0.5;
            let cand = Rgb::new(
                (self.r as f32 + (target.r as f32 - self.r as f32) * mid).round() as u8,
                (self.g as f32 + (target.g as f32 - self.g as f32) * mid).round() as u8,
                (self.b as f32 + (target.b as f32 - self.b as f32) * mid).round() as u8,
            );
            if cand.contrast_ratio(bg) >= min_ratio {
                best = cand;
                hi = mid;
            } else {
                lo = mid;
            }
        }
        best
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub is_dark: bool,
    pub bg_preview: Rgb,
    pub accent_preview: Rgb,
}

pub const CURATED_THEMES: [ThemeInfo; 10] = [
    // 5 Dark Mode Themes
    ThemeInfo {
        id: "ocean-dark",
        label: "Ocean Dark",
        is_dark: true,
        bg_preview: Rgb::new(0x0d, 0x13, 0x1a),
        accent_preview: Rgb::new(0x58, 0xd1, 0xeb),
    },
    ThemeInfo {
        id: "tokyo-night",
        label: "Tokyo Night",
        is_dark: true,
        bg_preview: Rgb::new(0x1a, 0x1b, 0x26),
        accent_preview: Rgb::new(0x7a, 0xa2, 0xf7),
    },
    ThemeInfo {
        id: "catppuccin-mocha",
        label: "Catppuccin Mocha",
        is_dark: true,
        bg_preview: Rgb::new(0x1e, 0x1e, 0x2e),
        accent_preview: Rgb::new(0xcb, 0xa6, 0xf7),
    },
    ThemeInfo {
        id: "gruvbox-dark",
        label: "Gruvbox Dark",
        is_dark: true,
        bg_preview: Rgb::new(0x28, 0x28, 0x28),
        accent_preview: Rgb::new(0xfa, 0xbd, 0x2f),
    },
    ThemeInfo {
        id: "nord",
        label: "Nord",
        is_dark: true,
        bg_preview: Rgb::new(0x2e, 0x34, 0x40),
        accent_preview: Rgb::new(0x88, 0xc0, 0xd0),
    },
    // 5 Light Mode Themes
    ThemeInfo {
        id: "ocean-light",
        label: "Ocean Light",
        is_dark: false,
        bg_preview: Rgb::new(0xf7, 0xf9, 0xfc),
        accent_preview: Rgb::new(0x09, 0x69, 0xda),
    },
    ThemeInfo {
        id: "tokyo-night-light",
        label: "Tokyo Night Light",
        is_dark: false,
        bg_preview: Rgb::new(0xe1, 0xe2, 0xe7),
        accent_preview: Rgb::new(0x2e, 0x7d, 0xe9),
    },
    ThemeInfo {
        id: "catppuccin-latte",
        label: "Catppuccin Latte",
        is_dark: false,
        bg_preview: Rgb::new(0xef, 0xf1, 0xf5),
        accent_preview: Rgb::new(0x88, 0x39, 0xef),
    },
    ThemeInfo {
        id: "gruvbox-light",
        label: "Gruvbox Light",
        is_dark: false,
        bg_preview: Rgb::new(0xfb, 0xf1, 0xc7),
        accent_preview: Rgb::new(0xd6, 0x5d, 0x0e),
    },
    ThemeInfo {
        id: "solarized-light",
        label: "Solarized Light",
        is_dark: false,
        bg_preview: Rgb::new(0xfd, 0xf6, 0xe3),
        accent_preview: Rgb::new(0x26, 0x8b, 0xd2),
    },
];

#[derive(Debug, Clone)]
pub struct ThemePalette {
    pub name: String,
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor_color: Rgb,
    pub cursor_text: Rgb,
    pub selection_background: Rgb,
    pub selection_foreground: Option<Rgb>,
    pub ansi: [Rgb; 256],
}

impl ThemePalette {
    pub fn build_256(ansi16: [Rgb; 16]) -> [Rgb; 256] {
        let mut out = [Rgb::new(0, 0, 0); 256];
        out[..16].copy_from_slice(&ansi16);
        // 6x6x6 xterm color cube (16..232)
        let levels = [0u8, 95, 135, 175, 215, 255];
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    let idx = 16 + r * 36 + g * 6 + b;
                    out[idx] = Rgb::new(levels[r], levels[g], levels[b]);
                }
            }
        }
        // Grayscale ramp (232..256)
        for i in 0..24 {
            let v = (8 + i * 10) as u8;
            out[232 + i] = Rgb::new(v, v, v);
        }
        out
    }

    pub fn curated_themes() -> &'static [ThemeInfo; 10] {
        &CURATED_THEMES
    }

    pub fn builtin_names() -> &'static [&'static str] {
        &[
            "ocean-dark",
            "tokyo-night",
            "catppuccin-mocha",
            "gruvbox-dark",
            "nord",
            "ocean-light",
            "tokyo-night-light",
            "catppuccin-latte",
            "gruvbox-light",
            "solarized-light",
        ]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let key = name.trim().to_ascii_lowercase().replace(' ', "-");
        let (bg, fg, cursor, cursor_txt, sel_bg, sel_fg, hex16): (
            &str,
            &str,
            &str,
            &str,
            &str,
            Option<&str>,
            [&str; 16],
        ) = match key.as_str() {
            "ocean-dark" | "ocean" | "default" => (
                "#0d131a",
                "#e6edf3",
                "#58d1eb",
                "#0d131a",
                "#243b53",
                None,
                [
                    "#151d28", "#f47067", "#57ab5a", "#e0af68", "#539bf5", "#b083f0", "#39c5cf",
                    "#c9d1d9", "#485466", "#ff938a", "#6bc46d", "#f2cc60", "#6cb6ff", "#dcbdfb",
                    "#56d4dd", "#f0f6fc",
                ],
            ),
            "ocean-light" => (
                "#f7f9fc",
                "#1b2430",
                "#0969da",
                "#ffffff",
                "#cde3ff",
                Some("#1b2430"),
                [
                    "#1b2430", "#cf222e", "#1a7f37", "#9a6700", "#0969da", "#8250df", "#0e7490",
                    "#57606a", "#6e7781", "#a40e26", "#2da44e", "#bf8700", "#218bff", "#a475f9",
                    "#1592b3", "#24292f",
                ],
            ),
            "tokyo-night-light" | "tokyo-day" => (
                "#e1e2e7",
                "#3760bf",
                "#2e7de9",
                "#ffffff",
                "#b7c1e3",
                Some("#1d202f"),
                [
                    "#1d202f", "#f52a65", "#587539", "#8c6c3e", "#2e7de9", "#9854f1", "#007197",
                    "#6172b0", "#7880a8", "#f52a65", "#587539", "#8c6c3e", "#2e7de9", "#9854f1",
                    "#007197", "#2e3c64",
                ],
            ),
            "gruvbox-light" => (
                "#fbf1c7",
                "#3c3836",
                "#af3a03",
                "#fbf1c7",
                "#d5c4a1",
                Some("#282828"),
                [
                    "#3c3836", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a",
                    "#7c6f64", "#928374", "#9d0006", "#79740e", "#b57614", "#076678", "#8f3f71",
                    "#427b58", "#282828",
                ],
            ),
            "solarized-light" => (
                "#fdf6e3",
                "#586e75",
                "#268bd2",
                "#fdf6e3",
                "#e3dcc7",
                Some("#073642"),
                [
                    "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198",
                    "#657b83", "#839496", "#cb4b16", "#659b00", "#9c7100", "#1e78bf", "#6c71c4",
                    "#208c84", "#002b36",
                ],
            ),
            "ghostty-default" | "ghostty" => (
                "#282c34",
                "#ffffff",
                "#ffffff",
                "#282c34",
                "#474e5d",
                None,
                [
                    "#1d1f21", "#cc6666", "#b5bd68", "#f0c674", "#81a2be", "#b294bb", "#8abeb7",
                    "#c5c8c6", "#666666", "#d54e53", "#b9ca4a", "#e7c547", "#7aa6da", "#c397d8",
                    "#70c0b1", "#eaeaea",
                ],
            ),
            "catppuccin-mocha" => (
                "#1e1e2e",
                "#cdd6f4",
                "#f5e0dc",
                "#1e1e2e",
                "#45475a",
                Some("#cdd6f4"),
                [
                    "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5",
                    "#bac2de", "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7",
                    "#94e2d5", "#a6adc8",
                ],
            ),
            "catppuccin-latte" => (
                "#eff1f5",
                "#4c4f69",
                "#1e66f5",
                "#eff1f5",
                "#ccd0da",
                Some("#4c4f69"),
                [
                    "#5c5f77", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299",
                    "#7c7f93", "#6c6f85", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb",
                    "#179299", "#4c4f69",
                ],
            ),
            "tokyo-night" => (
                "#1a1b26",
                "#c0caf5",
                "#c0caf5",
                "#1a1b26",
                "#33467c",
                None,
                [
                    "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff",
                    "#a9b1d6", "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7",
                    "#7dcfff", "#c0caf5",
                ],
            ),
            "gruvbox-dark" => (
                "#282828",
                "#ebdbb2",
                "#ebdbb2",
                "#282828",
                "#504945",
                None,
                [
                    "#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a",
                    "#a89984", "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b",
                    "#8ec07c", "#ebdbb2",
                ],
            ),
            "nord" => (
                "#2e3440",
                "#d8dee9",
                "#d8dee9",
                "#2e3440",
                "#434c5e",
                None,
                [
                    "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0",
                    "#e5e9f0", "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead",
                    "#8fbcbb", "#eceff4",
                ],
            ),
            "dracula" => (
                "#282a36",
                "#f8f8f2",
                "#f8f8f2",
                "#282a36",
                "#44475a",
                None,
                [
                    "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd",
                    "#f8f8f2", "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df",
                    "#a4ffff", "#ffffff",
                ],
            ),
            "rose-pine" => (
                "#191724",
                "#e0def4",
                "#524f67",
                "#e0def4",
                "#2a283e",
                None,
                [
                    "#26233a", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba",
                    "#e0def4", "#6e6a86", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7",
                    "#ebbcba", "#e0def4",
                ],
            ),
            "kanagawa" => (
                "#1f1f28",
                "#dcd7ba",
                "#c8c093",
                "#1f1f28",
                "#2d4f67",
                None,
                [
                    "#090618", "#c34043", "#76946a", "#c0a36e", "#7e9cd8", "#957fb8", "#6a9589",
                    "#c8c093", "#727169", "#e82424", "#98bb6c", "#e6c384", "#7fb4ca", "#938aa9",
                    "#7aa89f", "#dcd7ba",
                ],
            ),
            _ => return Self::load_theme_file(name),
        };

        let mut ansi16 = [Rgb::new(0, 0, 0); 16];
        for (i, h) in hex16.iter().enumerate() {
            ansi16[i] = Rgb::parse(h)?;
        }

        let canonical = match key.as_str() {
            "ocean" | "default" => "ocean-dark".to_string(),
            "tokyo-day" => "tokyo-night-light".to_string(),
            "ghostty" => "ghostty-default".to_string(),
            _ => key,
        };

        Some(Self {
            name: canonical,
            background: Rgb::parse(bg)?,
            foreground: Rgb::parse(fg)?,
            cursor_color: Rgb::parse(cursor)?,
            cursor_text: Rgb::parse(cursor_txt)?,
            selection_background: Rgb::parse(sel_bg)?,
            selection_foreground: sel_fg.and_then(Rgb::parse),
            ansi: Self::build_256(ansi16),
        })
    }

    fn load_theme_file(name: &str) -> Option<Self> {
        let mut candidates = Vec::new();
        let p = Path::new(name);
        if p.is_absolute() || p.exists() {
            candidates.push(p.to_path_buf());
        }
        if let Some(cfg_dir) = dirs_config_dir() {
            candidates.push(cfg_dir.join("ocean/themes").join(name));
            candidates.push(cfg_dir.join("ghostty/themes").join(name));
        }
        candidates.push(PathBuf::from("/usr/share/ghostty/themes").join(name));

        let content = candidates
            .into_iter()
            .find_map(|path| fs::read_to_string(path).ok())?;

        let mut base = Self::from_name("ocean-dark").unwrap();
        base.name = name.to_string();
        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim().trim_matches('"');
            match key {
                "background" => {
                    if let Some(c) = Rgb::parse(val) {
                        base.background = c;
                    }
                }
                "foreground" => {
                    if let Some(c) = Rgb::parse(val) {
                        base.foreground = c;
                    }
                }
                "cursor-color" => {
                    if let Some(c) = Rgb::parse(val) {
                        base.cursor_color = c;
                    }
                }
                "cursor-text" => {
                    if let Some(c) = Rgb::parse(val) {
                        base.cursor_text = c;
                    }
                }
                "selection-background" => {
                    if let Some(c) = Rgb::parse(val) {
                        base.selection_background = c;
                    }
                }
                "selection-foreground" => {
                    base.selection_foreground = Rgb::parse(val);
                }
                "palette" => {
                    if let Some((idx_s, col_s)) = val.split_once('=')
                        && let Ok(idx) = idx_s.trim().parse::<usize>()
                        && idx < 256
                        && let Some(c) = Rgb::parse(col_s)
                    {
                        base.ansi[idx] = c;
                    }
                }
                _ => {}
            }
        }
        Some(base)
    }
}

pub fn dirs_config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Some(PathBuf::from(xdg));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}
