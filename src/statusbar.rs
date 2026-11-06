use crate::config::keybind::{
    Action, Keybind, SplitDirection, SplitFocusTarget, shortcut_for_action,
};
use crate::config::{Config, ZawgyiMode};
use crate::help::HelpDialog;
use crate::menu::ContextMenu;

/// The bottom status bar renders slightly smaller than the terminal grid.
pub const STATUS_BAR_TEXT_SCALE: f32 = 0.80;
/// Vertical padding above/below the status bar text (before scale factor).
pub const STATUS_BAR_VERTICAL_PAD: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusMode {
    Normal,
    Tui,
    Scroll,
    Menu,
    Theme,
    Help,
}

impl StatusMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "OCEAN",
            Self::Tui => "TUI",
            Self::Scroll => "SCROLL",
            Self::Menu => "MENU",
            Self::Theme => "THEME",
            Self::Help => "HELP",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatusClickTarget {
    Execute(Action),
    OpenThemeMenu,
    ToggleMetricsDetail,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl BarRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Debug, Clone)]
pub struct StatusChip {
    pub hit_rect: BarRect,
    pub key_rect: BarRect,
    pub key_text: String,
    pub key_text_x: f32,
    pub label_text: &'static str,
    pub label_x: f32,
    pub target: Option<StatusClickTarget>,
}

#[derive(Debug, Clone)]
pub struct StatusPill {
    pub rect: BarRect,
    pub text: String,
    pub text_x: f32,
    pub target: Option<StatusClickTarget>,
}

#[derive(Debug, Clone)]
pub struct StatusBarGeometry {
    pub y: f32,
    pub h: f32,
    pub w: f32,
    pub mode: StatusMode,
    pub mode_pill: StatusPill,
    pub info_pill: Option<StatusPill>,
    pub chips: Vec<StatusChip>,
    pub zawgyi_pill: Option<StatusPill>,
    pub resource_pill: Option<StatusPill>,
    pub theme_pill: Option<StatusPill>,
    pub update_pill: Option<StatusPill>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatusBarHover {
    pub hovered_id: Option<usize>,
}

impl StatusBarHover {
    /// Update hover state from cursor coordinates. Returns `(needs_redraw, pointer_cursor)`.
    pub fn update(&mut self, geom: &StatusBarGeometry, px: f32, py: f32) -> (bool, bool) {
        let next = geom.interactive_id_at(px, py);
        let changed = self.hovered_id != next;
        self.hovered_id = next;
        (changed, next.is_some())
    }

    pub fn clear(&mut self) -> bool {
        self.hovered_id.take().is_some()
    }
}

pub struct StatusBarContext<'a> {
    pub active_pane_index: usize,
    pub num_panes: usize,
    pub num_tabs: usize,
    pub display_offset: usize,
    pub tui_mode: bool,
    pub context_menu: Option<&'a ContextMenu>,
    pub help_dialog: Option<&'a HelpDialog>,
    pub available_update: Option<&'a crate::update::UpdateInfo>,
    pub metrics: Option<&'a crate::metrics::ProcessMetrics>,
}

impl StatusBarGeometry {
    // Interactive IDs:
    // 1 = mode_pill, 2 = info_pill, 3 = zawgyi_pill, 4 = theme_pill, 100 + idx = chip[idx]
    pub fn interactive_id_at(&self, px: f32, py: f32) -> Option<usize> {
        if py < self.y || py > self.y + self.h || px < 0.0 || px > self.w {
            return None;
        }
        if self.mode_pill.target.is_some() && self.mode_pill.rect.contains(px, py) {
            return Some(1);
        }
        if let Some(info) = &self.info_pill
            && info.target.is_some()
            && info.rect.contains(px, py)
        {
            return Some(2);
        }
        if let Some(zg) = &self.zawgyi_pill
            && zg.target.is_some()
            && zg.rect.contains(px, py)
        {
            return Some(3);
        }
        if let Some(th) = &self.theme_pill
            && th.target.is_some()
            && th.rect.contains(px, py)
        {
            return Some(4);
        }
        if let Some(up) = &self.update_pill
            && up.target.is_some()
            && up.rect.contains(px, py)
        {
            return Some(5);
        }
        if let Some(res) = &self.resource_pill
            && res.target.is_some()
            && res.rect.contains(px, py)
        {
            return Some(6);
        }
        for (i, chip) in self.chips.iter().enumerate() {
            if chip.target.is_some() && chip.hit_rect.contains(px, py) {
                return Some(100 + i);
            }
        }
        None
    }

