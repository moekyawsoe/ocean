pub mod atlas;
pub mod box_drawing;

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use alacritty_terminal::term::TermMode;
use glow::HasContext;

use crate::config::theme::Rgb;
use crate::config::{Config, CursorStyle, TabBarMode};
use crate::font::shaper::{CellInput, PositionedGlyph, TextShaper};
use crate::font::{FontStyle, FontSystem};
use crate::help::{HelpDialog, SECTION_TITLES, StyledLine};
use crate::menu::{ContextMenu, MenuEntry, ThemeSubEntry};
use crate::split::{PaneLayout, SplitDivider};
use crate::statusbar::{StatusBarContext, StatusBarGeometry, StatusMode};
use crate::terminal::{RenderCell, TerminalSnapshot};
use atlas::GlyphAtlas;
use box_drawing::{emit_box_char, is_procedural_box_char};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct QuadInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub uv_pos: [f32; 2],
    pub uv_size: [f32; 2],
    pub color: [f32; 4],
    /// 0.0 = solid rect, 1.0 = alpha mask glyph, 2.0 = color RGBA glyph, 3.0 = hollow rect
    pub kind: f32,
}

impl QuadInstance {
    #[inline]
    pub fn solid(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Self {
        Self {
            pos: [x, y],
            size: [w, h],
            uv_pos: [0.0, 0.0],
            uv_size: [0.0, 0.0],
            color,
            kind: 0.0,
        }
    }

    #[inline]
    pub fn hollow(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Self {
        Self {
            pos: [x, y],
            size: [w, h],
            uv_pos: [0.0, 0.0],
            uv_size: [0.0, 0.0],
            color,
            kind: 3.0,
        }
    }
}

pub struct TabTitle<'a> {
    pub title: &'a str,
    pub active: bool,
}

pub struct PaneScene<'a> {
    pub snapshot: &'a TerminalSnapshot,
    pub layout: PaneLayout,
    pub is_active: bool,
    pub ime_preedit: Option<&'a str>,
}

pub struct RenderEngine {
    pub atlas: GlyphAtlas,
    pub shaper: TextShaper,
    bg_instances: Vec<QuadInstance>,
    fg_instances: Vec<QuadInstance>,
    overlay_instances: Vec<QuadInstance>,
    gpu_buffer_staging: Vec<QuadInstance>,
    shaped_row_buf: Vec<PositionedGlyph>,
}

impl Default for RenderEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderEngine {
    pub fn new() -> Self {
        Self {
            atlas: GlyphAtlas::new(1024),
            shaper: TextShaper::new(),
            bg_instances: Vec::with_capacity(2048),
            fg_instances: Vec::with_capacity(8192),
            overlay_instances: Vec::with_capacity(512),
            gpu_buffer_staging: Vec::with_capacity(10240),
            shaped_row_buf: Vec::with_capacity(256),
        }
    }

    pub fn invalidate_caches(&mut self) {
        self.atlas.clear();
        self.shaper.clear_cache();
    }

    pub fn tab_bar_height(config: &Config, font_sys: &FontSystem, num_tabs: usize) -> f32 {
        let show = match config.window_show_tab_bar {
            TabBarMode::Always => true,
            TabBarMode::Auto => num_tabs > 1,
            TabBarMode::Never => false,
        };
        if show {
            (font_sys.metrics.cell_height + 6.0).round()
        } else {
            0.0
        }
    }

    pub fn status_bar_height(config: &Config, font_sys: &FontSystem, scale_factor: f64) -> f32 {
        if config.window_show_status_bar {
            (font_sys.metrics.cell_height + (6.0 * scale_factor as f32).round()).round()
        } else {
            0.0
        }
    }

    /// Returns `(tab_width, plus_btn_x, plus_btn_w)` when tab bar is visible.
    pub fn tab_bar_metrics(win_w: f32, tab_h: f32, num_tabs: usize) -> (f32, f32, f32) {
        let n = num_tabs.max(1) as f32;
        let plus_w = tab_h.max(24.0);
        let avail_for_tabs = (win_w - plus_w - 4.0).max(48.0);
        let tab_w = (avail_for_tabs / n).min(220.0).floor().max(40.0);
        let plus_x = (n * tab_w + 2.0).round();
        (tab_w, plus_x, plus_w)
    }

