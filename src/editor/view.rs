use std::sync::Arc;

use crate::config::Config;
use crate::editor::document::{EditorDocument, SelectionKind};
use crate::editor::explorer::FileExplorer;
use crate::editor::lsp::EditorDiagnostic;
use crate::editor::overlay::{CommandPalette, CompletionOverlay, FindBar, HoverOverlay};
use crate::editor::syntax::SyntaxTokenKind;
use crate::editor::vim::{VimMode, VimState};
use crate::font::{FontStyle, FontSystem};
use crate::renderer::QuadInstance;
use crate::renderer::atlas::GlyphAtlas;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorViewport {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct EditorView;

impl EditorView {
    #[allow(clippy::too_many_arguments)]
    pub fn emit_scene(
        config: &Config,
        font_sys: &mut FontSystem,
        atlas: &mut GlyphAtlas,
        bg_instances: &mut Vec<QuadInstance>,
        fg_instances: &mut Vec<QuadInstance>,
        overlay_instances: &mut Vec<QuadInstance>,
        doc: &mut EditorDocument,
        doc_titles: &[(String, bool)],
        active_doc_idx: usize,
        raw_vp: &EditorViewport,
        cursor_blink: bool,
        is_focused: bool,
        palette: Option<&CommandPalette>,
        completion: Option<&CompletionOverlay>,
        find_bar: Option<&FindBar>,
        hover: Option<&HoverOverlay>,
        diagnostics: &[EditorDiagnostic],
        plugin_status_items: &[String],
        lsp_status: Option<&str>,
        explorer: Option<&FileExplorer>,
        scrollbar_dragging: bool,
        scrollbar_hovered: bool,
        problems: &[crate::editor::ProblemEntry],
        active_panel: crate::editor::SidebarPanel,
        activity_bar_visible: bool,
        vim: Option<&VimState>,
        ime_preedit: Option<&str>,
    ) {
        if raw_vp.w <= 10.0 || raw_vp.h <= 10.0 {
            return;
        }

        let theme = &config.colors;
        let line_h = font_sys.metrics.cell_height;
        let cell_w = font_sys.metrics.cell_width;
        let baseline = font_sys.metrics.baseline;

        let activity_w = if activity_bar_visible { 44.0 } else { 0.0 };
        let mut panel_open = false;
        let mut sidebar_w = 0.0;

        if let Some(exp) = explorer {
            panel_open = exp.visible
                && (active_panel == crate::editor::SidebarPanel::Problems || exp.is_open());
            if panel_open {
                sidebar_w = if active_panel == crate::editor::SidebarPanel::Problems {
                    exp.width.max(230.0).min((raw_vp.w - 160.0).max(80.0))
                } else {
                    exp.width.min(raw_vp.w - 100.0).max(80.0)
                };
            }
        }

        if activity_bar_visible {
            Self::emit_activity_bar(
                config,
                font_sys,
                atlas,
                bg_instances,
                fg_instances,
                raw_vp,
                activity_w,
                active_panel,
            );
        }

        if panel_open {
            let sidebar_x = raw_vp.x + activity_w;
            match active_panel {
                crate::editor::SidebarPanel::Explorer => {
                    if let Some(exp) = explorer {
                        Self::emit_sidebar(
                            config,
                            font_sys,
                            atlas,
                            bg_instances,
                            fg_instances,
                            exp,
                            sidebar_x,
                            raw_vp.y,
                            sidebar_w,
                            raw_vp.h,
                        );
                    }
                }
                crate::editor::SidebarPanel::Problems => {
                    Self::emit_problems(
                        config,
                        font_sys,
                        atlas,
                        bg_instances,
                        fg_instances,
                        problems,
                        sidebar_x,
                        raw_vp.y,
                        sidebar_w,
                        raw_vp.h,
                        line_h,
                        baseline,
                    );
                }
            }
        }

        let code_vp = EditorViewport {
            x: raw_vp.x + activity_w + if panel_open { sidebar_w } else { 0.0 },
            y: raw_vp.y,
            w: (raw_vp.w - activity_w - if panel_open { sidebar_w } else { 0.0 }).max(10.0),
            h: raw_vp.h,
        };
        let vp = &code_vp;

        // 1. Viewport background
        bg_instances.push(QuadInstance::solid(
            vp.x,
            vp.y,
            vp.w,
            vp.h,
            theme.background.to_f32_rgba(1.0),
        ));

        // 2. Document Tab Bar (always visible in the editor)
        let tab_h = (line_h + 6.0).round();
        let effective_tab_h = tab_h;

        bg_instances.push(QuadInstance::solid(
            vp.x,
            vp.y,
            vp.w,
            tab_h,
            theme.background.to_f32_rgba(0.95),
        ));
        fg_instances.push(QuadInstance::solid(
            vp.x,
            vp.y + tab_h - 1.0,
            vp.w,
            1.0,
            theme.foreground.to_f32_rgba(0.12),
        ));

        let mut tab_x = vp.x + 4.0;
        let tab_w = (180.0_f32)
            .min((vp.w - 40.0) / doc_titles.len().max(1) as f32)
            .max(60.0);
        let max_title_chars = (((tab_w - 34.0) / cell_w).floor() as usize).max(4);

        for (idx, (name, is_dirty)) in doc_titles.iter().enumerate() {
            let is_active = idx == active_doc_idx;
            let bg_col = if is_active {
                theme.selection_background.to_f32_rgba(0.40)
            } else {
                theme.background.to_f32_rgba(0.20)
            };

            bg_instances.push(QuadInstance::solid(
                tab_x,
                vp.y + 2.0,
                tab_w,
                tab_h - 3.0,
                bg_col,
            ));

            if is_active {
                fg_instances.push(QuadInstance::solid(
                    tab_x,
                    vp.y + 2.0,
                    tab_w,
                    2.0,
                    theme.cursor_color.to_f32_rgba(1.0),
                ));
            }

            let title: String = name.chars().take(max_title_chars).collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &title,
                tab_x + 8.0,
                vp.y + 2.0 + (tab_h - line_h) * 0.5 + baseline,
                if is_active {
                    theme.foreground.to_f32_rgba(1.0)
                } else {
                    theme.foreground.to_f32_rgba(0.60)
                },
            );

            if *is_dirty {
                fg_instances.push(QuadInstance::solid(
                    tab_x + tab_w - 26.0,
                    vp.y + (tab_h - 5.0) * 0.5,
                    5.0,
                    5.0,
                    theme.ansi[3].to_f32_rgba(0.95),
                ));
            }

            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                "x",
                tab_x + tab_w - 14.0,
                vp.y + 2.0 + (tab_h - line_h) * 0.5 + baseline,
                if is_active {
                    theme.foreground.to_f32_rgba(0.75)
                } else {
                    theme.foreground.to_f32_rgba(0.35)
                },
            );

            tab_x += tab_w + 4.0;
        }

        // New tab button
        if tab_x + 22.0 < vp.x + vp.w - 8.0 {
            bg_instances.push(QuadInstance::solid(
                tab_x,
                vp.y + 3.0,
                tab_h - 4.0,
                tab_h - 5.0,
                theme.foreground.to_f32_rgba(0.06),
            ));
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                "+",
                tab_x + cell_w * 0.7,
                vp.y + 2.0 + (tab_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.65),
            );
        }

        // 2b. Breadcrumb bar
        let crumb_h = if doc.path().is_some() {
            line_h + 4.0
        } else {
            0.0
        };
        if crumb_h > 0.0 {
            let crumb_y = vp.y + tab_h;
            bg_instances.push(QuadInstance::solid(
                vp.x,
                crumb_y,
                vp.w,
                crumb_h,
                theme.selection_background.to_f32_rgba(0.14),
            ));
            fg_instances.push(QuadInstance::solid(
                vp.x,
                crumb_y + crumb_h - 1.0,
                vp.w,
                1.0,
                theme.foreground.to_f32_rgba(0.08),
            ));

            let path = doc.path().cloned().unwrap_or_default();
            let relative = explorer
                .and_then(|exp| {
                    path.strip_prefix(&exp.root_dir)
                        .ok()
                        .map(std::path::Path::to_path_buf)
                })
                .unwrap_or_else(|| path.clone());
            let crumbs = if relative.as_os_str().is_empty() {
                doc.file_name()
            } else {
                relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(" > ")
            };

            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &crumbs,
                vp.x + 8.0,
                crumb_y + (crumb_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.62),
            );
        }

        // 3. Status Bar (bottom of editor view)
        let status_h = (line_h + 4.0).round();
        let status_y = vp.y + vp.h - status_h;

        bg_instances.push(QuadInstance::solid(
            vp.x,
            status_y,
            vp.w,
            status_h,
            theme.selection_background.to_f32_rgba(0.25),
        ));
        fg_instances.push(QuadInstance::solid(
            vp.x,
            status_y,
            vp.w,
            1.0,
            theme.foreground.to_f32_rgba(0.12),
        ));

        // Status bar elements
        let status_baseline = status_y + (status_h - line_h) * 0.5 + baseline;
        let mode_text = if let Some(vim) = vim {
            format!(" {} ", vim.indicator())
        } else if is_focused {
            " NORMAL ".to_string()
        } else {
            " (SPLIT) ".to_string()
        };
        let mode_color = match vim.map(|v| v.mode) {
            Some(VimMode::Insert) => theme.ansi[2],
            Some(VimMode::Visual) | Some(VimMode::VisualLine) => theme.ansi[5],
            Some(VimMode::Command) => theme.ansi[3],
            _ => theme.ansi[4],
        };
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &mode_text,
            vp.x + 8.0,
            status_baseline,
            mode_color.to_f32_rgba(1.0),
        );

        let file_info = format!("{}  {} lines", doc.file_name(), doc.buffer.len_lines());
        let file_info_x = vp.x + 12.0 + (mode_text.chars().count() as f32 + 1.0) * cell_w;
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &file_info,
            file_info_x,
            status_baseline,
            theme.foreground.to_f32_rgba(0.85),
        );

        let error_count = diagnostics
            .iter()
            .filter(|d| d.severity == lsp_types::DiagnosticSeverity::ERROR)
            .count();
        let warning_count = diagnostics
            .iter()
            .filter(|d| d.severity == lsp_types::DiagnosticSeverity::WARNING)
            .count();
        let mut right_info = format!(
            "Ln {}, Col {}  |  E:{} W:{}  |  {}  |  {}",
            doc.cursor.line + 1,
            doc.cursor.cluster_idx + 1,
            error_count,
            warning_count,
            doc.language.display_name(),
            lsp_status.unwrap_or("LSP: Idle"),
        );
        for item in plugin_status_items {
            right_info.push_str(&format!("  |  {item}"));
        }
        if let Some(vim) = vim
            && let Some((message, at)) = &vim.status_message
            && at.elapsed().as_secs() < 5
        {
            right_info.push_str(&format!("  |  {message}"));
        }

        let right_w = right_info.chars().count() as f32 * cell_w;
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &right_info,
            (vp.x + vp.w - right_w - 12.0).max(vp.x + 200.0),
            status_baseline,
            theme.foreground.to_f32_rgba(0.70),
        );

        // 4. Main Code Area & Gutter
        let content_y = vp.y + effective_tab_h + crumb_h;
        let content_h = (status_y - content_y).max(1.0);

        let max_line_digits = (doc.buffer.len_lines() + 1).to_string().len().max(3);
        let gutter_w = (max_line_digits as f32 + 3.0) * cell_w;
        let scrollbar_w = 8.0_f32;
        let scrollbar_x = vp.x + vp.w - scrollbar_w;
        let text_area_x = vp.x + gutter_w;
        let text_area_w = (vp.w - gutter_w - scrollbar_w).max(1.0);

        // Gutter background & border
        bg_instances.push(QuadInstance::solid(
            vp.x,
            content_y,
            gutter_w,
            content_h,
            theme.background.to_f32_rgba(0.98),
        ));
        fg_instances.push(QuadInstance::solid(
            vp.x + gutter_w - 1.0,
            content_y,
            1.0,
            content_h,
            theme.foreground.to_f32_rgba(0.08),
        ));

        let visible_lines = (content_h / line_h).ceil() as usize + 1;
        if doc.scroll_to_cursor_requested {
            doc.ensure_cursor_visible(visible_lines.saturating_sub(2), vp.w, gutter_w, font_sys);
            doc.scroll_to_cursor_requested = false;
        }

        let max_scroll = doc.buffer.len_lines().saturating_sub(1);
        doc.scroll_y = doc.scroll_y.min(max_scroll);

        let start_line = doc.scroll_y;
        let end_line = (start_line + visible_lines).min(doc.buffer.len_lines());

        // Current line highlight
        if doc.cursor.line >= start_line && doc.cursor.line < end_line {
            let row_idx = doc.cursor.line - start_line;
            let cur_line_y = content_y + row_idx as f32 * line_h;
            bg_instances.push(QuadInstance::solid(
                vp.x,
                cur_line_y,
                (scrollbar_x - vp.x).max(cell_w),
                line_h,
                theme.foreground.to_f32_rgba(0.055),
            ));
        }

        let bracket_match = doc.matching_bracket(font_sys);

        // Selection bounds
        let (sel_start, sel_end) = doc.selection_ordered_cursors();
        let vim_visual = vim.map(|v| v.mode.is_visual()).unwrap_or(false);
        let has_selection = doc.selection_anchor.is_some() && (sel_start != sel_end || vim_visual);
        let selection_kind = doc.selection_kind;
        let visual_char_selection = vim_visual && selection_kind == SelectionKind::Char;

        let scroll_x = doc.scroll_x;
        let cursor_line = doc.cursor.line;
        let cursor_cluster = doc.cursor.cluster_idx;

        // Render visible lines with zero-allocation cached shaping & syntax highlighting
        for line_idx in start_line..end_line {
            let row_idx = line_idx - start_line;
            let row_y = content_y + row_idx as f32 * line_h;
            let row_baseline = row_y + baseline;

            // Line number in gutter
            let line_num_str = format!("{:>width$} ", line_idx + 1, width = max_line_digits);
            let num_col = if line_idx == cursor_line {
                theme.foreground.to_f32_rgba(0.90)
            } else {
                theme.foreground.to_f32_rgba(0.35)
            };
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &line_num_str,
                vp.x + cell_w * 0.5,
                row_baseline,
                num_col,
            );

            // Diagnostic marker in gutter (dot if error/warning)
            if let Some(diag) = diagnostics.iter().find(|d| d.start_line == line_idx) {
                let dot_col = match diag.severity {
                    lsp_types::DiagnosticSeverity::ERROR => theme.ansi[1].to_f32_rgba(1.0),
                    _ => theme.ansi[3].to_f32_rgba(1.0),
                };
                fg_instances.push(QuadInstance::solid(
                    vp.x + 2.0,
                    row_y + (line_h - 6.0) * 0.5,
                    4.0,
                    6.0,
                    dot_col,
                ));
            }

            let line_text = doc.buffer.line_without_newline(line_idx);
            let indent = line_text.chars().take_while(|c| *c == ' ').count();
            for level in 1..=(indent / 4) {
                let gx = text_area_x - scroll_x + level as f32 * 4.0 * cell_w;
                if gx >= text_area_x && gx < scrollbar_x {
                    bg_instances.push(QuadInstance::solid(
                        gx.round(),
                        row_y,
                        1.0,
                        line_h,
                        theme.foreground.to_f32_rgba(0.075),
                    ));
                }
            }

            let cached = doc.cached_line(line_idx, font_sys);
            let shaped = Arc::clone(&cached.shaped);
            let cluster_tokens = &cached.cluster_tokens;

            // Selection background on this line
            if has_selection && line_idx >= sel_start.line && line_idx <= sel_end.line {
                if selection_kind == SelectionKind::Line {
                    bg_instances.push(QuadInstance::solid(
                        text_area_x,
                        row_y,
                        text_area_w,
                        line_h,
                        theme.selection_background.to_f32_rgba(0.45),
                    ));
                } else {
                    let start_cluster = if line_idx == sel_start.line {
                        sel_start.cluster_idx
                    } else {
                        0
                    };
                    let end_cluster = if line_idx == sel_end.line {
                        if visual_char_selection {
                            (sel_end.cluster_idx + 1).min(shaped.cluster_count())
                        } else {
                            sel_end.cluster_idx
                        }
                    } else {
                        shaped.cluster_count()
                    };

                    let x1 = text_area_x - scroll_x + shaped.visual_x_for_cluster(start_cluster);
                    let x2 = text_area_x - scroll_x + shaped.visual_x_for_cluster(end_cluster);
                    let sel_w = (x2 - x1).max(cell_w);

                    bg_instances.push(QuadInstance::solid(
                        x1.max(text_area_x),
                        row_y,
                        sel_w.min(text_area_w),
                        line_h,
                        theme.selection_background.to_f32_rgba(0.45),
                    ));
                }
            }

            for glyph in &shaped.glyphs {
                let fpx = if glyph.is_myanmar {
                    font_sys.metrics.myanmar_font_px
                } else {
                    font_sys.metrics.font_px
                };
                let entry = atlas.get_or_rasterize(
                    font_sys,
                    glyph.font_id,
                    glyph.glyph_id,
                    fpx,
                    config.font_thicken,
                );
                if entry.width == 0 || entry.height == 0 {
                    continue;
                }

                let token_kind = cluster_tokens
                    .get(glyph.cluster_idx)
                    .copied()
                    .unwrap_or(SyntaxTokenKind::Plain);

                let fg_rgb = token_kind.color(theme);

                let gx = (text_area_x - scroll_x + glyph.x + entry.left as f32).round();
                let gy = (row_baseline - glyph.y_offset - entry.top as f32).round();

                // Clip to text area and vertical bounds
                if gx + (entry.width as f32) < text_area_x
                    || gx > scrollbar_x
                    || gy + (entry.height as f32) < content_y
                    || gy > status_y
                {
                    continue;
                }

                fg_instances.push(QuadInstance {
                    pos: [gx, gy],
                    size: [entry.width as f32, entry.height as f32],
                    uv_pos: [entry.uv_x, entry.uv_y],
                    uv_size: [entry.uv_w, entry.uv_h],
                    color: if entry.is_color {
                        [1.0, 1.0, 1.0, 1.0]
                    } else {
                        fg_rgb.to_f32_rgba(1.0)
                    },
                    kind: if entry.is_color { 2.0 } else { 1.0 },
                });
            }

            // Bracket match highlight
            if let Some(((match_line_a, match_cluster_a), (match_line_b, match_cluster_b))) =
                bracket_match
                && (line_idx == match_line_a || line_idx == match_line_b)
            {
                let cluster = if line_idx == match_line_a {
                    match_cluster_a
                } else {
                    match_cluster_b
                };
                let x1 = text_area_x - scroll_x + shaped.visual_x_for_cluster(cluster);
                let x2 = text_area_x - scroll_x + shaped.visual_x_for_cluster(cluster + 1);
                fg_instances.push(QuadInstance::hollow(
                    x1.round(),
                    row_y,
                    (x2 - x1).max(cell_w),
                    line_h,
                    theme.cursor_color.to_f32_rgba(0.75),
                ));
            }

            // Diagnostic underline squiggles
            for diag in diagnostics.iter().filter(|d| d.start_line == line_idx) {
                let x1 = text_area_x - scroll_x + shaped.visual_x_for_cluster(diag.start_col);
                let x2 = text_area_x - scroll_x
                    + shaped.visual_x_for_cluster(diag.end_col.max(diag.start_col + 1));
                let sq_color = match diag.severity {
                    lsp_types::DiagnosticSeverity::ERROR => theme.ansi[1].to_f32_rgba(0.9),
                    _ => theme.ansi[3].to_f32_rgba(0.9),
                };
                fg_instances.push(QuadInstance::solid(
                    x1.max(text_area_x),
                    row_y + line_h - 2.0,
                    (x2 - x1).max(cell_w),
                    2.0,
                    sq_color,
                ));
            }
        }

        // 5. Cursor rendering
        if is_focused && cursor_blink && cursor_line >= start_line && cursor_line < end_line {
            let row_idx = cursor_line - start_line;
            let row_y = content_y + row_idx as f32 * line_h;
            let shaped = doc.shaped_line(cursor_line, font_sys);
            let visual_x = shaped.visual_x_for_cluster(cursor_cluster);
            let cx = text_area_x - scroll_x + visual_x;

            if cx >= text_area_x && cx <= scrollbar_x {
                let block_cursor = vim
                    .map(|v| !matches!(v.mode, VimMode::Insert))
                    .unwrap_or(false);
                if block_cursor {
                    let next_x = shaped.visual_x_for_cluster(cursor_cluster + 1);
                    let block_w = (next_x - visual_x).max(cell_w);
                    fg_instances.push(QuadInstance::solid(
                        cx.round(),
                        row_y,
                        block_w,
                        line_h,
                        theme.cursor_color.to_f32_rgba(0.38),
                    ));
                }
                fg_instances.push(QuadInstance::solid(
                    cx.round(),
                    row_y,
                    2.5,
                    line_h,
                    theme.cursor_color.to_f32_rgba(1.0),
                ));
            }
        }

        // 5a. IME preedit composition anchored at the editor caret
        if let Some(preedit) = ime_preedit
            && is_focused
            && !preedit.is_empty()
            && cursor_line >= start_line
            && cursor_line < end_line
        {
            let row_idx = cursor_line - start_line;
            let row_y = content_y + row_idx as f32 * line_h;
            let row_baseline = row_y + baseline;
            let cursor_shaped = doc.shaped_line(cursor_line, font_sys);
            let cursor_x =
                text_area_x - scroll_x + cursor_shaped.visual_x_for_cluster(cursor_cluster);
            let shaped = crate::editor::myanmar::shape_and_measure_line(preedit, font_sys);
            let width = shaped.total_width.max(cell_w) + 4.0;

            if cursor_x + width >= text_area_x && cursor_x <= scrollbar_x {
                bg_instances.push(QuadInstance::solid(
                    (cursor_x - 2.0).round(),
                    row_y,
                    width,
                    line_h,
                    theme.selection_background.to_f32_rgba(0.65),
                ));
                fg_instances.push(QuadInstance::solid(
                    (cursor_x - 2.0).round(),
                    row_y + line_h - 2.0,
                    width,
                    2.0,
                    theme.cursor_color.to_f32_rgba(0.95),
                ));

                for glyph in &shaped.glyphs {
                    let fpx = if glyph.is_myanmar {
                        font_sys.metrics.myanmar_font_px
                    } else {
                        font_sys.metrics.font_px
                    };
                    let entry = atlas.get_or_rasterize(
                        font_sys,
                        glyph.font_id,
                        glyph.glyph_id,
                        fpx,
                        config.font_thicken,
                    );
                    if entry.width == 0 || entry.height == 0 {
                        continue;
                    }
                    let gx = (cursor_x + glyph.x + entry.left as f32).round();
                    let gy = (row_baseline - glyph.y_offset - entry.top as f32).round();
                    fg_instances.push(QuadInstance {
                        pos: [gx, gy],
                        size: [entry.width as f32, entry.height as f32],
                        uv_pos: [entry.uv_x, entry.uv_y],
                        uv_size: [entry.uv_w, entry.uv_h],
                        color: if entry.is_color {
                            [1.0, 1.0, 1.0, 1.0]
                        } else {
                            theme.foreground.to_f32_rgba(1.0)
                        },
                        kind: if entry.is_color { 2.0 } else { 1.0 },
                    });
                }
            }
        }

        // 5b. Interactive Scrollbar (Right edge)
        let total_lines = doc.buffer.len_lines();
        if total_lines > 0 && content_h > 20.0 {
            // Track background
            bg_instances.push(QuadInstance::solid(
                scrollbar_x,
                content_y,
                scrollbar_w,
                content_h,
                theme.background.to_f32_rgba(0.20),
            ));

            let visible_ratio = (visible_lines as f32 / total_lines as f32).clamp(0.04, 1.0);
            let thumb_h = (content_h * visible_ratio).clamp(24.0, content_h);
            let track_avail = (content_h - thumb_h).max(0.0);
            let max_scroll = total_lines.saturating_sub(visible_lines).max(1);
            let scroll_ratio = (doc.scroll_y as f32 / max_scroll as f32).clamp(0.0, 1.0);
            let thumb_y = content_y + scroll_ratio * track_avail;

            let thumb_alpha = if scrollbar_dragging {
                0.70
            } else if scrollbar_hovered {
                0.45
            } else {
                0.22
            };

            fg_instances.push(QuadInstance::solid(
                scrollbar_x + 1.0,
                thumb_y,
                scrollbar_w - 2.0,
                thumb_h,
                theme.foreground.to_f32_rgba(thumb_alpha),
            ));
        }

        // 6. Floating Find / Replace Bar
        if let Some(fb) = find_bar
            && fb.is_active
        {
            emit_find_bar(config, font_sys, atlas, overlay_instances, fb, vp);
        }

        // 7. Floating Autocompletion Popup
        if let Some(comp) = completion
            && !comp.is_empty()
        {
            let shaped = doc.shaped_line(cursor_line, font_sys);
            let visual_x = shaped.visual_x_for_cluster(cursor_cluster);
            let row_idx = cursor_line.saturating_sub(start_line);
            let cx = text_area_x - scroll_x + visual_x;
            let cy = content_y + (row_idx + 1) as f32 * line_h;

            emit_completion_popup(config, font_sys, atlas, overlay_instances, comp, cx, cy, vp);
        }

        // 8. Hover Documentation Popup
        if let Some(h) = hover {
            let shaped = doc.shaped_line(h.line, font_sys);
            let visual_x = shaped.visual_x_for_cluster(h.cluster_idx);
            let row_idx = h.line.saturating_sub(start_line);
            let cx = text_area_x - scroll_x + visual_x;
            let cy = content_y + row_idx as f32 * line_h;

            emit_hover_popup(config, font_sys, atlas, overlay_instances, h, cx, cy, vp);
        }

        // 9. Command Palette Modal (Ctrl+P)
        if let Some(cp) = palette {
            emit_command_palette(config, font_sys, atlas, overlay_instances, cp, vp);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_activity_bar(
        config: &Config,
        font_sys: &mut FontSystem,
        atlas: &mut GlyphAtlas,
        bg_instances: &mut Vec<QuadInstance>,
        fg_instances: &mut Vec<QuadInstance>,
        vp: &EditorViewport,
        width: f32,
        active_panel: crate::editor::SidebarPanel,
    ) {
        let theme = &config.colors;
        let line_h = font_sys.metrics.cell_height;
        let baseline = font_sys.metrics.baseline;

        bg_instances.push(QuadInstance::solid(
            vp.x,
            vp.y,
            width,
            vp.h,
            theme.background.to_f32_rgba(0.98),
        ));
        fg_instances.push(QuadInstance::solid(
            vp.x + width - 1.0,
            vp.y,
            1.0,
            vp.h,
            theme.foreground.to_f32_rgba(0.10),
        ));

        let active_explorer = active_panel == crate::editor::SidebarPanel::Explorer;
        let active_problems = active_panel == crate::editor::SidebarPanel::Problems;

        for index in 0..3 {
            let cy = vp.y + 14.0 + index as f32 * 44.0;
            let is_active = (index == 0 && active_explorer) || (index == 1 && active_problems);

            if is_active {
                bg_instances.push(QuadInstance::solid(
                    vp.x,
                    cy - 4.0,
                    width,
                    28.0,
                    theme.selection_background.to_f32_rgba(0.30),
                ));
                fg_instances.push(QuadInstance::solid(
                    vp.x,
                    cy - 4.0,
                    2.0,
                    28.0,
                    theme.cursor_color.to_f32_rgba(1.0),
                ));
            }

            let icon_color = theme
                .foreground
                .to_f32_rgba(if is_active { 0.95 } else { 0.55 });
            let ix = vp.x + 12.0;

            match index {
                0 => {
                    fg_instances.push(QuadInstance::hollow(ix, cy, 20.0, 15.0, icon_color));
                    fg_instances.push(QuadInstance::solid(ix, cy, 9.0, 3.0, icon_color));
                }
                1 => {
                    fg_instances.push(QuadInstance::hollow(ix, cy, 20.0, 18.0, icon_color));
                    emit_simple_string(
                        config,
                        font_sys,
                        atlas,
                        fg_instances,
                        "!",
                        ix + 7.0,
                        cy + baseline + 3.0,
                        icon_color,
                    );
                }
                _ => {
                    fg_instances.push(QuadInstance::hollow(ix + 1.0, cy, 14.0, 14.0, icon_color));
                    fg_instances.push(QuadInstance::solid(
                        ix + 12.0,
                        cy + 12.0,
                        8.0,
                        2.0,
                        icon_color,
                    ));
                }
            }

            let _ = line_h;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_problems(
        config: &Config,
        font_sys: &mut FontSystem,
        atlas: &mut GlyphAtlas,
        bg_instances: &mut Vec<QuadInstance>,
        fg_instances: &mut Vec<QuadInstance>,
        problems: &[crate::editor::ProblemEntry],
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        line_h: f32,
        baseline: f32,
    ) {
        let theme = &config.colors;
        let cell_w = font_sys.metrics.cell_width;
        let header_h = (line_h + 6.0).round();
        let item_h = (line_h + 3.0).round();

        bg_instances.push(QuadInstance::solid(
            x,
            y,
            width,
            height,
            theme.background.to_f32_rgba(0.97),
        ));
        fg_instances.push(QuadInstance::solid(
            x + width - 1.0,
            y,
            1.0,
            height,
            theme.foreground.to_f32_rgba(0.12),
        ));

        bg_instances.push(QuadInstance::solid(
            x,
            y,
            width,
            header_h,
            theme.selection_background.to_f32_rgba(0.20),
        ));
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &format!("PROBLEMS ({})", problems.len()),
            x + 10.0,
            y + (header_h - line_h) * 0.5 + baseline,
            theme.ansi[4].to_f32_rgba(1.0),
        );

        let max_chars = ((width - 30.0) / cell_w).floor().max(6.0) as usize;
        let visible_rows = ((height - header_h) / item_h).floor().max(0.0) as usize;

        for (index, problem) in problems.iter().take(visible_rows).enumerate() {
            let row_y = y + header_h + index as f32 * item_h;
            let color = match problem.severity {
                lsp_types::DiagnosticSeverity::ERROR => theme.ansi[1].to_f32_rgba(1.0),
                lsp_types::DiagnosticSeverity::WARNING => theme.ansi[3].to_f32_rgba(1.0),
                _ => theme.ansi[4].to_f32_rgba(1.0),
            };
            fg_instances.push(QuadInstance::solid(
                x + 8.0,
                row_y + (item_h - 6.0) * 0.5,
                6.0,
                6.0,
                color,
            ));

            let label = format!(
                "{}:{}  {}",
                problem.file_name,
                problem.line + 1,
                problem.message
            );
            let label: String = label.chars().take(max_chars).collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &label,
                x + 20.0,
                row_y + (item_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.78),
            );
        }
    }

    fn emit_sidebar(
        config: &Config,
        font_sys: &mut FontSystem,
        atlas: &mut GlyphAtlas,
        bg_instances: &mut Vec<QuadInstance>,
        fg_instances: &mut Vec<QuadInstance>,
        exp: &FileExplorer,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
    ) {
        let theme = &config.colors;
        let line_h = font_sys.metrics.cell_height;
        let cell_w = font_sys.metrics.cell_width;
        let baseline = font_sys.metrics.baseline;

        // 1. Sidebar background
        bg_instances.push(QuadInstance::solid(
            sx,
            sy,
            sw,
            sh,
            theme.background.to_f32_rgba(0.97),
        ));

        // 2. Vertical Divider between Sidebar and Code Viewport
        fg_instances.push(QuadInstance::solid(
            sx + sw - 1.0,
            sy,
            1.0,
            sh,
            theme.foreground.to_f32_rgba(0.12),
        ));

        // 3. Header: "EXPLORER: <ROOT_NAME>"
        let header_h = (line_h + 6.0).round();
        bg_instances.push(QuadInstance::solid(
            sx,
            sy,
            sw - 1.0,
            header_h,
            theme.selection_background.to_f32_rgba(0.20),
        ));
        fg_instances.push(QuadInstance::solid(
            sx,
            sy + header_h - 1.0,
            sw - 1.0,
            1.0,
            theme.foreground.to_f32_rgba(0.10),
        ));

        let header_title = format!("EXPLORER: {}", exp.root_name.to_uppercase());
        let max_hdr_chars = ((sw - 16.0) / cell_w).floor().max(1.0) as usize;
        let truncated_hdr: String = header_title.chars().take(max_hdr_chars).collect();
        let header_baseline = sy + (header_h - line_h) * 0.5 + baseline;

        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &truncated_hdr,
            sx + 8.0,
            header_baseline,
            theme.ansi[4].to_f32_rgba(1.0),
        );

        // 4. File Tree Entries
        let content_y = sy + header_h;
        let content_h = (sh - header_h).max(0.0);
        let item_h = (line_h + 3.0).round();

        let visible_items = (content_h / item_h).ceil() as usize + 1;
        let start_idx = (exp.scroll_y / item_h).floor() as usize;
        let end_idx = (start_idx + visible_items).min(exp.entries.len());

        for i in start_idx..end_idx {
            let entry = &exp.entries[i];
            let item_y = content_y + (i as f32 * item_h) - exp.scroll_y;
            if item_y + item_h < content_y || item_y >= sy + sh {
                continue;
            }

            let is_selected = exp.selected_path.as_ref() == Some(&entry.path);
            let is_hovered = exp.hovered_idx == Some(i);

            if is_selected {
                bg_instances.push(QuadInstance::solid(
                    sx + 2.0,
                    item_y,
                    sw - 4.0,
                    item_h,
                    theme.selection_background.to_f32_rgba(0.40),
                ));
                fg_instances.push(QuadInstance::solid(
                    sx + 2.0,
                    item_y,
                    2.0,
                    item_h,
                    theme.cursor_color.to_f32_rgba(1.0),
                ));
            } else if is_hovered {
                bg_instances.push(QuadInstance::solid(
                    sx + 2.0,
                    item_y,
                    sw - 4.0,
                    item_h,
                    theme.foreground.to_f32_rgba(0.06),
                ));
            }

            let indent_px = (entry.depth as f32 * 12.0).min(sw - 40.0);
            let prefix = if entry.is_dir {
                if entry.is_expanded { "v " } else { "> " }
            } else {
                "  "
            };

            let label = format!("{}{}", prefix, entry.name);
            let max_chars = ((sw - 12.0 - indent_px) / cell_w).floor().max(1.0) as usize;
            let truncated: String = label.chars().take(max_chars).collect();

            let item_baseline = item_y + (item_h - line_h) * 0.5 + baseline;
            let text_color = if entry.is_dir {
                theme.foreground.to_f32_rgba(1.0)
            } else if is_selected {
                theme.foreground.to_f32_rgba(1.0)
            } else {
                theme.foreground.to_f32_rgba(0.78)
            };

            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &truncated,
                sx + 6.0 + indent_px,
                item_baseline,
                text_color,
            );
        }
    }
}