    pub fn click_at(&self, px: f32, py: f32) -> Option<StatusClickTarget> {
        match self.interactive_id_at(px, py)? {
            1 => self.mode_pill.target.clone(),
            2 => self.info_pill.as_ref().and_then(|p| p.target.clone()),
            3 => self.zawgyi_pill.as_ref().and_then(|p| p.target.clone()),
            4 => self.theme_pill.as_ref().and_then(|p| p.target.clone()),
            5 => self.update_pill.as_ref().and_then(|p| p.target.clone()),
            6 => self.resource_pill.as_ref().and_then(|p| p.target.clone()),
            id if id >= 100 => self.chips.get(id - 100).and_then(|c| c.target.clone()),
            _ => None,
        }
    }

    pub fn compute(
        config: &Config,
        ctx: &StatusBarContext<'_>,
        cell_w: f32,
        cell_h: f32,
        win_w: u32,
        win_h: u32,
        scale_factor: f64,
    ) -> Option<Self> {
        if !config.window_show_status_bar || win_w == 0 || win_h == 0 {
            return None;
        }
        let sf = scale_factor as f32;
        let cw = cell_w.max(5.0);
        let ch = cell_h.max(8.0);
        let bar_h = (ch + (STATUS_BAR_VERTICAL_PAD * sf).round()).round();
        if (win_h as f32) < bar_h + ch {
            return None;
        }
        let bar_y = (win_h as f32 - bar_h).round();
        let w = win_w as f32;

        let mode = if ctx.help_dialog.is_some() {
            StatusMode::Help
        } else if let Some(menu) = ctx.context_menu {
            if menu.submenu_open && menu.sub_hovered.is_some() {
                StatusMode::Theme
            } else {
                StatusMode::Menu
            }
        } else if ctx.display_offset > 0 {
            StatusMode::Scroll
        } else if ctx.tui_mode {
            StatusMode::Tui
        } else {
            StatusMode::Normal
        };

        // 1. Left Neovim-style mode badge (flush on left, ` OCEAN `, ` MENU `, etc.)
        let mode_label = format!(" {} ", mode.label());
        let mode_pad = (4.0 * sf).round();
        let mode_w = (mode_label.chars().count() as f32 * cw + mode_pad * 2.0).round();
        let mode_pill = StatusPill {
            rect: BarRect {
                x: 0.0,
                y: bar_y + 1.0,
                w: mode_w,
                h: (bar_h - 1.0).max(1.0),
            },
            text_x: mode_pad,
            text: mode_label,
            target: Some(StatusClickTarget::Execute(Action::ToggleContextMenu)),
        };

        let mut left_cursor = mode_w;

        // 2. Optional secondary info segment right next to mode badge (pane `1/3` or scroll `+24L`)
        let info_pill = if mode == StatusMode::Scroll && ctx.display_offset > 0 {
            let txt = format!(" +{}L ", ctx.display_offset);
            let iw = (txt.chars().count() as f32 * cw + (4.0 * sf).round()).round();
            let pill = StatusPill {
                rect: BarRect {
                    x: left_cursor,
                    y: bar_y + 1.0,
                    w: iw,
                    h: (bar_h - 1.0).max(1.0),
                },
                text_x: (left_cursor + (2.0 * sf).round()).round(),
                text: txt,
                target: Some(StatusClickTarget::Execute(Action::ScrollToBottom)),
            };
            left_cursor += iw;
            Some(pill)
        } else if ctx.num_panes > 1 {
            let txt = format!(" {}/{} ", ctx.active_pane_index + 1, ctx.num_panes);
            let iw = (txt.chars().count() as f32 * cw + (4.0 * sf).round()).round();
            let pill = StatusPill {
                rect: BarRect {
                    x: left_cursor,
                    y: bar_y + 1.0,
                    w: iw,
                    h: (bar_h - 1.0).max(1.0),
                },
                text_x: (left_cursor + (2.0 * sf).round()).round(),
                text: txt,
                target: Some(StatusClickTarget::Execute(Action::GotoSplit(
                    SplitFocusTarget::Next,
                ))),
            };
            left_cursor += iw;
            Some(pill)
        } else {
            None
        };

        // 3. Right status pills (Theme pill + optional Resource / Zawgyi / Update badge)
        let mut right_limit = w - (6.0 * sf).round();
        let mut theme_pill = None;
        let mut resource_pill = None;
        let mut zawgyi_pill = None;

        let total_cols = (w / cw).floor() as usize;
        if total_cols >= 62 {
            let th_txt = format!(" {} ", config.theme_name);
            let th_pad = (4.0 * sf).round();
            let th_w = (th_txt.chars().count() as f32 * cw + th_pad * 2.0).round();
            let th_x = (w - th_w).round();
            if th_x > left_cursor + 24.0 * cw {
                theme_pill = Some(StatusPill {
                    rect: BarRect {
                        x: th_x,
                        y: bar_y + 1.0,
                        w: th_w,
                        h: (bar_h - 1.0).max(1.0),
                    },
                    text_x: (th_x + th_pad).round(),
                    text: th_txt,
                    target: Some(StatusClickTarget::OpenThemeMenu),
                });
                right_limit = th_x - (8.0 * sf).round();
            }
        }

        // Ocean System Resource Usage pill (compact or detail)
        if let Some(metrics) = ctx.metrics
            && total_cols >= 64
        {
            let res_label = if total_cols >= 82 {
                metrics.current_display()
            } else {
                &metrics.mem_only_label
            };
            let res_txt = format!(" {res_label} ");
            let res_pad = (4.0 * sf).round();
            let res_w = (res_txt.chars().count() as f32 * cw + res_pad * 2.0).round();
            let res_x = (right_limit - res_w).round();
            if res_x > left_cursor + 24.0 * cw {
                resource_pill = Some(StatusPill {
                    rect: BarRect {
                        x: res_x,
                        y: bar_y + 1.0,
                        w: res_w,
                        h: (bar_h - 1.0).max(1.0),
                    },
                    text_x: (res_x + res_pad).round(),
                    text: res_txt,
                    target: Some(StatusClickTarget::ToggleMetricsDetail),
                });
                right_limit = res_x - (8.0 * sf).round();
            }
        }

        if config.myanmar_zawgyi != ZawgyiMode::Off && total_cols >= 74 {
            let zg_txt = " ZG ".to_string();
            let zg_w = (zg_txt.chars().count() as f32 * cw + (4.0 * sf).round()).round();
            let zg_x = (right_limit - zg_w + (4.0 * sf).round()).round();
            if zg_x > left_cursor + 24.0 * cw {
                zawgyi_pill = Some(StatusPill {
                    rect: BarRect {
                        x: zg_x,
                        y: bar_y + 1.0,
                        w: zg_w,
                        h: (bar_h - 1.0).max(1.0),
                    },
                    text_x: (zg_x + (2.0 * sf).round()).round(),
                    text: zg_txt,
                    target: Some(StatusClickTarget::Execute(Action::ToggleMyanmarZawgyi)),
                });
                right_limit = zg_x - (8.0 * sf).round();
            }
        }

        let mut update_pill = None;
        if let Some(update) = ctx.available_update
            && total_cols >= 50
        {
            let up_txt = format!(" ↑ v{} ", update.version);
            let up_pad = (4.0 * sf).round();
            let up_w = (up_txt.chars().count() as f32 * cw + up_pad * 2.0).round();
            let up_x = (right_limit - up_w).round();
            if up_x > left_cursor + 20.0 * cw {
                update_pill = Some(StatusPill {
                    rect: BarRect {
                        x: up_x,
                        y: bar_y + 1.0,
                        w: up_w,
                        h: (bar_h - 1.0).max(1.0),
                    },
                    text_x: (up_x + up_pad).round(),
                    text: up_txt,
                    target: Some(StatusClickTarget::Execute(Action::TriggerUpdate)),
                });
                right_limit = up_x - (8.0 * sf).round();
            }
        }

        // 4. Context-aware shortcut chips fitted cleanly into `[left_cursor .. right_limit]`
        let candidates = build_candidate_chips(mode, ctx, &config.keybinds);
        let mut chips = Vec::with_capacity(candidates.len());

        let mut x = (left_cursor + (10.0 * sf).round()).round();
        let key_pad_x = (5.0 * sf).round();
        let key_label_gap = (5.0 * sf).round();
        let chip_gap = (12.0 * sf).round();
        let pill_pad_y = (2.0 * sf).round().max(2.0);
        let pill_y = bar_y + 1.0 + pill_pad_y;
        let pill_h = (bar_h - 1.0 - pill_pad_y * 2.0).max(ch - 2.0);

        for cand in candidates {
            let key_chars = cand.key.chars().count() as f32;
            let lbl_chars = cand.label.chars().count() as f32;
            let key_w = (key_chars * cw + key_pad_x * 2.0).round();
            let lbl_w = (lbl_chars * cw).round();
            let total_chip_w = key_w + key_label_gap + lbl_w;

            if x + total_chip_w > right_limit {
                continue;
            }

            let key_rect = BarRect {
                x,
                y: pill_y,
                w: key_w,
                h: pill_h,
            };
            let label_x = (x + key_w + key_label_gap).round();
            let hit_rect = BarRect {
                x: (x - 2.0).max(left_cursor),
                y: bar_y + 1.0,
                w: total_chip_w + 4.0,
                h: (bar_h - 1.0).max(1.0),
            };

            chips.push(StatusChip {
                hit_rect,
                key_rect,
                key_text: cand.key,
                key_text_x: (x + key_pad_x).round(),
                label_text: cand.label,
                label_x,
                target: cand.target,
            });

            x = (x + total_chip_w + chip_gap).round();
        }

        Some(Self {
            y: bar_y,
            h: bar_h,
            w,
            mode,
            mode_pill,
            info_pill,
            chips,
            zawgyi_pill,
            resource_pill,
            theme_pill,
            update_pill,
        })
    }
}