    /// Compute `(origin_x, origin_y, cols, rows)` in physical pixels for a single-pane terminal grid.
    pub fn compute_grid_geometry(
        config: &Config,
        font_sys: &FontSystem,
        win_w: u32,
        win_h: u32,
        scale_factor: f64,
        num_tabs: usize,
    ) -> (f32, f32, u16, u16) {
        let sf = scale_factor as f32;
        let pad_l = config.window_padding_x.0 as f32 * sf;
        let pad_r = config.window_padding_x.1 as f32 * sf;
        let pad_t = config.window_padding_y.0 as f32 * sf
            + Self::tab_bar_height(config, font_sys, num_tabs);
        let pad_b = config.window_padding_y.1 as f32 * sf
            + Self::status_bar_height(config, font_sys, scale_factor);

        let cw = font_sys.metrics.cell_width.max(1.0);
        let ch = font_sys.metrics.cell_height.max(1.0);

        let avail_w = (win_w as f32 - pad_l - pad_r).max(cw);
        let avail_h = (win_h as f32 - pad_t - pad_b).max(ch);

        let cols = (avail_w / cw).floor().clamp(2.0, 1000.0) as u16;
        let rows = (avail_h / ch).floor().clamp(1.0, 500.0) as u16;

        let (ox, oy) = if config.window_padding_balance {
            let rem_w = (avail_w - cols as f32 * cw).max(0.0);
            let rem_h = (avail_h - rows as f32 * ch).max(0.0);
            ((pad_l + rem_w * 0.5).floor(), (pad_t + rem_h * 0.5).floor())
        } else {
            (pad_l.floor(), pad_t.floor())
        };

        (ox, oy, cols, rows)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_scene(
        &mut self,
        config: &Config,
        font_sys: &mut FontSystem,
        panes: &[PaneScene<'_>],
        dividers: &[SplitDivider],
        win_w: u32,
        win_h: u32,
        scale_factor: f64,
        tabs: &[TabTitle<'_>],
        context_menu: Option<&ContextMenu>,
        help_dialog: Option<&HelpDialog>,
        status_hover_id: Option<usize>,
        cursor_visible_blink: bool,
        window_focused: bool,
    ) -> (&mut GlyphAtlas, &[QuadInstance]) {
        self.bg_instances.clear();
        self.fg_instances.clear();
        self.overlay_instances.clear();

        // Optional minimal tab bar when > 1 tab (or `window-show-tab-bar = always`)
        let tab_h = Self::tab_bar_height(config, font_sys, tabs.len());
        if tab_h > 0.0 && !tabs.is_empty() {
            self.emit_tab_bar(config, font_sys, win_w as f32, tab_h, tabs);
        }

        let status_h = Self::status_bar_height(config, font_sys, scale_factor);

        for pane in panes {
            self.emit_pane(
                config,
                font_sys,
                pane,
                scale_factor,
                cursor_visible_blink,
                window_focused && pane.is_active,
            );
        }

        // Split pane dividers
        if !dividers.is_empty() {
            let div_col = config.colors.foreground.to_f32_rgba(0.20);
            for div in dividers {
                self.fg_instances
                    .push(QuadInstance::solid(div.x, div.y, div.w, div.h, div_col));
            }
        }

        // Right-click / keyboard context menu overlay (drawn above all panes & dividers)
        if let Some(menu) = context_menu {
            self.emit_context_menu(config, font_sys, menu, win_w, win_h, scale_factor, status_h);
        }

        // In-app Help & Documentation modal dialog overlay
        if let Some(help) = help_dialog {
            self.emit_help_dialog(config, font_sys, help, win_w, win_h, scale_factor, status_h);
        }

        // Neovim-style bottom shortcut & mode status bar
        if status_h > 0.0 {
            let active_idx = panes.iter().position(|p| p.is_active).unwrap_or(0);
            let active_snap = panes.get(active_idx).map(|p| p.snapshot);
            let status_ctx = StatusBarContext {
                active_pane_index: active_idx,
                num_panes: panes.len().max(1),
                num_tabs: tabs.len().max(1),
                display_offset: active_snap.map(|s| s.display_offset).unwrap_or(0),
                tui_mode: active_snap.is_some_and(|s| {
                    s.mode
                        .intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE)
                }),
                context_menu,
                help_dialog,
            };
            self.emit_status_bar(
                config,
                font_sys,
                &status_ctx,
                win_w,
                win_h,
                scale_factor,
                status_hover_id,
            );
        }

        self.gpu_buffer_staging.clear();
        self.gpu_buffer_staging.reserve(
            self.bg_instances.len() + self.fg_instances.len() + self.overlay_instances.len(),
        );
        self.gpu_buffer_staging.extend_from_slice(&self.bg_instances);
        self.gpu_buffer_staging.extend_from_slice(&self.fg_instances);
        self.gpu_buffer_staging
            .extend_from_slice(&self.overlay_instances);
        (&mut self.atlas, &self.gpu_buffer_staging)
    }

    fn emit_pane(
        &mut self,
        config: &Config,
        font_sys: &mut FontSystem,
        pane: &PaneScene<'_>,
        scale_factor: f64,
        cursor_visible_blink: bool,
        focused: bool,
    ) {
        let snapshot = pane.snapshot;
        let origin_x = pane.layout.origin_x;
        let origin_y = pane.layout.origin_y;
        let cw = font_sys.metrics.cell_width;
        let ch = font_sys.metrics.cell_height;
        let baseline = font_sys.metrics.baseline;
        let default_bg = config.colors.background;

        let num_cols = snapshot.cols;
        let num_rows = snapshot.rows;
        if num_cols == 0 || num_rows == 0 {
            return;
        }

        // Pass 1 & 2 per row: emit cell backgrounds into `bg_instances` and glyphs/decorations into `fg_instances`
        let mut cell_inputs: Vec<CellInput<'_>> = Vec::with_capacity(num_cols);

        for row in 0..num_rows {
            if (row + 1) * num_cols > snapshot.cells.len() {
                break;
            }
            let row_y = origin_y + row as f32 * ch;
            let row_slice = &snapshot.cells[row * num_cols..(row + 1) * num_cols];

            // Coalesce adjacent identical non-default background runs on this row
            let mut c = 0;
            while c < num_cols {
                let bg = row_slice[c].bg;
                if bg != default_bg {
                    let start = c;
                    c += 1;
                    while c < num_cols && row_slice[c].bg == bg {
                        c += 1;
                    }
                    let rx = origin_x + start as f32 * cw;
                    let rw = (c - start) as f32 * cw;
                    self.bg_instances
                        .push(QuadInstance::solid(rx, row_y, rw, ch, bg.to_f32_rgba(1.0)));
                } else {
                    c += 1;
                }
            }

            // Prepare row for shaping & emit procedural box drawing + underlines/strikeouts
            cell_inputs.clear();
            for (col, rc) in row_slice.iter().enumerate() {
                let is_box = rc.zerowidth.is_empty() && is_procedural_box_char(rc.c);
                let cell_x = origin_x + col as f32 * cw;

                if is_box {
                    let fg = effective_fg(rc, config);
                    emit_box_char(
                        rc.c,
                        cell_x,
                        row_y,
                        cw,
                        ch,
                        fg.to_f32_rgba(rc.fg_alpha),
                        &mut self.fg_instances,
                    );
                }

                if rc.underline && !rc.wide_spacer {
                    let span_w = if rc.wide { cw * 2.0 } else { cw };
                    let fg = effective_fg(rc, config);
                    self.fg_instances.push(QuadInstance::solid(
                        cell_x,
                        row_y + font_sys.metrics.underline_top,
                        span_w,
                        font_sys.metrics.underline_thickness,
                        fg.to_f32_rgba(rc.fg_alpha),
                    ));
                }

                if rc.strikeout && !rc.wide_spacer {
                    let span_w = if rc.wide { cw * 2.0 } else { cw };
                    let fg = effective_fg(rc, config);
                    self.fg_instances.push(QuadInstance::solid(
                        cell_x,
                        row_y + font_sys.metrics.strikeout_top,
                        span_w,
                        font_sys.metrics.underline_thickness,
                        fg.to_f32_rgba(rc.fg_alpha),
                    ));
                }

                cell_inputs.push(CellInput {
                    c: rc.c,
                    zerowidth: &rc.zerowidth,
                    style: FontStyle::from_flags(rc.bold, rc.italic),
                    is_wide_spacer: rc.wide_spacer,
                    is_box_drawing: is_box,
                });
            }

            self.shaper.shape_row(
                font_sys,
                &cell_inputs,
                config.myanmar_zawgyi,
                &mut self.shaped_row_buf,
            );

            let baseline_y = row_y + baseline;
            for pg in &self.shaped_row_buf {
                let font_px = if pg.is_myanmar {
                    font_sys.metrics.myanmar_font_px
                } else {
                    font_sys.metrics.font_px
                };
                let entry = self.atlas.get_or_rasterize(
                    font_sys,
                    pg.font_id,
                    pg.glyph_id,
                    font_px,
                    config.font_thicken,
                );
                if entry.width == 0 || entry.height == 0 {
                    continue;
                }

                let rc = &row_slice[pg.col.min(num_cols.saturating_sub(1))];
                // Invert glyph color if sitting directly under an opaque Block cursor
                let under_block_cursor = snapshot.cursor_visible
                    && cursor_visible_blink
                    && focused
                    && snapshot.display_offset == 0
                    && snapshot.cursor_style == CursorStyle::Block
                    && row == snapshot.cursor_row
                    && pg.col == snapshot.cursor_col;

                let fg_rgb = if under_block_cursor {
                    config.colors.cursor_text
                } else {
                    effective_fg(rc, config)
                };

                let gx = (origin_x + pg.x + entry.left as f32 * pg.x_scale).round();
                let gy = (baseline_y - pg.y_offset - entry.top as f32).round();
                let gw = (entry.width as f32 * pg.x_scale).max(1.0);
                let gh = entry.height as f32;

                self.fg_instances.push(QuadInstance {
                    pos: [gx, gy],
                    size: [gw, gh],
                    uv_pos: [entry.uv_x, entry.uv_y],
                    uv_size: [entry.uv_w, entry.uv_h],
                    color: if entry.is_color {
                        [1.0, 1.0, 1.0, rc.fg_alpha]
                    } else {
                        fg_rgb.to_f32_rgba(rc.fg_alpha)
                    },
                    kind: if entry.is_color { 2.0 } else { 1.0 },
                });
            }
        }

        // Cursor rendering (when at bottom of scrollback & visible)
        if snapshot.cursor_visible
            && cursor_visible_blink
            && snapshot.display_offset == 0
            && snapshot.cursor_row < num_rows
            && snapshot.cursor_col < num_cols
        {
            let cx = origin_x + snapshot.cursor_col as f32 * cw;
            let cy = origin_y + snapshot.cursor_row as f32 * ch;
            let cur_col = config.colors.cursor_color.to_f32_rgba(config.cursor_opacity);
            let cur_w = if snapshot.cells[snapshot.cursor_row * num_cols + snapshot.cursor_col].wide
            {
                cw * 2.0
            } else {
                cw
            };
            let thick = (config.adjust_cursor_thickness as f32 * scale_factor as f32)
                .round()
                .max(1.0);

            let effective_style = if !focused {
                CursorStyle::BlockHollow
            } else {
                snapshot.cursor_style
            };

            match effective_style {
                CursorStyle::Block => {
                    self.bg_instances
                        .push(QuadInstance::solid(cx, cy, cur_w, ch, cur_col));
                }
                CursorStyle::Bar => {
                    self.fg_instances
                        .push(QuadInstance::solid(cx, cy, thick, ch, cur_col));
                }
                CursorStyle::Underline => {
                    self.fg_instances
                        .push(QuadInstance::solid(cx, cy + ch - thick, cur_w, thick, cur_col));
                }
                CursorStyle::BlockHollow => {
                    self.fg_instances
                        .push(QuadInstance::hollow(cx, cy, cur_w, ch, cur_col));
                }
            }
        }

        // IME Preedit Overlay (for Burmese IBus / Fcitx5 / Keyman live composition!)
        if let Some(preedit) = pane.ime_preedit
            && !preedit.is_empty()
            && snapshot.cursor_row < num_rows
        {
            self.emit_ime_preedit(
                config,
                font_sys,
                preedit,
                origin_x + snapshot.cursor_col as f32 * cw,
                origin_y + snapshot.cursor_row as f32 * ch,
            );
        }

        // Subtle minimal scrollback thumb when scrolled up in history
        if snapshot.display_offset > 0 && snapshot.history_size > 0 {
            let track_h = num_rows as f32 * ch;
            let total_lines = (snapshot.history_size + num_rows) as f32;
            let thumb_h = ((num_rows as f32 / total_lines) * track_h).clamp(16.0, track_h);
            let ratio =
                1.0 - (snapshot.display_offset as f32 / snapshot.history_size.max(1) as f32);
            let thumb_y = origin_y + ratio * (track_h - thumb_h);
            let thumb_w = (4.0 * scale_factor as f32).round().max(3.0);
            let thumb_x = pane.layout.bounds_x + pane.layout.bounds_w - thumb_w - 2.0;
            self.fg_instances.push(QuadInstance::solid(
                thumb_x,
                thumb_y,
                thumb_w,
                thumb_h,
                config.colors.foreground.to_f32_rgba(0.32),
            ));
        }
    }

    fn emit_tab_bar(
        &mut self,
        config: &Config,
        font_sys: &mut FontSystem,
        win_w: f32,
        tab_h: f32,
        tabs: &[TabTitle<'_>],
    ) {
        let bg = config.colors.background;
        let is_dark = bg.relative_luminance() < 0.35;
        let bar_bg = if is_dark {
            Rgb::new(
                bg.r.saturating_sub(6),
                bg.g.saturating_sub(6),
                bg.b.saturating_sub(8),
            )
        } else {
            Rgb::new(
                bg.r.saturating_sub(14),
                bg.g.saturating_sub(13),
                bg.b.saturating_sub(11),
            )
        };
        self.bg_instances
            .push(QuadInstance::solid(0.0, 0.0, win_w, tab_h, bar_bg.to_f32_rgba(0.98)));
        // Bottom 1px divider
        self.fg_instances.push(QuadInstance::solid(
            0.0,
            tab_h - 1.0,
            win_w,
            1.0,
            config.colors.foreground.to_f32_rgba(0.12),
        ));

        let (tab_w, plus_x, plus_w) = Self::tab_bar_metrics(win_w, tab_h, tabs.len());
        let cw = font_sys.metrics.cell_width;

        for (idx, tab) in tabs.iter().enumerate() {
            let tx = idx as f32 * tab_w;
            if tab.active {
                self.bg_instances.push(QuadInstance::solid(
                    tx,
                    0.0,
                    tab_w,
                    tab_h - 1.0,
                    bg.to_f32_rgba(1.0),
                ));
                self.fg_instances.push(QuadInstance::solid(
                    tx,
                    tab_h - 2.0,
                    tab_w,
                    2.0,
                    config.colors.cursor_color.to_f32_rgba(0.95),
                ));
            } else if idx > 0 {
                // Subtle separator between inactive tabs
                self.fg_instances.push(QuadInstance::solid(
                    tx,
                    5.0,
                    1.0,
                    (tab_h - 10.0).max(4.0),
                    config.colors.foreground.to_f32_rgba(0.10),
                ));
            }

            let label = format!(" {}: {} ", idx + 1, tab.title);
            let max_chars = ((tab_w - 12.0) / cw).floor().max(4.0) as usize;
            let text_alpha = if tab.active { 0.95 } else { 0.55 };
            let fg_col = config.colors.foreground.to_f32_rgba(text_alpha);
            let baseline_y = 3.0 + font_sys.metrics.baseline;

            Self::emit_shaped_label(
                &mut self.atlas,
                &mut self.shaper,
                &mut self.fg_instances,
                config,
                font_sys,
                &label,
                max_chars,
                tx + 6.0,
                baseline_y,
                fg_col,
                tab.active,
            );
        }

        // Crisp `+` New Tab button right after the tabs
        if plus_x + plus_w <= win_w {
            let cx = (plus_x + plus_w * 0.5).floor();
            let cy = ((tab_h - 1.0) * 0.5).floor();
            let arm = (tab_h * 0.22).round().clamp(4.0, 7.0);
            let plus_col = config.colors.foreground.to_f32_rgba(0.62);
            self.fg_instances.push(QuadInstance::solid(
                cx - arm,
                cy,
                arm * 2.0 + 1.0,
                1.5,
                plus_col,
            ));
            self.fg_instances.push(QuadInstance::solid(
                cx,
                cy - arm,
                1.5,
                arm * 2.0 + 1.0,
                plus_col,
            ));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_context_menu(
        &mut self,
        config: &Config,
        font_sys: &mut FontSystem,
        menu: &ContextMenu,
        win_w: u32,
        win_h: u32,
        scale_factor: f64,
        status_h: f32,
    ) {
        let sf = scale_factor as f32;
        let cw = font_sys.metrics.cell_width;
        let ch = font_sys.metrics.cell_height;
        let geom = menu.compute_geometry(cw, ch, win_w, win_h, scale_factor, status_h);

        let bg = config.colors.background;
        let fg = config.colors.foreground;
        let is_dark = bg.relative_luminance() < 0.35;
        let menu_bg = if is_dark {
            Rgb::new(
                bg.r.saturating_add(16),
                bg.g.saturating_add(20),
                bg.b.saturating_add(26),
            )
        } else {
            Rgb::new(
                bg.r.saturating_add(8),
                bg.g.saturating_add(8),
                bg.b.saturating_add(8),
            )
        };
        let accent = config.colors.cursor_color.ensure_contrast(menu_bg, 3.2);

        // Layered soft drop shadow for main menu
        self.overlay_instances.push(QuadInstance::solid(
            geom.x + 3.0,
            geom.y + 5.0,
            geom.w,
            geom.h,
            [0.0, 0.0, 0.0, if is_dark { 0.38 } else { 0.16 }],
        ));
        self.overlay_instances.push(QuadInstance::solid(
            geom.x + 1.0,
            geom.y + 2.0,
            geom.w,
            geom.h,
            [0.0, 0.0, 0.0, if is_dark { 0.24 } else { 0.10 }],
        ));

        // Opaque menu surface
        self.overlay_instances.push(QuadInstance::solid(
            geom.x,
            geom.y,
            geom.w,
            geom.h,
            menu_bg.to_f32_rgba(0.99),
        ));

        // Separators & hovered item highlight
        let accent_w = (2.5 * sf).round().max(2.0);
        for row in &geom.rows {
            if row.is_separator {
                let sy = (row.y + row.h * 0.5).floor();
                self.overlay_instances.push(QuadInstance::solid(
                    geom.x + 8.0,
                    sy,
                    (geom.w - 16.0).max(1.0),
                    1.0,
                    fg.to_f32_rgba(0.14),
                ));
            } else {
                let is_sub_parent =
                    menu.submenu_open && matches!(menu.entries[row.entry_index], MenuEntry::ThemeSubmenu { .. });
                if menu.hovered == Some(row.entry_index) || is_sub_parent {
                    self.overlay_instances.push(QuadInstance::solid(
                        geom.x + 4.0,
                        row.y + 1.0,
                        (geom.w - 8.0).max(1.0),
                        (row.h - 2.0).max(1.0),
                        config.colors.selection_background.to_f32_rgba(0.95),
                    ));
                    self.overlay_instances.push(QuadInstance::solid(
                        geom.x + 4.0,
                        row.y + 3.0,
                        accent_w,
                        (row.h - 6.0).max(2.0),
                        accent.to_f32_rgba(1.0),
                    ));
                }
            }
        }

        // Crisp 1.5px border
        self.overlay_instances.push(QuadInstance::hollow(
            geom.x,
            geom.y,
            geom.w,
            geom.h,
            fg.to_f32_rgba(0.24),
        ));

        // Main menu item labels, shortcuts, and submenu chevron
        for row in &geom.rows {
            if row.is_separator {
                continue;
            }
            let text_top = row.y + ((row.h - ch) * 0.5).round();
            let baseline_y = text_top + font_sys.metrics.baseline;

            match &menu.entries[row.entry_index] {
                MenuEntry::Item(item) => {
                    let is_hovered = menu.hovered == Some(row.entry_index) && item.enabled;
                    let label_col = if !item.enabled {
                        fg.to_f32_rgba(0.36)
                    } else if is_hovered {
                        config
                            .colors
                            .selection_foreground
                            .unwrap_or(fg)
                            .to_f32_rgba(1.0)
                    } else {
                        fg.to_f32_rgba(0.94)
                    };

                    Self::emit_shaped_label(
                        &mut self.atlas,
                        &mut self.shaper,
                        &mut self.overlay_instances,
                        config,
                        font_sys,
                        item.label,
                        64,
                        geom.x + geom.pad_x,
                        baseline_y,
                        label_col,
                        is_hovered,
                    );

                    if !item.shortcut.is_empty() {
                        let sc_w = item.shortcut.chars().count() as f32 * cw;
                        let sc_x = (geom.x + geom.w - geom.pad_x - sc_w).round();
                        let sc_col = fg.to_f32_rgba(if !item.enabled {
                            0.22
                        } else if is_hovered {
                            0.74
                        } else {
                            0.48
                        });
                        Self::emit_shaped_label(
                            &mut self.atlas,
                            &mut self.shaper,
                            &mut self.overlay_instances,
                            config,
                            font_sys,
                            &item.shortcut,
                            32,
                            sc_x,
                            baseline_y,
                            sc_col,
                            false,
                        );
                    }
                }
                MenuEntry::ThemeSubmenu { label } => {
                    let is_hovered = menu.hovered == Some(row.entry_index) || menu.submenu_open;
                    let label_col = if is_hovered {
                        config
                            .colors
                            .selection_foreground
                            .unwrap_or(fg)
                            .to_f32_rgba(1.0)
                    } else {
                        fg.to_f32_rgba(0.94)
                    };

                    Self::emit_shaped_label(
                        &mut self.atlas,
                        &mut self.shaper,
                        &mut self.overlay_instances,
                        config,
                        font_sys,
                        label,
                        64,
                        geom.x + geom.pad_x,
                        baseline_y,
                        label_col,
                        is_hovered,
                    );

                    // Procedural right-pointing triangle arrow (font-independent)
                    let arrow_col = if is_hovered {
                        accent.to_f32_rgba(1.0)
                    } else {
                        fg.to_f32_rgba(0.60)
                    };
                    let ax = (geom.x + geom.w - geom.pad_x - 6.0 * sf).round();
                    let ay = (row.y + row.h * 0.5).round();
                    let step = (1.5 * sf).max(1.0);
                    for i in 0..4 {
                        let half = (4 - i) as f32 * sf;
                        self.overlay_instances.push(QuadInstance::solid(
                            ax + i as f32 * step,
                            (ay - half).round(),
                            step,
                            (half * 2.0 + 1.0).round(),
                            arrow_col,
                        ));
                    }
                }
                MenuEntry::Separator => {}
            }
        }

        // Render Switch Theme flyout submenu (5 Dark + 5 Light themes)
        if let Some(sub) = &geom.submenu {
            self.overlay_instances.push(QuadInstance::solid(
                sub.x + 3.0,
                sub.y + 5.0,
                sub.w,
                sub.h,
                [0.0, 0.0, 0.0, if is_dark { 0.40 } else { 0.16 }],
            ));
            self.overlay_instances.push(QuadInstance::solid(
                sub.x + 1.0,
                sub.y + 2.0,
                sub.w,
                sub.h,
                [0.0, 0.0, 0.0, if is_dark { 0.25 } else { 0.10 }],
            ));
            self.overlay_instances.push(QuadInstance::solid(
                sub.x,
                sub.y,
                sub.w,
                sub.h,
                menu_bg.to_f32_rgba(0.99),
            ));

            // Row highlights & separators inside submenu
            for s_row in &sub.rows {
                if s_row.is_separator {
                    let sy = (s_row.y + s_row.h * 0.5).floor();
                    self.overlay_instances.push(QuadInstance::solid(
                        sub.x + 8.0,
                        sy,
                        (sub.w - 16.0).max(1.0),
                        1.0,
                        fg.to_f32_rgba(0.14),
                    ));
                } else if let ThemeSubEntry::Theme(info) = &menu.sub_entries[s_row.entry_index] {
                    let is_hov = menu.sub_hovered == Some(s_row.entry_index);
                    let is_active = info.id == config.theme_name;
                    if is_hov {
                        self.overlay_instances.push(QuadInstance::solid(
                            sub.x + 4.0,
                            s_row.y + 1.0,
                            (sub.w - 8.0).max(1.0),
                            (s_row.h - 2.0).max(1.0),
                            config.colors.selection_background.to_f32_rgba(0.95),
                        ));
                        self.overlay_instances.push(QuadInstance::solid(
                            sub.x + 4.0,
                            s_row.y + 3.0,
                            accent_w,
                            (s_row.h - 6.0).max(2.0),
                            accent.to_f32_rgba(1.0),
                        ));
                    } else if is_active {
                        self.overlay_instances.push(QuadInstance::solid(
                            sub.x + 4.0,
                            s_row.y + 1.0,
                            (sub.w - 8.0).max(1.0),
                            (s_row.h - 2.0).max(1.0),
                            accent.to_f32_rgba(0.14),
                        ));
                    }
                }
            }

            // Submenu border
            self.overlay_instances.push(QuadInstance::hollow(
                sub.x,
                sub.y,
                sub.w,
                sub.h,
                fg.to_f32_rgba(0.24),
            ));

            let sw_sz = (13.0 * sf).round().max(10.0);
            let hdr_col = accent.ensure_contrast(menu_bg, 3.8).to_f32_rgba(0.88);

            for s_row in &sub.rows {
                if s_row.is_separator {
                    continue;
                }
                let text_top = s_row.y + ((s_row.h - ch) * 0.5).round();
                let baseline_y = text_top + font_sys.metrics.baseline;

                match &menu.sub_entries[s_row.entry_index] {
                    ThemeSubEntry::Header(hdr) => {
                        Self::emit_shaped_label(
                            &mut self.atlas,
                            &mut self.shaper,
                            &mut self.overlay_instances,
                            config,
                            font_sys,
                            hdr,
                            32,
                            sub.x + sub.pad_x,
                            baseline_y,
                            hdr_col,
                            true,
                        );
                    }
                    ThemeSubEntry::Theme(info) => {
                        let is_hov = menu.sub_hovered == Some(s_row.entry_index);
                        let is_active = info.id == config.theme_name;

                        // Two-tone color swatch (background + accent + crisp frame)
                        let sw_x = sub.x + sub.pad_x;
                        let sw_y = (s_row.y + (s_row.h - sw_sz) * 0.5).round();
                        self.overlay_instances.push(QuadInstance::solid(
                            sw_x,
                            sw_y,
                            sw_sz,
                            sw_sz,
                            info.bg_preview.to_f32_rgba(1.0),
                        ));
                        let inner = (sw_sz * 0.42).round().max(4.0);
                        self.overlay_instances.push(QuadInstance::solid(
                            (sw_x + sw_sz - inner - 2.0).round(),
                            (sw_y + 2.0).round(),
                            inner,
                            (sw_sz - 4.0).max(4.0),
                            info.accent_preview.to_f32_rgba(1.0),
                        ));
                        self.overlay_instances.push(QuadInstance::hollow(
                            sw_x,
                            sw_y,
                            sw_sz,
                            sw_sz,
                            fg.to_f32_rgba(0.32),
                        ));

                        let label_x = (sw_x + sw_sz + 8.0 * sf).round();
                        let label_col = if is_hov {
                            config
                                .colors
                                .selection_foreground
                                .unwrap_or(fg)
                                .to_f32_rgba(1.0)
                        } else if is_active {
                            fg.to_f32_rgba(1.0)
                        } else {
                            fg.to_f32_rgba(0.92)
                        };

                        Self::emit_shaped_label(
                            &mut self.atlas,
                            &mut self.shaper,
                            &mut self.overlay_instances,
                            config,
                            font_sys,
                            info.label,
                            24,
                            label_x,
                            baseline_y,
                            label_col,
                            is_hov || is_active,
                        );

                        // Active theme indicator dot on the right
                        if is_active {
                            let dot = (6.0 * sf).round().max(5.0);
                            let dx = (sub.x + sub.w - sub.pad_x - dot).round();
                            let dy = (s_row.y + (s_row.h - dot) * 0.5).round();
                            self.overlay_instances.push(QuadInstance::solid(
                                dx,
                                dy,
                                dot,
                                dot,
                                accent.to_f32_rgba(1.0),
                            ));
                        }
                    }
                    ThemeSubEntry::Separator => {}
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_help_dialog(
        &mut self,
        config: &Config,
        font_sys: &mut FontSystem,
        help: &HelpDialog,
        win_w: u32,
        win_h: u32,
        scale_factor: f64,
        status_h: f32,
    ) {
        let sf = scale_factor as f32;
        let cw = font_sys.metrics.cell_width;
        let ch = font_sys.metrics.cell_height;
        let geom = help.compute_geometry(cw, ch, win_w, win_h, scale_factor, status_h);
        let lines = help.wrapped_lines(geom.max_cols);
        let (scroll_line, max_scroll) = help.clamped_scroll(lines.len(), geom.visible_rows);

        let bg = config.colors.background;
        let fg = config.colors.foreground;
        let is_dark = bg.relative_luminance() < 0.35;

        let card_bg = if is_dark {
            Rgb::new(
                bg.r.saturating_add(14),
                bg.g.saturating_add(18),
                bg.b.saturating_add(24),
            )
        } else {
            Rgb::new(
                bg.r.saturating_add(8),
                bg.g.saturating_add(8),
                bg.b.saturating_add(8),
            )
        };
        let chrome_bg = if is_dark {
            Rgb::new(
                bg.r.saturating_add(6),
                bg.g.saturating_add(8),
                bg.b.saturating_add(12),
            )
        } else {
            Rgb::new(
                bg.r.saturating_sub(12),
                bg.g.saturating_sub(11),
                bg.b.saturating_sub(9),
            )
        };
        let code_bg = if is_dark {
            Rgb::new(
                bg.r.saturating_add(4),
                bg.g.saturating_add(6),
                bg.b.saturating_add(9),
            )
        } else {
            Rgb::new(
                bg.r.saturating_sub(16),
                bg.g.saturating_sub(15),
                bg.b.saturating_sub(12),
            )
        };

        let accent = config.colors.cursor_color.ensure_contrast(card_bg, 3.8);
        let key_fg = config.colors.ansi[4].ensure_contrast(card_bg, 4.3);
        let code_fg = config.colors.ansi[2].ensure_contrast(code_bg, 4.3);

        // 1. Translucent window backdrop dimmer (leaves bottom shortcut bar crisp)
        self.overlay_instances.push(QuadInstance::solid(
            0.0,
            0.0,
            win_w as f32,
            (win_h as f32 - status_h).max(1.0),
            [0.0, 0.0, 0.0, if is_dark { 0.56 } else { 0.34 }],
        ));

        // 2. Layered modal drop shadow
        let m = geom.modal;
        self.overlay_instances.push(QuadInstance::solid(
            m.x + 4.0,
            m.y + 8.0,
            m.w,
            m.h,
            [0.0, 0.0, 0.0, if is_dark { 0.42 } else { 0.18 }],
        ));
        self.overlay_instances.push(QuadInstance::solid(
            m.x + 2.0,
            m.y + 3.0,
            m.w,
            m.h,
            [0.0, 0.0, 0.0, if is_dark { 0.26 } else { 0.10 }],
        ));

        // 3. Modal card surface + header/tab strip + footer strip
        self.overlay_instances
            .push(QuadInstance::solid(m.x, m.y, m.w, m.h, card_bg.to_f32_rgba(0.995)));
        self.overlay_instances.push(QuadInstance::solid(
            m.x,
            m.y,
            m.w,
            geom.header_h + geom.tabs_h,
            chrome_bg.to_f32_rgba(0.995),
        ));
        self.overlay_instances.push(QuadInstance::solid(
            m.x,
            geom.footer_y,
            m.w,
            geom.footer_h,
            chrome_bg.to_f32_rgba(0.995),
        ));

        // 4. Close [X] button background
        let cb = geom.close_btn;
        if help.close_hovered {
            self.overlay_instances.push(QuadInstance::solid(
                cb.x,
                cb.y,
                cb.w,
                cb.h,
                config.colors.ansi[1].to_f32_rgba(0.85),
            ));
        } else {
            self.overlay_instances.push(QuadInstance::hollow(
                cb.x,
                cb.y,
                cb.w,
                cb.h,
                fg.to_f32_rgba(0.20),
            ));
        }

        // 5. Section tab backgrounds & active tab underline
        for tab in &geom.tabs {
            let r = tab.rect;
            if tab.index == help.active_tab {
                self.overlay_instances
                    .push(QuadInstance::solid(r.x, r.y, r.w, r.h, card_bg.to_f32_rgba(1.0)));
                self.overlay_instances.push(QuadInstance::solid(
                    r.x,
                    r.y,
                    r.w,
                    (2.5 * sf).round().max(2.0),
                    accent.to_f32_rgba(1.0),
                ));
            } else if help.hovered_tab == Some(tab.index) {
                self.overlay_instances.push(QuadInstance::solid(
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    fg.to_f32_rgba(0.08),
                ));
            }
        }

        // Horizontal dividers below tabs & above footer
        self.overlay_instances.push(QuadInstance::solid(
            m.x,
            geom.tabs_y + geom.tabs_h - 1.0,
            m.w,
            1.0,
            fg.to_f32_rgba(0.14),
        ));
        self.overlay_instances.push(QuadInstance::solid(
            m.x,
            geom.footer_y,
            m.w,
            1.0,
            fg.to_f32_rgba(0.14),
        ));

        // 6. Code block background strips (emitted before glyphs)
        let end_line = (scroll_line + geom.visible_rows).min(lines.len());
        for (row_idx, line) in lines[scroll_line..end_line].iter().enumerate() {
            if matches!(line, StyledLine::Code(_)) {
                let ry = geom.body_y + row_idx as f32 * geom.line_h;
                self.overlay_instances.push(QuadInstance::solid(
                    geom.body_x - 4.0,
                    ry,
                    geom.body_w + 8.0,
                    geom.line_h,
                    code_bg.to_f32_rgba(0.98),
                ));
                self.overlay_instances.push(QuadInstance::solid(
                    geom.body_x - 4.0,
                    ry + 1.0,
                    (2.5 * sf).round().max(2.0),
                    (geom.line_h - 2.0).max(2.0),
                    accent.to_f32_rgba(0.85),
                ));
            }
        }

        // 7. Scrollbar track & thumb when content exceeds visible body height
        if max_scroll > 0 {
            let sb_w = (4.0 * sf).round().max(3.0);
            let sb_x = (m.x + m.w - sb_w - 6.0 * sf).round();
            let track_h = geom.body_h;
            self.overlay_instances.push(QuadInstance::solid(
                sb_x,
                geom.body_y,
                sb_w,
                track_h,
                fg.to_f32_rgba(0.08),
            ));
            let thumb_h = ((geom.visible_rows as f32 / lines.len() as f32) * track_h)
                .round()
                .clamp(18.0 * sf, track_h);
            let ratio = scroll_line as f32 / max_scroll as f32;
            let thumb_y = (geom.body_y + ratio * (track_h - thumb_h)).round();
            self.overlay_instances.push(QuadInstance::solid(
                sb_x,
                thumb_y,
                sb_w,
                thumb_h,
                accent.to_f32_rgba(0.75),
            ));
        }

        // 8. Outer crisp modal border
        self.overlay_instances.push(QuadInstance::hollow(
            m.x,
            m.y,
            m.w,
            m.h,
            fg.to_f32_rgba(0.26),
        ));

        // 9. Header title & [X] close label
        let hdr_text_top = m.y + ((geom.header_h - ch) * 0.5).round();
        let hdr_baseline = hdr_text_top + font_sys.metrics.baseline;
        let title_max_chars = ((cb.x - (m.x + 16.0 * sf)) / cw).floor().max(8.0) as usize;
        Self::emit_shaped_label(
            &mut self.atlas,
            &mut self.shaper,
            &mut self.overlay_instances,
            config,
            font_sys,
            "Ocean Terminal — Help & Documentation",
            title_max_chars,
            (m.x + 14.0 * sf).round(),
            hdr_baseline,
            fg.ensure_contrast(chrome_bg, 4.5).to_f32_rgba(1.0),
            true,
        );

        let close_label = "X";
        let close_lx = (cb.x + (cb.w - cw) * 0.5).round();
        let close_ly = cb.y + ((cb.h - ch) * 0.5).round() + font_sys.metrics.baseline;
        let close_fg = if help.close_hovered {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            fg.to_f32_rgba(0.82)
        };
        Self::emit_shaped_label(
            &mut self.atlas,
            &mut self.shaper,
            &mut self.overlay_instances,
            config,
            font_sys,
            close_label,
            3,
            close_lx,
            close_ly,
            close_fg,
            true,
        );

        // 10. Section tab labels
        for tab in &geom.tabs {
            let r = tab.rect;
            let title = SECTION_TITLES[tab.index];
            let max_c = ((r.w - 8.0) / cw).floor().max(3.0) as usize;
            let shown_c = title.chars().count().min(max_c);
            let lx = (r.x + (r.w - shown_c as f32 * cw) * 0.5).round();
            let ly = r.y + ((r.h - ch) * 0.5).round() + font_sys.metrics.baseline;
            let is_act = tab.index == help.active_tab;
            let t_col = if is_act {
                accent.to_f32_rgba(1.0)
            } else if help.hovered_tab == Some(tab.index) {
                fg.to_f32_rgba(0.92)
            } else {
                fg.to_f32_rgba(0.66)
            };
            Self::emit_shaped_label(
                &mut self.atlas,
                &mut self.shaper,
                &mut self.overlay_instances,
                config,
                font_sys,
                title,
                max_c,
                lx,
                ly,
                t_col,
                is_act,
            );
        }

        // 11. Body documentation lines
        let body_fg = fg.ensure_contrast(card_bg, 4.5).to_f32_rgba(0.94);
        let heading_fg = accent.to_f32_rgba(1.0);
        let key_col_rgba = key_fg.to_f32_rgba(1.0);
        let code_col_rgba = code_fg.to_f32_rgba(1.0);

        for (row_idx, line) in lines[scroll_line..end_line].iter().enumerate() {
            let ry = geom.body_y + row_idx as f32 * geom.line_h;
            let baseline_y =
                ry + ((geom.line_h - ch) * 0.5).round() + font_sys.metrics.baseline;

            match line {
                StyledLine::Heading(s) => {
                    Self::emit_shaped_label(
                        &mut self.atlas,
                        &mut self.shaper,
                        &mut self.overlay_instances,
                        config,
                        font_sys,
                        s,
                        geom.max_cols,
                        geom.body_x,
                        baseline_y,
                        heading_fg,
                        true,
                    );
                }
                StyledLine::Body(s) => {
                    Self::emit_shaped_label(
                        &mut self.atlas,
                        &mut self.shaper,
                        &mut self.overlay_instances,
                        config,
                        font_sys,
                        s,
                        geom.max_cols,
                        geom.body_x,
                        baseline_y,
                        body_fg,
                        false,
                    );
                }
                StyledLine::Bullet { first, text } => {
                    if *first {
                        let bsz = (4.5 * sf).round().max(4.0);
                        let bx = (geom.body_x + cw * 0.35).round();
                        let by = (ry + (geom.line_h - bsz) * 0.5).round();
                        self.overlay_instances.push(QuadInstance::solid(
                            bx,
                            by,
                            bsz,
                            bsz,
                            heading_fg,
                        ));
                    }
                    Self::emit_shaped_label(
                        &mut self.atlas,
                        &mut self.shaper,
                        &mut self.overlay_instances,
                        config,
                        font_sys,
                        text,
                        geom.max_cols.saturating_sub(2),
                        (geom.body_x + 2.0 * cw).round(),
                        baseline_y,
                        body_fg,
                        false,
                    );
                }
                StyledLine::KeyRow { key, val, key_cols } => {
                    if !key.is_empty() {
                        Self::emit_shaped_label(
                            &mut self.atlas,
                            &mut self.shaper,
                            &mut self.overlay_instances,
                            config,
                            font_sys,
                            key,
                            *key_cols,
                            geom.body_x,
                            baseline_y,
                            key_col_rgba,
                            true,
                        );
                    }
                    if !val.is_empty() {
                        Self::emit_shaped_label(
                            &mut self.atlas,
                            &mut self.shaper,
                            &mut self.overlay_instances,
                            config,
                            font_sys,
                            val,
                            geom.max_cols.saturating_sub(*key_cols),
                            (geom.body_x + *key_cols as f32 * cw).round(),
                            baseline_y,
                            body_fg,
                            false,
                        );
                    }
                }
                StyledLine::Code(s) => {
                    Self::emit_shaped_label(
                        &mut self.atlas,
                        &mut self.shaper,
                        &mut self.overlay_instances,
                        config,
                        font_sys,
                        s,
                        geom.max_cols,
                        geom.body_x,
                        baseline_y,
                        code_col_rgba,
                        false,
                    );
                }
                StyledLine::Spacer => {}
            }
        }

        // 12. Footer navigation hint + active theme indicator
        let f_baseline =
            geom.footer_y + ((geom.footer_h - ch) * 0.5).round() + font_sys.metrics.baseline;
        let hint = "1-5 / h/l: Section  |  j/k / Space: Scroll  |  g/G: Top/End  |  Esc/q: Close";
        Self::emit_shaped_label(
            &mut self.atlas,
            &mut self.shaper,
            &mut self.overlay_instances,
            config,
            font_sys,
            hint,
            geom.max_cols,
            geom.body_x,
            f_baseline,
            fg.ensure_contrast(chrome_bg, 3.8).to_f32_rgba(0.76),
            false,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_status_bar(
        &mut self,
        config: &Config,
        font_sys: &mut FontSystem,
        ctx: &StatusBarContext<'_>,
        win_w: u32,
        win_h: u32,
        scale_factor: f64,
        status_hover_id: Option<usize>,
    ) {
        let cw = font_sys.metrics.cell_width;
        let ch = font_sys.metrics.cell_height;
        let Some(geom) =
            StatusBarGeometry::compute(config, ctx, cw, ch, win_w, win_h, scale_factor)
        else {
            return;
        };

        let bg = config.colors.background;
        let fg = config.colors.foreground;
        let is_dark = bg.relative_luminance() < 0.35;

        let bar_bg = if is_dark {
            Rgb::new(
                bg.r.saturating_add(9),
                bg.g.saturating_add(13),
                bg.b.saturating_add(19),
            )
        } else {
            Rgb::new(
                bg.r.saturating_sub(14),
                bg.g.saturating_sub(12),
                bg.b.saturating_sub(9),
            )
        };
        let key_bg = if is_dark {
            Rgb::new(
                bar_bg.r.saturating_add(13),
                bar_bg.g.saturating_add(16),
                bar_bg.b.saturating_add(23),
            )
        } else {
            Rgb::new(
                bar_bg.r.saturating_sub(13),
                bar_bg.g.saturating_sub(12),
                bar_bg.b.saturating_sub(10),
            )
        };

        // Base statusline strip + 1px top border
        self.overlay_instances.push(QuadInstance::solid(
            0.0,
            geom.y,
            geom.w,
            geom.h,
            bar_bg.to_f32_rgba(0.99),
        ));
        self.overlay_instances.push(QuadInstance::solid(
            0.0,
            geom.y,
            geom.w,
            1.0,
            fg.to_f32_rgba(0.16),
        ));

        let text_top = geom.y + 1.0 + ((geom.h - 1.0 - ch) * 0.5).round();
        let baseline_y = text_top + font_sys.metrics.baseline;

        // 1. Left Neovim-style mode badge (` OCEAN `, ` TUI `, ` SCROLL `, ` MENU `, ` THEME `, ` HELP `)
        let raw_mode_bg = match geom.mode {
            StatusMode::Normal => config.colors.cursor_color,
            StatusMode::Tui => config.colors.ansi[6],
            StatusMode::Scroll => config.colors.ansi[3],
            StatusMode::Menu => config.colors.ansi[4],
            StatusMode::Theme => config.colors.ansi[5],
            StatusMode::Help => config.colors.ansi[2],
        };
        let mode_bg = raw_mode_bg.ensure_contrast(bar_bg, 2.2);
        let mode_fg = if mode_bg.relative_luminance() > 0.34 {
            Rgb::new(10, 15, 22)
        } else {
            Rgb::new(255, 255, 255)
        };
        let mode_alpha = if status_hover_id == Some(1) { 1.0 } else { 0.94 };
        self.overlay_instances.push(QuadInstance::solid(
            geom.mode_pill.rect.x,
            geom.mode_pill.rect.y,
            geom.mode_pill.rect.w,
            geom.mode_pill.rect.h,
            mode_bg.to_f32_rgba(mode_alpha),
        ));
        Self::emit_shaped_label(
            &mut self.atlas,
            &mut self.shaper,
            &mut self.overlay_instances,
            config,
            font_sys,
            &geom.mode_pill.text,
            16,
            geom.mode_pill.text_x,
            baseline_y,
            mode_fg.to_f32_rgba(1.0),
            true,
        );

        // 2. Optional secondary info segment (` 1/3 ` split index or ` +24L ` scroll offset)
        if let Some(info) = &geom.info_pill {
            let info_hov = status_hover_id == Some(2);
            let info_bg = config.colors.selection_background;
            self.overlay_instances.push(QuadInstance::solid(
                info.rect.x,
                info.rect.y,
                info.rect.w,
                info.rect.h,
                info_bg.to_f32_rgba(if info_hov { 0.98 } else { 0.78 }),
            ));
            let info_fg = fg.ensure_contrast(info_bg, 4.2);
            Self::emit_shaped_label(
                &mut self.atlas,
                &mut self.shaper,
                &mut self.overlay_instances,
                config,
                font_sys,
                &info.text,
                16,
                info.text_x,
                baseline_y,
                info_fg.to_f32_rgba(0.95),
                true,
            );
        }

        // 3. Center/left shortcut chips (`[Ctrl+Shift+P] Menu`, `[Ctrl+Shift+D/E] Split`, etc.)
        let key_accent = if is_dark {
            config.colors.cursor_color.ensure_contrast(key_bg, 4.0)
        } else {
            config.colors.ansi[4].ensure_contrast(key_bg, 4.4)
        };
        let label_fg = fg.ensure_contrast(bar_bg, 4.3);

        for (idx, chip) in geom.chips.iter().enumerate() {
            let is_hov = status_hover_id == Some(100 + idx);
            if is_hov {
                self.overlay_instances.push(QuadInstance::solid(
                    chip.hit_rect.x,
                    chip.hit_rect.y,
                    chip.hit_rect.w,
                    chip.hit_rect.h,
                    config.colors.selection_background.to_f32_rgba(0.45),
                ));
            }

            let pill_col = if is_hov {
                config.colors.selection_background.to_f32_rgba(0.96)
            } else {
                key_bg.to_f32_rgba(0.98)
            };
            self.overlay_instances.push(QuadInstance::solid(
                chip.key_rect.x,
                chip.key_rect.y,
                chip.key_rect.w,
                chip.key_rect.h,
                pill_col,
            ));
            // Crisp 1px keycap bottom edge for tactile depth
            self.overlay_instances.push(QuadInstance::solid(
                chip.key_rect.x,
                chip.key_rect.y + chip.key_rect.h - 1.0,
                chip.key_rect.w,
                1.0,
                fg.to_f32_rgba(if is_dark { 0.20 } else { 0.16 }),
            ));

            Self::emit_shaped_label(
                &mut self.atlas,
                &mut self.shaper,
                &mut self.overlay_instances,
                config,
                font_sys,
                &chip.key_text,
                32,
                chip.key_text_x,
                baseline_y,
                key_accent.to_f32_rgba(1.0),
                true,
            );

            Self::emit_shaped_label(
                &mut self.atlas,
                &mut self.shaper,
                &mut self.overlay_instances,
                config,
                font_sys,
                chip.label_text,
                24,
                chip.label_x,
                baseline_y,
                label_fg.to_f32_rgba(if is_hov { 1.0 } else { 0.85 }),
                is_hov,
            );
        }

        // 4. Optional legacy Zawgyi mode indicator pill (` ZG `)
        if let Some(zg) = &geom.zawgyi_pill {
            let zg_bg = config.colors.ansi[3].ensure_contrast(bar_bg, 2.2);
            let zg_fg = if zg_bg.relative_luminance() > 0.34 {
                Rgb::new(12, 16, 22)
            } else {
                Rgb::new(255, 255, 255)
            };
            self.overlay_instances.push(QuadInstance::solid(
                zg.rect.x,
                zg.rect.y,
                zg.rect.w,
                zg.rect.h,
                zg_bg.to_f32_rgba(0.95),
            ));
            Self::emit_shaped_label(
                &mut self.atlas,
                &mut self.shaper,
                &mut self.overlay_instances,
                config,
                font_sys,
                &zg.text,
                8,
                zg.text_x,
                baseline_y,
                zg_fg.to_f32_rgba(1.0),
                true,
            );
        }

        // 5. Right theme pill (` ocean-dark ` — click or press Ctrl+Shift+P -> t to switch)
        if let Some(th) = &geom.theme_pill {
            let th_hov = status_hover_id == Some(4);
            let th_bg = if th_hov {
                config.colors.selection_background
            } else {
                key_bg
            };
            self.overlay_instances.push(QuadInstance::solid(
                th.rect.x,
                th.rect.y,
                th.rect.w,
                th.rect.h,
                th_bg.to_f32_rgba(0.96),
            ));
            self.overlay_instances.push(QuadInstance::solid(
                th.rect.x,
                th.rect.y,
                1.0,
                th.rect.h,
                fg.to_f32_rgba(0.14),
            ));
            let th_fg = fg.ensure_contrast(th_bg, 4.2);
            Self::emit_shaped_label(
                &mut self.atlas,
                &mut self.shaper,
                &mut self.overlay_instances,
                config,
                font_sys,
                &th.text,
                32,
                th.text_x,
                baseline_y,
                th_fg.to_f32_rgba(if th_hov { 1.0 } else { 0.88 }),
                th_hov,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_shaped_label(
        atlas: &mut GlyphAtlas,
        shaper: &mut TextShaper,
        target: &mut Vec<QuadInstance>,
        config: &Config,
        font_sys: &mut FontSystem,
        label: &str,
        max_chars: usize,
        start_x: f32,
        baseline_y: f32,
        fg_col: [f32; 4],
        bold: bool,
    ) {
        let mut cells_owned: Vec<(char, Vec<char>)> = Vec::new();
        for ch in label.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            if w == 0 {
                if let Some(last) = cells_owned.last_mut() {
                    last.1.push(ch);
                }
            } else {
                if cells_owned.len() >= max_chars {
                    break;
                }
                cells_owned.push((ch, Vec::new()));
            }
        }
        let style = if bold {
            FontStyle::Bold
        } else {
            FontStyle::Regular
        };
        let inputs: Vec<CellInput<'_>> = cells_owned
            .iter()
            .map(|(c, zw)| CellInput {
                c: *c,
                zerowidth: zw.as_slice(),
                style,
                is_wide_spacer: false,
                is_box_drawing: false,
            })
            .collect();

        let mut glyphs = Vec::new();
        shaper.shape_row(font_sys, &inputs, config.myanmar_zawgyi, &mut glyphs);
        let text_alpha = fg_col[3];

        for pg in &glyphs {
            let fpx = if pg.is_myanmar {
                font_sys.metrics.myanmar_font_px
            } else {
                font_sys.metrics.font_px
            };
            let entry = atlas.get_or_rasterize(
                font_sys,
                pg.font_id,
                pg.glyph_id,
                fpx,
                config.font_thicken,
            );
            if entry.width == 0 || entry.height == 0 {
                continue;
            }
            let gx = (start_x + pg.x + entry.left as f32 * pg.x_scale).round();
            let gy = (baseline_y - pg.y_offset - entry.top as f32).round();
            target.push(QuadInstance {
                pos: [gx, gy],
                size: [entry.width as f32 * pg.x_scale, entry.height as f32],
                uv_pos: [entry.uv_x, entry.uv_y],
                uv_size: [entry.uv_w, entry.uv_h],
                color: if entry.is_color {
                    [1.0, 1.0, 1.0, text_alpha]
                } else {
                    fg_col
                },
                kind: if entry.is_color { 2.0 } else { 1.0 },
            });
        }
    }

    fn emit_ime_preedit(
        &mut self,
        config: &Config,
        font_sys: &mut FontSystem,
        preedit: &str,
        x: f32,
        y: f32,
    ) {
        let mut cells_owned: Vec<(char, Vec<char>)> = Vec::new();
        for ch in preedit.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            if w == 0 {
                if let Some(last) = cells_owned.last_mut() {
                    last.1.push(ch);
                } else {
                    cells_owned.push((ch, Vec::new()));
                }
            } else {
                cells_owned.push((ch, Vec::new()));
            }
        }
        let span_w = (cells_owned.len().max(1) as f32) * font_sys.metrics.cell_width;
        let ch = font_sys.metrics.cell_height;

        // Opaque pill background + accent underline for IME preedit
        self.fg_instances.push(QuadInstance::solid(
            x,
            y,
            span_w,
            ch,
            config.colors.selection_background.to_f32_rgba(0.97),
        ));
        self.fg_instances.push(QuadInstance::solid(
            x,
            y + ch - 2.0,
            span_w,
            2.0,
            config.colors.cursor_color.to_f32_rgba(1.0),
        ));

        let inputs: Vec<CellInput<'_>> = cells_owned
            .iter()
            .map(|(c, zw)| CellInput {
                c: *c,
                zerowidth: zw.as_slice(),
                style: FontStyle::Regular,
                is_wide_spacer: false,
                is_box_drawing: false,
            })
            .collect();

        let mut glyphs = Vec::new();
        self.shaper
            .shape_row(font_sys, &inputs, config.myanmar_zawgyi, &mut glyphs);
        let baseline_y = y + font_sys.metrics.baseline;
        let fg = config.colors.foreground.to_f32_rgba(1.0);

        for pg in &glyphs {
            let fpx = if pg.is_myanmar {
                font_sys.metrics.myanmar_font_px
            } else {
                font_sys.metrics.font_px
            };
            let entry = self.atlas.get_or_rasterize(
                font_sys,
                pg.font_id,
                pg.glyph_id,
                fpx,
                config.font_thicken,
            );
            if entry.width == 0 || entry.height == 0 {
                continue;
            }
            let gx = (x + pg.x + entry.left as f32 * pg.x_scale).round();
            let gy = (baseline_y - pg.y_offset - entry.top as f32).round();
            self.fg_instances.push(QuadInstance {
                pos: [gx, gy],
                size: [entry.width as f32 * pg.x_scale, entry.height as f32],
                uv_pos: [entry.uv_x, entry.uv_y],
                uv_size: [entry.uv_w, entry.uv_h],
                color: if entry.is_color {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    fg
                },
                kind: if entry.is_color { 2.0 } else { 1.0 },
            });
        }
    }
}

#[inline]
fn effective_fg(rc: &RenderCell, config: &Config) -> Rgb {
    if config.minimum_contrast > 1.0 {
        rc.fg.ensure_contrast(rc.bg, config.minimum_contrast)
    } else {
        rc.fg
    }
}

/// Hardware-accelerated OpenGL 3.3 / GLES 3.0 Single-Draw-Call Instanced Renderer.
pub struct GlRenderer {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    vbo_capacity_bytes: usize,
    atlas_tex: glow::Texture,
    u_screen_size: glow::UniformLocation,
    tex_w: u32,
    tex_h: u32,
}

impl GlRenderer {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let is_es = gl.version().is_embedded;
            let header = if is_es {
                "#version 300 es\nprecision highp float;\n"
            } else {
                "#version 330 core\n"
            };

            let vert_src = format!(
                r#"{header}
layout(location = 0) in vec2 i_pos;
layout(location = 1) in vec2 i_size;
layout(location = 2) in vec2 i_uv_pos;
layout(location = 3) in vec2 i_uv_size;
layout(location = 4) in vec4 i_color;
layout(location = 5) in float i_kind;

uniform vec2 u_screen_size;

out vec2 v_uv;
out vec2 v_local_px;
out vec2 v_size_px;
out vec4 v_color;
flat out int v_kind;

void main() {{
    vec2 corner = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    vec2 px = i_pos + corner * i_size;
    vec2 ndc = (px / u_screen_size) * 2.0 - 1.0;
    gl_Position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    v_uv = i_uv_pos + corner * i_uv_size;
    v_local_px = corner * i_size;
    v_size_px = i_size;
    v_color = i_color;
    v_kind = int(i_kind + 0.5);
}}
"#
            );

            let frag_src = format!(
                r#"{header}
in vec2 v_uv;
in vec2 v_local_px;
in vec2 v_size_px;
in vec4 v_color;
flat in int v_kind;

uniform sampler2D u_atlas;

out vec4 f_color;

void main() {{
    if (v_kind == 0) {{
        float a = v_color.a;
        f_color = vec4(v_color.rgb * a, a);
    }} else if (v_kind == 1) {{
        float cov = texture(u_atlas, v_uv).a;
        float a = cov * v_color.a;
        f_color = vec4(v_color.rgb * a, a);
    }} else if (v_kind == 2) {{
        vec4 tex = texture(u_atlas, v_uv);
        float a = tex.a * v_color.a;
        f_color = vec4(tex.rgb * a, a);
    }} else {{
        float bw = 1.5;
        bool edge = v_local_px.x < bw || v_local_px.x > (v_size_px.x - bw)
                 || v_local_px.y < bw || v_local_px.y > (v_size_px.y - bw);
        if (!edge) discard;
        float a = v_color.a;
        f_color = vec4(v_color.rgb * a, a);
    }}
}}
"#
            );

            let program = compile_program(gl, &vert_src, &frag_src)?;
            let u_screen_size = gl
                .get_uniform_location(program, "u_screen_size")
                .ok_or_else(|| "Missing uniform u_screen_size".to_string())?;

            gl.use_program(Some(program));
            if let Some(u_atlas) = gl.get_uniform_location(program, "u_atlas") {
                gl.uniform_1_i32(Some(&u_atlas), 0);
            }

            let vao = gl
                .create_vertex_array()
                .map_err(|e| format!("create_vertex_array: {e}"))?;
            let vbo = gl
                .create_buffer()
                .map_err(|e| format!("create_buffer: {e}"))?;

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

            let initial_cap = 8192 * std::mem::size_of::<QuadInstance>();
            gl.buffer_data_size(
                glow::ARRAY_BUFFER,
                initial_cap as i32,
                glow::STREAM_DRAW,
            );

            let stride = std::mem::size_of::<QuadInstance>() as i32;
            let attrs: [(u32, i32, i32); 6] = [
                (0, 2, 0),
                (1, 2, 8),
                (2, 2, 16),
                (3, 2, 24),
                (4, 4, 32),
                (5, 1, 48),
            ];
            for (loc, comps, offset) in attrs {
                gl.enable_vertex_attrib_array(loc);
                gl.vertex_attrib_pointer_f32(loc, comps, glow::FLOAT, false, stride, offset);
                gl.vertex_attrib_divisor(loc, 1);
            }

            let atlas_tex = gl
                .create_texture()
                .map_err(|e| format!("create_texture: {e}"))?;
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(atlas_tex));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::DEPTH_TEST);

            Ok(Self {
                program,
                vao,
                vbo,
                vbo_capacity_bytes: initial_cap,
                atlas_tex,
                u_screen_size,
                tex_w: 0,
                tex_h: 0,
            })
        }
    }

    pub fn draw(
        &mut self,
        gl: &glow::Context,
        atlas: &mut GlyphAtlas,
        instances: &[QuadInstance],
        win_w: u32,
        win_h: u32,
        bg: Rgb,
        bg_opacity: f32,
    ) {
        unsafe {
            gl.viewport(0, 0, win_w as i32, win_h as i32);
            let op = bg_opacity.clamp(0.0, 1.0);
            gl.clear_color(
                (bg.r as f32 / 255.0) * op,
                (bg.g as f32 / 255.0) * op,
                (bg.b as f32 / 255.0) * op,
                op,
            );
            gl.clear(glow::COLOR_BUFFER_BIT);

            if instances.is_empty() {
                return;
            }

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);

            if atlas.resized || self.tex_w != atlas.width || self.tex_h != atlas.height {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    atlas.width as i32,
                    atlas.height as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&atlas.pixels)),
                );
                self.tex_w = atlas.width;
                self.tex_h = atlas.height;
                atlas.resized = false;
                atlas.dirty_rows = None;
            } else if let Some((y0, y1)) = atlas.dirty_rows.take() {
                let y0 = y0.min(atlas.height);
                let y1 = y1.min(atlas.height);
                if y1 > y0 {
                    let start = (y0 * atlas.width * 4) as usize;
                    let end = (y1 * atlas.width * 4) as usize;
                    gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        y0 as i32,
                        atlas.width as i32,
                        (y1 - y0) as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&atlas.pixels[start..end])),
                    );
                }
            }

            gl.use_program(Some(self.program));
            gl.uniform_2_f32(Some(&self.u_screen_size), win_w as f32, win_h as f32);
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));

            let byte_len = std::mem::size_of_val(instances);
            let raw_bytes =
                std::slice::from_raw_parts(instances.as_ptr() as *const u8, byte_len);

            if byte_len > self.vbo_capacity_bytes {
                self.vbo_capacity_bytes = byte_len.next_power_of_two();
                gl.buffer_data_size(
                    glow::ARRAY_BUFFER,
                    self.vbo_capacity_bytes as i32,
                    glow::STREAM_DRAW,
                );
            }
            gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, raw_bytes);

            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, instances.len() as i32);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.vbo);
            gl.delete_texture(self.atlas_tex);
        }
    }
}