fn emit_command_palette(
    config: &Config,
    font_sys: &mut FontSystem,
    atlas: &mut GlyphAtlas,
    overlays: &mut Vec<QuadInstance>,
    cp: &CommandPalette,
    vp: &EditorViewport,
) {
    let theme = &config.colors;
    let line_h = font_sys.metrics.cell_height;
    let baseline = font_sys.metrics.baseline;

    // Dim the editor behind the modal palette.
    overlays.push(QuadInstance::solid(
        vp.x,
        vp.y,
        vp.w,
        vp.h,
        [0.0, 0.0, 0.0, 0.35],
    ));

    let pw = (640.0_f32).min(vp.w - 40.0);
    let px = (vp.x + (vp.w - pw) * 0.5).round();
    let py = (vp.y + (vp.h * 0.10).clamp(24.0, 80.0)).round();
    let ph = (340.0_f32).min(vp.h - (py - vp.y) - 20.0).max(120.0);

    // Subtle drop shadow
    overlays.push(QuadInstance::solid(
        px + 6.0,
        py + 6.0,
        pw,
        ph,
        [0.0, 0.0, 0.0, 0.40],
    ));

    // Modal background
    overlays.push(QuadInstance::solid(
        px,
        py,
        pw,
        ph,
        theme.background.to_f32_rgba(0.98),
    ));

    // Border
    overlays.push(QuadInstance::hollow(
        px,
        py,
        pw,
        ph,
        theme.foreground.to_f32_rgba(0.25),
    ));

    // Input search box
    let input_h = (line_h + 12.0).round();
    overlays.push(QuadInstance::solid(
        px + 10.0,
        py + 10.0,
        pw - 20.0,
        input_h,
        theme.selection_background.to_f32_rgba(0.20),
    ));
    overlays.push(QuadInstance::hollow(
        px + 10.0,
        py + 10.0,
        pw - 20.0,
        input_h,
        theme.cursor_color.to_f32_rgba(0.50),
    ));

    let prompt = format!("> {}|", cp.query);
    emit_simple_string(
        config,
        font_sys,
        atlas,
        overlays,
        &prompt,
        px + 18.0,
        py + 10.0 + (input_h - line_h) * 0.5 + baseline,
        theme.foreground.to_f32_rgba(1.0),
    );

    // Results list
    let list_y = py + input_h + 16.0;
    let item_h = (line_h + 8.0).round();
    let max_items = ((ph - input_h - 26.0) / item_h).floor() as usize;

    for (display_idx, (item_idx, _)) in cp.filtered_indices.iter().take(max_items).enumerate() {
        let is_selected = display_idx == cp.selected;
        let iy = list_y + display_idx as f32 * item_h;
        let item = &cp.all_items[*item_idx];

        if is_selected {
            overlays.push(QuadInstance::solid(
                px + 8.0,
                iy,
                pw - 16.0,
                item_h,
                theme.selection_background.to_f32_rgba(0.35),
            ));
        }

        emit_simple_string(
            config,
            font_sys,
            atlas,
            overlays,
            &item.title,
            px + 16.0,
            iy + (item_h - line_h) * 0.5 + baseline,
            if is_selected {
                theme.cursor_color.to_f32_rgba(1.0)
            } else {
                theme.foreground.to_f32_rgba(0.90)
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_completion_popup(
    config: &Config,
    font_sys: &mut FontSystem,
    atlas: &mut GlyphAtlas,
    overlays: &mut Vec<QuadInstance>,
    comp: &CompletionOverlay,
    cx: f32,
    cy: f32,
    vp: &EditorViewport,
) {
    let theme = &config.colors;
    let line_h = font_sys.metrics.cell_height;
    let baseline = font_sys.metrics.baseline;

    let pw = 280.0_f32;
    let item_h = (line_h + 4.0).round();
    let ph = ((comp.items.len().min(8) as f32) * item_h + 6.0).max(30.0);

    let px = cx.min(vp.x + vp.w - pw - 10.0).max(vp.x + 10.0);
    let py = if cy + ph > vp.y + vp.h {
        (cy - ph - line_h - 4.0).max(vp.y + 10.0)
    } else {
        cy + 4.0
    };

    // Shadow & background
    overlays.push(QuadInstance::solid(
        px + 4.0,
        py + 4.0,
        pw,
        ph,
        [0.0, 0.0, 0.0, 0.35],
    ));
    overlays.push(QuadInstance::solid(
        px,
        py,
        pw,
        ph,
        theme.background.to_f32_rgba(0.98),
    ));
    overlays.push(QuadInstance::hollow(
        px,
        py,
        pw,
        ph,
        theme.foreground.to_f32_rgba(0.20),
    ));

    for (i, item) in comp.items.iter().take(8).enumerate() {
        let is_selected = i == comp.selected;
        let iy = py + 3.0 + i as f32 * item_h;

        if is_selected {
            overlays.push(QuadInstance::solid(
                px + 3.0,
                iy,
                pw - 6.0,
                item_h,
                theme.selection_background.to_f32_rgba(0.40),
            ));
        }

        let label_str = format!(" {} ", item.label);
        emit_simple_string(
            config,
            font_sys,
            atlas,
            overlays,
            &label_str,
            px + 6.0,
            iy + (item_h - line_h) * 0.5 + baseline,
            if is_selected {
                theme.cursor_color.to_f32_rgba(1.0)
            } else {
                theme.foreground.to_f32_rgba(0.90)
            },
        );

        if let Some(detail) = &item.detail {
            let det_str = detail.chars().take(16).collect::<String>();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                overlays,
                &det_str,
                px + pw - (det_str.len() as f32 * font_sys.metrics.cell_width) - 10.0,
                iy + (item_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.50),
            );
        }
    }
}

fn emit_find_bar(
    config: &Config,
    font_sys: &mut FontSystem,
    atlas: &mut GlyphAtlas,
    overlays: &mut Vec<QuadInstance>,
    fb: &FindBar,
    vp: &EditorViewport,
) {
    let theme = &config.colors;
    let line_h = font_sys.metrics.cell_height;
    let baseline = font_sys.metrics.baseline;

    let fw = 320.0_f32;
    let fh = (line_h + 12.0).round();
    let fx = (vp.x + vp.w - fw - 20.0).round();
    let fy = (vp.y + 36.0).round();

    overlays.push(QuadInstance::solid(
        fx + 3.0,
        fy + 3.0,
        fw,
        fh,
        [0.0, 0.0, 0.0, 0.30],
    ));
    overlays.push(QuadInstance::solid(
        fx,
        fy,
        fw,
        fh,
        theme.background.to_f32_rgba(0.97),
    ));
    overlays.push(QuadInstance::hollow(
        fx,
        fy,
        fw,
        fh,
        theme.cursor_color.to_f32_rgba(0.60),
    ));

    let text = format!("Find: {}|  ({} matches)", fb.query, fb.match_count);
    emit_simple_string(
        config,
        font_sys,
        atlas,
        overlays,
        &text,
        fx + 10.0,
        fy + (fh - line_h) * 0.5 + baseline,
        theme.foreground.to_f32_rgba(0.95),
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_hover_popup(
    config: &Config,
    font_sys: &mut FontSystem,
    atlas: &mut GlyphAtlas,
    overlays: &mut Vec<QuadInstance>,
    hover: &HoverOverlay,
    cx: f32,
    cy: f32,
    vp: &EditorViewport,
) {
    let theme = &config.colors;
    let line_h = font_sys.metrics.cell_height;
    let baseline = font_sys.metrics.baseline;

    let lines: Vec<&str> = hover.text.lines().collect();
    let hw = (400.0_f32).min(vp.w - 40.0);
    let hh = ((lines.len() as f32 * line_h) + 16.0).min(200.0);

    let hx = cx.min(vp.x + vp.w - hw - 10.0).max(vp.x + 10.0);
    let hy = if cy - hh - 4.0 >= vp.y {
        cy - hh - 4.0
    } else {
        cy + line_h + 4.0
    };

    overlays.push(QuadInstance::solid(
        hx + 4.0,
        hy + 4.0,
        hw,
        hh,
        [0.0, 0.0, 0.0, 0.35],
    ));
    overlays.push(QuadInstance::solid(
        hx,
        hy,
        hw,
        hh,
        theme.background.to_f32_rgba(0.98),
    ));
    overlays.push(QuadInstance::hollow(
        hx,
        hy,
        hw,
        hh,
        theme.foreground.to_f32_rgba(0.20),
    ));

    for (i, l) in lines.iter().take(8).enumerate() {
        emit_simple_string(
            config,
            font_sys,
            atlas,
            overlays,
            l,
            hx + 8.0,
            hy + 8.0 + (i as f32 * line_h) + baseline,
            theme.foreground.to_f32_rgba(0.90),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_simple_string(
    config: &Config,
    font_sys: &mut FontSystem,
    atlas: &mut GlyphAtlas,
    instances: &mut Vec<QuadInstance>,
    text: &str,
    start_x: f32,
    baseline_y: f32,
    color: [f32; 4],
) {
    let mut cur_x = start_x;
    for ch in text.chars() {
        let font_id = font_sys.resolve_font_for_char(ch, FontStyle::Regular);
        let gid = font_sys.font(font_id).glyph_id(ch);
        if gid != 0 {
            let entry = atlas.get_or_rasterize(
                font_sys,
                font_id,
                gid,
                font_sys.metrics.font_px,
                config.font_thicken,
            );
            if entry.width > 0 && entry.height > 0 {
                let gx = (cur_x + entry.left as f32).round();
                let gy = (baseline_y - entry.top as f32).round();
                instances.push(QuadInstance {
                    pos: [gx, gy],
                    size: [entry.width as f32, entry.height as f32],
                    uv_pos: [entry.uv_x, entry.uv_y],
                    uv_size: [entry.uv_w, entry.uv_h],
                    color,
                    kind: if entry.is_color { 2.0 } else { 1.0 },
                });
            }
        }
        cur_x += font_sys.metrics.cell_width;
    }
}