struct CandidateChip {
    key: String,
    label: &'static str,
    target: Option<StatusClickTarget>,
}

fn build_candidate_chips(
    mode: StatusMode,
    ctx: &StatusBarContext<'_>,
    binds: &[Keybind],
) -> Vec<CandidateChip> {
    let sc = |act: Action, fallback: &str| -> String {
        shortcut_for_action(binds, &act).unwrap_or_else(|| fallback.to_string())
    };

    let menu_key = sc(Action::ToggleContextMenu, "Ctrl+Shift+P");
    let help_key = sc(Action::ShowHelp, "Ctrl+Shift+?");
    let new_tab_key = sc(Action::NewTab, "Ctrl+Shift+T");
    let close_key = sc(Action::CloseSurface, "Ctrl+Shift+W");
    let bar_key = sc(Action::ToggleStatusBar, "Ctrl+Shift+B");
    let eq_key = sc(Action::EqualizeSplits, "Alt+Shift+=");
    let split_key = combined_split_shortcut(binds);

    match mode {
        StatusMode::Menu => vec![
            CandidateChip {
                key: "j/k".to_string(),
                label: "Navigate",
                target: None,
            },
            CandidateChip {
                key: "Enter".to_string(),
                label: "Select",
                target: None,
            },
            CandidateChip {
                key: "t".to_string(),
                label: "Themes",
                target: Some(StatusClickTarget::OpenThemeMenu),
            },
            CandidateChip {
                key: "1-9".to_string(),
                label: "Quick Pick",
                target: None,
            },
            CandidateChip {
                key: "Esc/q".to_string(),
                label: "Close",
                target: Some(StatusClickTarget::Execute(Action::ToggleContextMenu)),
            },
        ],
        StatusMode::Theme => vec![
            CandidateChip {
                key: "j/k".to_string(),
                label: "Select Theme",
                target: None,
            },
            CandidateChip {
                key: "1-0".to_string(),
                label: "Pick #1-10",
                target: None,
            },
            CandidateChip {
                key: "Enter".to_string(),
                label: "Apply",
                target: None,
            },
            CandidateChip {
                key: "h/Esc".to_string(),
                label: "Back",
                target: None,
            },
            CandidateChip {
                key: "q".to_string(),
                label: "Close",
                target: Some(StatusClickTarget::Execute(Action::ToggleContextMenu)),
            },
        ],
        StatusMode::Help => vec![
            CandidateChip {
                key: "1-5 / h/l".to_string(),
                label: "Section",
                target: None,
            },
            CandidateChip {
                key: "j/k / Space".to_string(),
                label: "Scroll",
                target: None,
            },
            CandidateChip {
                key: "g/G".to_string(),
                label: "Top/End",
                target: None,
            },
            CandidateChip {
                key: "Esc/q".to_string(),
                label: "Close",
                target: Some(StatusClickTarget::Execute(Action::ShowHelp)),
            },
        ],
        StatusMode::Scroll => vec![
            CandidateChip {
                key: "Shift+PgUp/Dn".to_string(),
                label: "Page",
                target: None,
            },
            CandidateChip {
                key: "Shift+End".to_string(),
                label: "Bottom",
                target: Some(StatusClickTarget::Execute(Action::ScrollToBottom)),
            },
            CandidateChip {
                key: menu_key,
                label: "Menu",
                target: Some(StatusClickTarget::Execute(Action::ToggleContextMenu)),
            },
            CandidateChip {
                key: help_key,
                label: "Help",
                target: Some(StatusClickTarget::Execute(Action::ShowHelp)),
            },
        ],
        StatusMode::Normal | StatusMode::Tui => {
            if ctx.num_panes > 1 {
                vec![
                    CandidateChip {
                        key: menu_key,
                        label: "Menu",
                        target: Some(StatusClickTarget::Execute(Action::ToggleContextMenu)),
                    },
                    CandidateChip {
                        key: "Alt+Shift+HJKL".to_string(),
                        label: "Focus",
                        target: Some(StatusClickTarget::Execute(Action::GotoSplit(
                            SplitFocusTarget::Next,
                        ))),
                    },
                    CandidateChip {
                        key: "Ctrl+Alt+HJKL".to_string(),
                        label: "Resize",
                        target: None,
                    },
                    CandidateChip {
                        key: help_key,
                        label: "Help",
                        target: Some(StatusClickTarget::Execute(Action::ShowHelp)),
                    },
                    CandidateChip {
                        key: close_key,
                        label: "Close",
                        target: Some(StatusClickTarget::Execute(Action::CloseSurface)),
                    },
                    CandidateChip {
                        key: split_key,
                        label: "Split",
                        target: Some(StatusClickTarget::Execute(Action::NewSplit(
                            SplitDirection::Auto,
                        ))),
                    },
                    CandidateChip {
                        key: eq_key,
                        label: "Equalize",
                        target: Some(StatusClickTarget::Execute(Action::EqualizeSplits)),
                    },
                ]
            } else {
                let mut list = vec![
                    CandidateChip {
                        key: menu_key,
                        label: "Menu",
                        target: Some(StatusClickTarget::Execute(Action::ToggleContextMenu)),
                    },
                    CandidateChip {
                        key: split_key,
                        label: "Split",
                        target: Some(StatusClickTarget::Execute(Action::NewSplit(
                            SplitDirection::Right,
                        ))),
                    },
                    CandidateChip {
                        key: new_tab_key,
                        label: "Tab",
                        target: Some(StatusClickTarget::Execute(Action::NewTab)),
                    },
                ];
                if ctx.num_tabs > 1 {
                    list.push(CandidateChip {
                        key: sc(Action::NextTab, "Ctrl+Tab"),
                        label: "Next",
                        target: Some(StatusClickTarget::Execute(Action::NextTab)),
                    });
                }
                list.extend([
                    CandidateChip {
                        key: help_key,
                        label: "Help",
                        target: Some(StatusClickTarget::Execute(Action::ShowHelp)),
                    },
                    CandidateChip {
                        key: close_key,
                        label: "Close",
                        target: Some(StatusClickTarget::Execute(Action::CloseSurface)),
                    },
                    CandidateChip {
                        key: bar_key,
                        label: "Bar",
                        target: Some(StatusClickTarget::Execute(Action::ToggleStatusBar)),
                    },
                ]);
                list
            }
        }
    }
}