unsafe fn compile_program(
    gl: &glow::Context,
    vert_src: &str,
    frag_src: &str,
) -> Result<glow::Program, String> {
    unsafe {
        let program = gl
            .create_program()
            .map_err(|e| format!("create_program: {e}"))?;
        let vs = compile_shader(gl, glow::VERTEX_SHADER, vert_src)?;
        let fs = compile_shader(gl, glow::FRAGMENT_SHADER, frag_src)?;
        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        gl.link_program(program);
        gl.detach_shader(program, vs);
        gl.detach_shader(program, fs);
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(format!("GL program link failed: {log}"));
        }
        Ok(program)
    }
}

unsafe fn compile_shader(
    gl: &glow::Context,
    ty: u32,
    src: &str,
) -> Result<glow::Shader, String> {
    unsafe {
        let sh = gl
            .create_shader(ty)
            .map_err(|e| format!("create_shader: {e}"))?;
        gl.shader_source(sh, src);
        gl.compile_shader(sh);
        if !gl.get_shader_compile_status(sh) {
            let log = gl.get_shader_info_log(sh);
            gl.delete_shader(sh);
            return Err(format!("GL shader compile failed: {log}"));
        }
        Ok(sh)
    }
}

/// Software rasterizer of the exact GPU `QuadInstance` list + `GlyphAtlas` to a PNG file
/// (used for `ocean --screenshot <out.png>` and headless rendering verification).
pub fn save_scene_png(
    path: &Path,
    atlas: &GlyphAtlas,
    instances: &[QuadInstance],
    width: u32,
    height: u32,
    bg: Rgb,
) -> Result<(), String> {
    let mut fb = vec![0u8; (width * height * 4) as usize];
    for px in fb.chunks_exact_mut(4) {
        px[0] = bg.r;
        px[1] = bg.g;
        px[2] = bg.b;
        px[3] = 255;
    }

    let aw = atlas.width as f32;
    let ah = atlas.height as f32;

    for inst in instances {
        let x0 = inst.pos[0].floor().max(0.0) as i32;
        let y0 = inst.pos[1].floor().max(0.0) as i32;
        let x1 = (inst.pos[0] + inst.size[0]).ceil().min(width as f32) as i32;
        let y1 = (inst.pos[1] + inst.size[1]).ceil().min(height as f32) as i32;
        if x1 <= x0 || y1 <= y0 || inst.size[0] <= 0.0 || inst.size[1] <= 0.0 {
            continue;
        }
        let kind = (inst.kind + 0.5) as i32;

        for py in y0..y1 {
            for px in x0..x1 {
                let lx = (px as f32 + 0.5) - inst.pos[0];
                let ly = (py as f32 + 0.5) - inst.pos[1];
                if lx < 0.0 || ly < 0.0 || lx > inst.size[0] || ly > inst.size[1] {
                    continue;
                }

                let (sr, sg, sb, sa) = match kind {
                    0 => (
                        inst.color[0],
                        inst.color[1],
                        inst.color[2],
                        inst.color[3],
                    ),
                    3 => {
                        let edge = lx < 1.5
                            || lx > inst.size[0] - 1.5
                            || ly < 1.5
                            || ly > inst.size[1] - 1.5;
                        if !edge {
                            continue;
                        }
                        (
                            inst.color[0],
                            inst.color[1],
                            inst.color[2],
                            inst.color[3],
                        )
                    }
                    1 | 2 => {
                        let u = (inst.uv_pos[0] + (lx / inst.size[0]) * inst.uv_size[0]) * aw - 0.5;
                        let v = (inst.uv_pos[1] + (ly / inst.size[1]) * inst.uv_size[1]) * ah - 0.5;
                        let tex = sample_bilinear(&atlas.pixels, atlas.width, atlas.height, u, v);
                        if kind == 1 {
                            (
                                inst.color[0],
                                inst.color[1],
                                inst.color[2],
                                tex[3] * inst.color[3],
                            )
                        } else {
                            (tex[0], tex[1], tex[2], tex[3] * inst.color[3])
                        }
                    }
                    _ => continue,
                };

                if sa <= 0.001 {
                    continue;
                }
                let idx = ((py as u32 * width + px as u32) * 4) as usize;
                let dr = fb[idx] as f32 / 255.0;
                let dg = fb[idx + 1] as f32 / 255.0;
                let db = fb[idx + 2] as f32 / 255.0;
                let inv = 1.0 - sa;
                fb[idx] = ((sr * sa + dr * inv).clamp(0.0, 1.0) * 255.0).round() as u8;
                fb[idx + 1] = ((sg * sa + dg * inv).clamp(0.0, 1.0) * 255.0).round() as u8;
                fb[idx + 2] = ((sb * sa + db * inv).clamp(0.0, 1.0) * 255.0).round() as u8;
                fb[idx + 3] = 255;
            }
        }
    }

    let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(&fb)
        .map_err(|e| format!("png data: {e}"))?;
    Ok(())
}