fn combined_split_shortcut(binds: &[Keybind]) -> String {
    let right = shortcut_for_action(binds, &Action::NewSplit(SplitDirection::Right))
        .unwrap_or_else(|| "Ctrl+Shift+D".to_string());
    let down = shortcut_for_action(binds, &Action::NewSplit(SplitDirection::Down))
        .unwrap_or_else(|| "Ctrl+Shift+E".to_string());

    if let (Some((p1, k1)), Some((p2, k2))) = (right.rsplit_once('+'), down.rsplit_once('+'))
        && p1 == p2
        && k1 != k2
    {
        return format!("{p1}+{k1}/{k2}");
    }
    right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_bar_computes_nvim_mode_and_context_menu_shortcut_without_overlap() {
        let cfg = Config::default();
        let ctx = StatusBarContext {
            active_pane_index: 0,
            num_panes: 1,
            num_tabs: 1,
            display_offset: 0,
            tui_mode: false,
            context_menu: None,
            help_dialog: None,
            available_update: None,
            metrics: None,
        };
        let geom = StatusBarGeometry::compute(&cfg, &ctx, 9.0, 20.0, 1010, 660, 1.0)
            .expect("status bar visible by default");
        assert_eq!(geom.mode, StatusMode::Normal);
        assert!(
            geom.chips
                .iter()
                .any(|c| c.key_text == "Ctrl+Shift+P" && c.label_text == "Menu"),
            "expected Context Menu shortcut in bottom bar, got: {:?}",
            geom.chips
                .iter()
                .map(|c| (&c.key_text, c.label_text))
                .collect::<Vec<_>>()
        );
        assert!(geom.theme_pill.is_some());

        // Verify chips never collide with the right theme pill
        let right_edge = geom.theme_pill.as_ref().unwrap().rect.x;
        for chip in &geom.chips {
            assert!(chip.hit_rect.x + chip.hit_rect.w <= right_edge + 2.0);
        }

        // Verify clicking the Menu chip dispatches ToggleContextMenu
        let menu_chip = geom
            .chips
            .iter()
            .find(|c| c.label_text == "Menu")
            .expect("Menu chip");
        assert_eq!(
            geom.click_at(menu_chip.key_rect.x + 4.0, menu_chip.key_rect.y + 4.0),
            Some(StatusClickTarget::Execute(Action::ToggleContextMenu))
        );
    }

    #[test]
    fn status_bar_switches_to_menu_and_split_modes_contextually() {
        let cfg = Config::default();
        let mut menu = ContextMenu::build(120.0, 120.0, false, 2, true, &cfg.keybinds, None);
        menu.select_first_enabled();

        let menu_ctx = StatusBarContext {
            active_pane_index: 1,
            num_panes: 2,
            num_tabs: 1,
            display_offset: 0,
            tui_mode: false,
            context_menu: Some(&menu),
            help_dialog: None,
            available_update: None,
            metrics: None,
        };
        let menu_geom =
            StatusBarGeometry::compute(&cfg, &menu_ctx, 9.0, 20.0, 1010, 660, 1.0).unwrap();
        assert_eq!(menu_geom.mode, StatusMode::Menu);
        assert!(menu_geom.chips.iter().any(|c| c.key_text == "j/k"));

        menu.open_theme_submenu("ocean-dark");
        let theme_ctx = StatusBarContext {
            active_pane_index: 1,
            num_panes: 2,
            num_tabs: 1,
            display_offset: 0,
            tui_mode: false,
            context_menu: Some(&menu),
            help_dialog: None,
            available_update: None,
            metrics: None,
        };
        let theme_geom =
            StatusBarGeometry::compute(&cfg, &theme_ctx, 9.0, 20.0, 1010, 660, 1.0).unwrap();
        assert_eq!(theme_geom.mode, StatusMode::Theme);
        assert!(theme_geom.chips.iter().any(|c| c.key_text == "1-0"));
    }

    #[test]
    fn status_bar_renders_and_clicks_update_pill_when_update_available() {
        let cfg = Config::default();
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
            body: "Test notes".to_string(),
        };
        let ctx = StatusBarContext {
            active_pane_index: 0,
            num_panes: 1,
            num_tabs: 1,
            display_offset: 0,
            tui_mode: false,
            context_menu: None,
            help_dialog: None,
            available_update: Some(&update),
            metrics: None,
        };
        let geom = StatusBarGeometry::compute(&cfg, &ctx, 9.0, 20.0, 1010, 660, 1.0).unwrap();
        assert!(geom.update_pill.is_some());
        let pill = geom.update_pill.as_ref().unwrap();
        assert!(pill.text.contains("v1.0.0"));
        assert_eq!(
            geom.click_at(pill.rect.x + 2.0, pill.rect.y + 2.0),
            Some(StatusClickTarget::Execute(Action::TriggerUpdate))
        );
    }

    #[test]
    fn status_bar_renders_resource_pill_when_metrics_available() {
        let cfg = Config::default();
        let mut metrics = crate::metrics::ProcessMetrics::new();
        metrics.compact_label = "0.5% · 38M (178M)".to_string();
        let ctx = StatusBarContext {
            active_pane_index: 0,
            num_panes: 1,
            num_tabs: 1,
            display_offset: 0,
            tui_mode: false,
            context_menu: None,
            help_dialog: None,
            available_update: None,
            metrics: Some(&metrics),
        };
        let geom = StatusBarGeometry::compute(&cfg, &ctx, 9.0, 20.0, 1010, 660, 1.0)
            .expect("status bar visible");
        assert!(geom.resource_pill.is_some());
        let pill = geom.resource_pill.as_ref().unwrap();
        assert!(pill.text.contains("0.5% · 38M (178M)"));
        assert_eq!(
            geom.click_at(pill.rect.x + 2.0, pill.rect.y + 2.0),
            Some(StatusClickTarget::ToggleMetricsDetail)
        );
    }
}