fn sample_bilinear(pixels: &[u8], w: u32, h: u32, u: f32, v: f32) -> [f32; 4] {
    let x0 = u.floor() as i32;
    let y0 = v.floor() as i32;
    let fx = u - x0 as f32;
    let fy = v - y0 as f32;

    let fetch = |x: i32, y: i32| -> [f32; 4] {
        let cx = x.clamp(0, w as i32 - 1) as u32;
        let cy = y.clamp(0, h as i32 - 1) as u32;
        let i = ((cy * w + cx) * 4) as usize;
        [
            pixels[i] as f32 / 255.0,
            pixels[i + 1] as f32 / 255.0,
            pixels[i + 2] as f32 / 255.0,
            pixels[i + 3] as f32 / 255.0,
        ]
    };

    let c00 = fetch(x0, y0);
    let c10 = fetch(x0 + 1, y0);
    let c01 = fetch(x0, y0 + 1);
    let c11 = fetch(x0 + 1, y0 + 1);

    let mut out = [0.0; 4];
    for c in 0..4 {
        let top = c00[c] * (1.0 - fx) + c10[c] * fx;
        let bot = c01[c] * (1.0 - fx) + c11[c] * fx;
        out[c] = top * (1.0 - fy) + bot * fy;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::keybind::SplitDirection;
    use crate::split::{SplitNode, compute_split_layout};
    use crate::terminal::snapshot_from_bytes;

    #[test]
    fn renders_split_panes_tab_bar_and_context_menu_scene() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let win_w = 960;
        let win_h = 560;

        let tabs = [
            TabTitle {
                title: "ocean@linux:~/BDATA/ocean",
                active: true,
            },
            TabTitle {
                title: "cargo watch -x test",
                active: false,
            },
        ];
        let tab_h = RenderEngine::tab_bar_height(&config, &font_sys, tabs.len());
        let status_h = RenderEngine::status_bar_height(&config, &font_sys, 1.0);

        let mut root = SplitNode::Leaf(1);
        root.split_leaf(1, 2, SplitDirection::Right);
        root.split_leaf(2, 3, SplitDirection::Down);
        let (layouts, dividers) = compute_split_layout(
            &root, &config, &font_sys, win_w, win_h, 1.0, tab_h, status_h,
        );
        assert_eq!(layouts.len(), 3);
        assert_eq!(dividers.len(), 2);

        let s1 = snapshot_from_bytes(
            b"\x1b[1;36m# Left Split Pane\x1b[0m\r\n$ ocean +show-config\r\nwindow-titlebar = false\r\n\x1b[1;32m\xe1\x80\x99\xe1\x80\x84\xe1\x80\xba\xe1\x80\xb9\xe1\x80\x82\xe1\x80\x9c\xe1\x80\xac\xe1\x80\x95\xe1\x80\xab\x1b[0m\r\n$ ",
            &config,
            layouts[0].cols as usize,
            layouts[0].rows as usize,
        );
        let s2 = snapshot_from_bytes(
            b"\x1b[1;33m# Top-Right Split\x1b[0m\r\n$ git status -sb\r\n## main\r\n$ ",
            &config,
            layouts[1].cols as usize,
            layouts[1].rows as usize,
        );
        let s3 = snapshot_from_bytes(
            b"\x1b[1;35m# Bottom-Right Split\x1b[0m\r\n$ cargo test\r\ntest result: \x1b[32mok\x1b[0m\r\n$ ",
            &config,
            layouts[2].cols as usize,
            layouts[2].rows as usize,
        );

        let scenes = [
            PaneScene {
                snapshot: &s1,
                layout: layouts[0],
                is_active: true,
                ime_preedit: None,
            },
            PaneScene {
                snapshot: &s2,
                layout: layouts[1],
                is_active: false,
                ime_preedit: None,
            },
            PaneScene {
                snapshot: &s3,
                layout: layouts[2],
                is_active: false,
                ime_preedit: None,
            },
        ];

        let mut menu = ContextMenu::build(180.0, 96.0, true, 3, true, &config.keybinds);
        menu.hovered = menu.theme_submenu_entry_index();
        menu.submenu_open = true;
        menu.sub_hovered = Some(2); // Hover "Tokyo Night"

        let mut engine = RenderEngine::new();
        if let Ok(splits_png) = std::env::var("OCEAN_TEST_SPLITS_BAR_PNG") {
            let (atlas_s, instances_s) = engine.build_scene(
                &config,
                &mut font_sys,
                &scenes,
                &dividers,
                win_w,
                win_h,
                1.0,
                &tabs,
                None,
                None,
                None,
                true,
                true,
            );
            save_scene_png(
                Path::new(&splits_png),
                atlas_s,
                instances_s,
                win_w,
                win_h,
                config.colors.background,
            )
            .expect("save splits bar preview png");
        }

        let (atlas, instances) = engine.build_scene(
            &config,
            &mut font_sys,
            &scenes,
            &dividers,
            win_w,
            win_h,
            1.0,
            &tabs,
            Some(&menu),
            None,
            None,
            true,
            true,
        );
        assert!(!instances.is_empty());

        if let Ok(out_png) = std::env::var("OCEAN_TEST_PREVIEW_PNG") {
            save_scene_png(
                Path::new(&out_png),
                atlas,
                instances,
                win_w,
                win_h,
                config.colors.background,
            )
            .expect("save preview png");
        }

        // Also test rendering the Help & Documentation dialog across dark & light themes
        let mut help = HelpDialog::new();
        let (atlas_h, instances_h) = engine.build_scene(
            &config,
            &mut font_sys,
            &scenes,
            &dividers,
            win_w,
            win_h,
            1.0,
            &tabs,
            None,
            Some(&help),
            None,
            true,
            true,
        );
        assert!(!instances_h.is_empty());

        if let Ok(help_png) = std::env::var("OCEAN_TEST_HELP_PNG") {
            save_scene_png(
                Path::new(&help_png),
                atlas_h,
                instances_h,
                win_w,
                win_h,
                config.colors.background,
            )
            .expect("save help preview png");
        }

        if let Ok(light_png) = std::env::var("OCEAN_TEST_LIGHT_MENU_PNG") {
            let mut light_cfg = Config::default();
            light_cfg.apply_cli_kv("theme", "tokyo-night-light");
            let sl1 = snapshot_from_bytes(
                b"\x1b[1;36m# Left Split Pane\x1b[0m\r\n$ ocean +show-config\r\ntheme = tokyo-night-light\r\n\x1b[1;32m\xe1\x80\x99\xe1\x80\x84\xe1\x80\xba\xe1\x80\xb9\xe1\x80\x82\xe1\x80\x9c\xe1\x80\xac\xe1\x80\x95\xe1\x80\xab\x1b[0m\r\n$ ",
                &light_cfg,
                layouts[0].cols as usize,
                layouts[0].rows as usize,
            );
            let sl2 = snapshot_from_bytes(
                b"\x1b[1;33m# Top-Right Split\x1b[0m\r\n$ ocean +list-themes\r\n* tokyo-night-light\r\n$ ",
                &light_cfg,
                layouts[1].cols as usize,
                layouts[1].rows as usize,
            );
            let sl3 = snapshot_from_bytes(
                b"\x1b[1;35m# Bottom-Right Split\x1b[0m\r\n$ cargo test\r\ntest result: \x1b[32mok\x1b[0m\r\n$ ",
                &light_cfg,
                layouts[2].cols as usize,
                layouts[2].rows as usize,
            );
            let light_scenes = [
                PaneScene {
                    snapshot: &sl1,
                    layout: layouts[0],
                    is_active: true,
                    ime_preedit: None,
                },
                PaneScene {
                    snapshot: &sl2,
                    layout: layouts[1],
                    is_active: false,
                    ime_preedit: None,
                },
                PaneScene {
                    snapshot: &sl3,
                    layout: layouts[2],
                    is_active: false,
                    ime_preedit: None,
                },
            ];
            let mut light_menu =
                ContextMenu::build(180.0, 96.0, true, 3, true, &light_cfg.keybinds);
            light_menu.hovered = light_menu.theme_submenu_entry_index();
            light_menu.submenu_open = true;
            light_menu.sub_hovered = Some(9); // Hover "Tokyo Night Light"
            let (atlas_l, instances_l) = engine.build_scene(
                &light_cfg,
                &mut font_sys,
                &light_scenes,
                &dividers,
                win_w,
                win_h,
                1.0,
                &tabs,
                Some(&light_menu),
                None,
                None,
                true,
                true,
            );
            save_scene_png(
                Path::new(&light_png),
                atlas_l,
                instances_l,
                win_w,
                win_h,
                light_cfg.colors.background,
            )
            .expect("save light menu preview png");

            if let Ok(light_help_png) = std::env::var("OCEAN_TEST_LIGHT_HELP_PNG") {
                help.active_tab = 3; // Config & Themes section
                let (atlas_lh, instances_lh) = engine.build_scene(
                    &light_cfg,
                    &mut font_sys,
                    &light_scenes,
                    &dividers,
                    win_w,
                    win_h,
                    1.0,
                    &tabs,
                    None,
                    Some(&help),
                    None,
                    true,
                    true,
                );
                save_scene_png(
                    Path::new(&light_help_png),
                    atlas_lh,
                    instances_lh,
                    win_w,
                    win_h,
                    light_cfg.colors.background,
                )
                .expect("save light help preview png");
            }
        }
    }
}
