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
use crate::renderer::icons::{self, IconId};

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
        plugins: &[crate::plugins::PluginInfo],
        search: &crate::editor::WorkspaceSearch,
        git: &crate::editor::git::GitStatus,
        active_panel: crate::editor::SidebarPanel,
        activity_bar_visible: bool,
        vim: Option<&VimState>,
        ime_preedit: Option<&str>,
        ui_scale: f32,
        menu_height: f32,
        metrics_label: Option<&str>,
    ) {
        let ui = ui_scale.max(0.5);
        if raw_vp.w <= 10.0 || raw_vp.h <= 10.0 {
            return;
        }
        let menu_height = menu_height.clamp(0.0, (raw_vp.h - 20.0).max(0.0));
        let body_vp = EditorViewport {
            x: raw_vp.x,
            y: raw_vp.y + menu_height,
            w: raw_vp.w,
            h: (raw_vp.h - menu_height).max(10.0),
        };
        let raw_vp = &body_vp;

        let theme = &config.colors;
        let line_h = font_sys.metrics.cell_height;
        let cell_w = font_sys.metrics.cell_width;
        let baseline = font_sys.metrics.baseline;

        let activity_w = if activity_bar_visible { 44.0 * ui } else { 0.0 };
        let mut panel_open = false;
        let mut sidebar_w = 0.0;

        if let Some(exp) = explorer {
            panel_open = exp.visible
                && (active_panel != crate::editor::SidebarPanel::Explorer || exp.is_open());
            if panel_open {
                sidebar_w = match active_panel {
                    crate::editor::SidebarPanel::Search => {
                        exp.width.max(280.0).min((raw_vp.w - 160.0).max(80.0))
                    }
                    crate::editor::SidebarPanel::Problems
                    | crate::editor::SidebarPanel::Plugins
                    | crate::editor::SidebarPanel::Git => {
                        exp.width.max(240.0).min((raw_vp.w - 160.0).max(80.0))
                    }
                    crate::editor::SidebarPanel::Explorer => {
                        exp.width.min(raw_vp.w - 100.0).max(80.0)
                    }
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
                ui,
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
                            ui,
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
                        ui,
                    );
                }
                crate::editor::SidebarPanel::Search => {
                    Self::emit_search(
                        config,
                        font_sys,
                        atlas,
                        bg_instances,
                        fg_instances,
                        search,
                        sidebar_x,
                        raw_vp.y,
                        sidebar_w,
                        raw_vp.h,
                        line_h,
                        baseline,
                        ui,
                    );
                }
                crate::editor::SidebarPanel::Plugins => {
                    Self::emit_plugins(
                        config,
                        font_sys,
                        atlas,
                        bg_instances,
                        fg_instances,
                        plugins,
                        sidebar_x,
                        raw_vp.y,
                        sidebar_w,
                        raw_vp.h,
                        line_h,
                        baseline,
                        ui,
                    );
                }
                crate::editor::SidebarPanel::Git => {
                    Self::emit_git(
                        config,
                        font_sys,
                        atlas,
                        bg_instances,
                        fg_instances,
                        git,
                        sidebar_x,
                        raw_vp.y,
                        sidebar_w,
                        raw_vp.h,
                        line_h,
                        baseline,
                        ui,
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
        let max_title_chars = (((tab_w - 48.0 * ui) / cell_w).floor() as usize).max(4);

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

            let icon_size = 13.0 * ui;
            let file_icon = icons::file_icon_for_name(name);
            let file_icon_color =
                icons::icon_color(file_icon, theme, if is_active { 0.95 } else { 0.55 });
            icons::emit(
                font_sys,
                atlas,
                fg_instances,
                file_icon,
                tab_x + 7.0 * ui,
                vp.y + (tab_h - icon_size) * 0.5,
                icon_size,
                file_icon_color,
            );

            let title: String = name.chars().take(max_title_chars).collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &title,
                tab_x + 24.0 * ui,
                vp.y + 2.0 + (tab_h - line_h) * 0.5 + baseline,
                if is_active {
                    theme.foreground.to_f32_rgba(1.0)
                } else {
                    theme.foreground.to_f32_rgba(0.60)
                },
            );

            if *is_dirty {
                icons::emit(
                    font_sys,
                    atlas,
                    fg_instances,
                    IconId::Dirty,
                    tab_x + tab_w - 31.0 * ui,
                    vp.y + (tab_h - 7.0 * ui) * 0.5,
                    7.0 * ui,
                    theme.ansi[3].to_f32_rgba(0.95),
                );
            }

            icons::emit(
                font_sys,
                atlas,
                fg_instances,
                IconId::Close,
                tab_x + tab_w - 17.0 * ui,
                vp.y + (tab_h - 11.0 * ui) * 0.5,
                11.0 * ui,
                theme
                    .foreground
                    .to_f32_rgba(if is_active { 0.72 } else { 0.32 }),
            );

            tab_x += tab_w + 4.0;
        }

        // New tab button
        if tab_x + 22.0 * ui < vp.x + vp.w - 8.0 {
            let button_w = tab_h - 4.0 * ui;
            bg_instances.push(QuadInstance::solid(
                tab_x,
                vp.y + 3.0 * ui,
                button_w,
                tab_h - 5.0 * ui,
                theme.foreground.to_f32_rgba(0.06),
            ));
            icons::emit(
                font_sys,
                atlas,
                fg_instances,
                IconId::Plus,
                tab_x + (button_w - 11.0 * ui) * 0.5,
                vp.y + (tab_h - 11.0 * ui) * 0.5,
                11.0 * ui,
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

            let crumb_icon = icons::file_icon_for_name(&doc.file_name());
            let crumb_color = icons::icon_color(crumb_icon, theme, 0.65);
            icons::emit(
                font_sys,
                atlas,
                fg_instances,
                crumb_icon,
                vp.x + 8.0 * ui,
                crumb_y + (crumb_h - 12.0 * ui) * 0.5,
                12.0 * ui,
                crumb_color,
            );
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &crumbs,
                vp.x + 25.0 * ui,
                crumb_y + (crumb_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.62),
            );
        }

        // 3. Status Bar (bottom of editor view)
        const STATUS_TEXT_SCALE: f32 = 0.80;
        let status_text_h = line_h * STATUS_TEXT_SCALE;
        let status_h = (status_text_h + 4.0).round();
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
        let status_baseline =
            status_y + (status_h - status_text_h) * 0.5 + baseline * STATUS_TEXT_SCALE;
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
        emit_simple_string_scaled(
            config,
            font_sys,
            atlas,
            fg_instances,
            &mode_text,
            vp.x + 8.0,
            status_baseline,
            mode_color.to_f32_rgba(1.0),
            STATUS_TEXT_SCALE,
        );

        let file_info = format!("{}  {} lines", doc.file_name(), doc.buffer.len_lines());
        let file_info_x =
            vp.x + 12.0 + (mode_text.chars().count() as f32 + 1.0) * cell_w * STATUS_TEXT_SCALE;
        emit_simple_string_scaled(
            config,
            font_sys,
            atlas,
            fg_instances,
            &file_info,
            file_info_x,
            status_baseline,
            theme.foreground.to_f32_rgba(0.85),
            STATUS_TEXT_SCALE,
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
        if git.is_repo {
            let branch = git.branch.as_deref().unwrap_or("git");
            right_info.push_str(&format!("  |  {} ({})", branch, git.changes.len()));
        }
        if doc.caret_count() > 1 {
            right_info.push_str(&format!("  |  {} cursors", doc.caret_count()));
        }
        for item in plugin_status_items {
            right_info.push_str(&format!("  |  {item}"));
        }
        if let Some(metrics_txt) = metrics_label {
            right_info.push_str(&format!("  |  {metrics_txt}"));
        }
        if let Some(vim) = vim
            && let Some((message, at)) = &vim.status_message
            && at.elapsed().as_secs() < 5
        {
            right_info.push_str(&format!("  |  {message}"));
        }

        let right_w = right_info.chars().count() as f32 * cell_w * STATUS_TEXT_SCALE;
        emit_simple_string_scaled(
            config,
            font_sys,
            atlas,
            fg_instances,
            &right_info,
            (vp.x + vp.w - right_w - 12.0).max(vp.x + 200.0),
            status_baseline,
            theme.foreground.to_f32_rgba(0.70),
            STATUS_TEXT_SCALE,
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

        let git_line_changes = doc.path().map(|path| git.line_changes(path)).unwrap_or(&[]);
        let blame_lines = doc.path().and_then(|path| git.blame_for(path));

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
                let (icon, dot_col) = match diag.severity {
                    lsp_types::DiagnosticSeverity::ERROR => (IconId::Error, theme.ansi[1]),
                    lsp_types::DiagnosticSeverity::WARNING => (IconId::Warning, theme.ansi[3]),
                    _ => (IconId::Info, theme.ansi[4]),
                };
                icons::emit(
                    font_sys,
                    atlas,
                    fg_instances,
                    icon,
                    vp.x + 1.0 * ui,
                    row_y + (line_h - 10.0 * ui) * 0.5,
                    10.0 * ui,
                    dot_col.to_f32_rgba(1.0),
                );
            }

            if let Some(change) = git_line_changes
                .iter()
                .find(|change| change.line == line_idx)
            {
                let color = match change.kind {
                    crate::editor::git::GitChangeKind::Added
                    | crate::editor::git::GitChangeKind::Untracked => theme.ansi[2],
                    crate::editor::git::GitChangeKind::Deleted
                    | crate::editor::git::GitChangeKind::Conflict => theme.ansi[1],
                    crate::editor::git::GitChangeKind::Renamed => theme.ansi[4],
                    crate::editor::git::GitChangeKind::Modified => theme.ansi[3],
                };
                fg_instances.push(QuadInstance::solid(
                    (vp.x + gutter_w - 4.0 * ui).round(),
                    row_y,
                    2.0 * ui,
                    line_h,
                    color.to_f32_rgba(0.95),
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

            if line_idx == cursor_line
                && let Some(blame) = blame_lines.and_then(|lines| lines.get(line_idx))
            {
                let label = crate::editor::git::format_blame(blame);
                let end_x = text_area_x - scroll_x
                    + shaped.visual_x_for_cluster(shaped.cluster_count())
                    + 2.0 * cell_w;
                let available = ((scrollbar_x - end_x) / cell_w).floor().max(0.0) as usize;
                if available >= 12 {
                    let label: String = label.chars().take(available - 1).collect();
                    emit_simple_string(
                        config,
                        font_sys,
                        atlas,
                        fg_instances,
                        &label,
                        end_x.round(),
                        row_baseline,
                        theme.foreground.to_f32_rgba(0.30),
                    );
                }
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
        if is_focused {
            for caret in doc.extra_cursors.clone() {
                if caret.line < start_line || caret.line >= end_line {
                    continue;
                }
                let row_y = content_y + (caret.line - start_line) as f32 * line_h;
                let shaped = doc.shaped_line(caret.line, font_sys);
                let cx = text_area_x - scroll_x + shaped.visual_x_for_cluster(caret.cluster_idx);
                if cx >= text_area_x && cx <= scrollbar_x {
                    fg_instances.push(QuadInstance::solid(
                        cx.round(),
                        row_y,
                        2.0,
                        line_h,
                        theme.cursor_color.to_f32_rgba(0.7),
                    ));
                }
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
        ui: f32,
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
        let active_search = active_panel == crate::editor::SidebarPanel::Search;
        let active_problems = active_panel == crate::editor::SidebarPanel::Problems;
        let active_plugins = active_panel == crate::editor::SidebarPanel::Plugins;
        let active_git = active_panel == crate::editor::SidebarPanel::Git;

        let items = [
            IconId::Explorer,
            IconId::Search,
            IconId::Problems,
            IconId::Plugins,
            IconId::Git,
        ];
        for (index, icon) in items.iter().enumerate() {
            let cy = vp.y + 13.0 * ui + index as f32 * 44.0 * ui;
            let is_active = (index == 0 && active_explorer)
                || (index == 1 && active_search)
                || (index == 2 && active_problems)
                || (index == 3 && active_plugins)
                || (index == 4 && active_git);

            if is_active {
                bg_instances.push(QuadInstance::solid(
                    vp.x,
                    cy - 5.0 * ui,
                    width,
                    28.0 * ui,
                    theme.selection_background.to_f32_rgba(0.30),
                ));
                fg_instances.push(QuadInstance::solid(
                    vp.x,
                    cy - 5.0 * ui,
                    2.0 * ui,
                    28.0 * ui,
                    theme.cursor_color.to_f32_rgba(1.0),
                ));
            }

            let icon_color = theme
                .foreground
                .to_f32_rgba(if is_active { 0.95 } else { 0.55 });
            icons::emit(
                font_sys,
                atlas,
                fg_instances,
                *icon,
                vp.x + 13.0 * ui,
                cy,
                18.0 * ui,
                icon_color,
            );
        }
        let _ = (line_h, baseline);
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
        ui: f32,
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
            let (icon, color) = match problem.severity {
                lsp_types::DiagnosticSeverity::ERROR => (IconId::Error, theme.ansi[1]),
                lsp_types::DiagnosticSeverity::WARNING => (IconId::Warning, theme.ansi[3]),
                _ => (IconId::Info, theme.ansi[4]),
            };
            icons::emit(
                font_sys,
                atlas,
                fg_instances,
                icon,
                x + 7.0 * ui,
                row_y + (item_h - 10.0 * ui) * 0.5,
                10.0 * ui,
                color.to_f32_rgba(1.0),
            );

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

    #[allow(clippy::too_many_arguments)]
    fn emit_search(
        config: &Config,
        font_sys: &mut FontSystem,
        atlas: &mut GlyphAtlas,
        bg_instances: &mut Vec<QuadInstance>,
        fg_instances: &mut Vec<QuadInstance>,
        search: &crate::editor::WorkspaceSearch,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        line_h: f32,
        baseline: f32,
        ui: f32,
    ) {
        let theme = &config.colors;
        let cell_w = font_sys.metrics.cell_width;
        let header_h = (line_h + 6.0).round();
        let field_h = (line_h + 4.0).round();
        let item_h = (line_h + 3.0).round();
        let field_baseline = |row_y: f32| row_y + (field_h - line_h) * 0.5 + baseline;

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
            &format!("SEARCH ({})", search.results.len()),
            x + 10.0,
            y + (header_h - line_h) * 0.5 + baseline,
            theme.ansi[4].to_f32_rgba(1.0),
        );

        let max_chars = ((width - 20.0 * ui) / cell_w).floor().max(6.0) as usize;
        let query_y = y + header_h;
        let replace_y = query_y + field_h;
        let options_y = replace_y + field_h;

        if search.field == 0 && search.active {
            bg_instances.push(QuadInstance::solid(
                x,
                query_y,
                width,
                field_h,
                theme.selection_background.to_f32_rgba(0.28),
            ));
        }
        if search.field == 1 && search.active {
            bg_instances.push(QuadInstance::solid(
                x,
                replace_y,
                width,
                field_h,
                theme.selection_background.to_f32_rgba(0.28),
            ));
        }

        let query_label = format!(
            "Find: {}{}",
            search.query,
            if search.field == 0 && search.active {
                "|"
            } else {
                ""
            }
        );
        let query_label: String = query_label.chars().take(max_chars).collect();
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &query_label,
            x + 8.0,
            field_baseline(query_y),
            theme.foreground.to_f32_rgba(0.90),
        );

        let replace_label = format!(
            "Replace: {}{}",
            search.replace,
            if search.field == 1 && search.active {
                "|"
            } else {
                ""
            }
        );
        let replace_label: String = replace_label.chars().take(max_chars).collect();
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &replace_label,
            x + 8.0,
            field_baseline(replace_y),
            theme.foreground.to_f32_rgba(0.78),
        );

        let case_color = if search.case_sensitive {
            theme.ansi[2].to_f32_rgba(1.0)
        } else {
            theme.foreground.to_f32_rgba(0.45)
        };
        let word_color = if search.whole_word {
            theme.ansi[2].to_f32_rgba(1.0)
        } else {
            theme.foreground.to_f32_rgba(0.45)
        };
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            "[Aa]",
            x + 8.0,
            field_baseline(options_y),
            case_color,
        );
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            "[W]",
            x + 46.0,
            field_baseline(options_y),
            word_color,
        );

        let results_y = options_y + field_h;
        let visible_rows = ((height - (results_y - y) - field_h) / item_h)
            .floor()
            .max(0.0) as usize;
        for (index, hit) in search.results.iter().take(visible_rows).enumerate() {
            let row_y = results_y + index as f32 * item_h;
            if index == search.selected {
                bg_instances.push(QuadInstance::solid(
                    x,
                    row_y,
                    width,
                    item_h,
                    theme.selection_background.to_f32_rgba(0.32),
                ));
            }
            let file = hit
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?");
            let label = format!("{}:{}  {}", file, hit.line + 1, hit.preview);
            let label: String = label.chars().take(max_chars).collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &label,
                x + 8.0,
                row_y + (item_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.76),
            );
        }

        let message = search
            .message
            .as_deref()
            .unwrap_or("Enter search · Tab field · Ctrl+Enter replace all");
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            message,
            x + 8.0,
            y + height - (line_h + 3.0 * ui),
            theme.foreground.to_f32_rgba(0.44),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_git(
        config: &Config,
        font_sys: &mut FontSystem,
        atlas: &mut GlyphAtlas,
        bg_instances: &mut Vec<QuadInstance>,
        fg_instances: &mut Vec<QuadInstance>,
        git: &crate::editor::git::GitStatus,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        line_h: f32,
        baseline: f32,
        ui: f32,
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
        fg_instances.push(QuadInstance::solid(
            x,
            y + header_h - 1.0,
            width,
            1.0,
            theme.foreground.to_f32_rgba(0.10),
        ));

        let title = if git.is_repo {
            format!("SOURCE CONTROL ({})", git.changes.len())
        } else {
            "SOURCE CONTROL".to_string()
        };
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &title,
            x + 8.0,
            y + (header_h - line_h) * 0.5 + baseline,
            theme.ansi[4].to_f32_rgba(1.0),
        );

        let mut row_y = y + header_h + 3.0;
        if !git.is_repo {
            let message = git.error.as_deref().unwrap_or("Not a git repository");
            let message: String = message
                .chars()
                .take(((width - 16.0) / cell_w) as usize)
                .collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &message,
                x + 8.0,
                row_y + (item_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.48),
            );
            return;
        }

        if let Some(branch) = &git.branch {
            let branch: String = format!("⎇ {branch}")
                .chars()
                .take(((width - 16.0) / cell_w).floor().max(4.0) as usize)
                .collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &branch,
                x + 8.0,
                row_y + (item_h - line_h) * 0.5 + baseline,
                theme.ansi[2].to_f32_rgba(0.90),
            );
            row_y += item_h;
        }

        if let Some(error) = &git.error {
            let message: String = error
                .chars()
                .take(((width - 16.0) / cell_w).floor().max(4.0) as usize)
                .collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &message,
                x + 8.0,
                row_y + (item_h - line_h) * 0.5 + baseline,
                theme.ansi[1].to_f32_rgba(0.85),
            );
            row_y += item_h;
        }

        if git.changes.is_empty() {
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                "Working tree clean",
                x + 8.0,
                row_y + (item_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.48),
            );
        }

        let max_chars = ((width - 30.0 * ui) / cell_w).floor().max(6.0) as usize;
        for change in git
            .changes
            .iter()
            .take(((height - row_y + y) / item_h) as usize)
        {
            let color = match change.kind {
                crate::editor::git::GitChangeKind::Added
                | crate::editor::git::GitChangeKind::Untracked => theme.ansi[2],
                crate::editor::git::GitChangeKind::Deleted
                | crate::editor::git::GitChangeKind::Conflict => theme.ansi[1],
                crate::editor::git::GitChangeKind::Renamed => theme.ansi[4],
                crate::editor::git::GitChangeKind::Modified => theme.ansi[3],
            };
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &change.kind.letter().to_string(),
                x + 8.0,
                row_y + (item_h - line_h) * 0.5 + baseline,
                color.to_f32_rgba(1.0),
            );
            let name = change
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&change.relative);
            let label: String = name.chars().take(max_chars).collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &label,
                x + 8.0 + 2.0 * cell_w,
                row_y + (item_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.88),
            );
            if change.staged {
                fg_instances.push(QuadInstance::solid(
                    (x + width - 9.0 * ui).round(),
                    row_y + (item_h - 5.0 * ui) * 0.5,
                    5.0 * ui,
                    5.0 * ui,
                    theme.ansi[2].to_f32_rgba(0.90),
                ));
            }
            row_y += item_h;
        }

        let footer: String = "Ctrl+Shift+G refresh · click to open"
            .chars()
            .take(((width - 16.0) / cell_w).floor().max(4.0) as usize)
            .collect();
        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            &footer,
            x + 8.0,
            y + height - (line_h + 3.0 * ui),
            theme.foreground.to_f32_rgba(0.40),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_plugins(
        config: &Config,
        font_sys: &mut FontSystem,
        atlas: &mut GlyphAtlas,
        bg_instances: &mut Vec<QuadInstance>,
        fg_instances: &mut Vec<QuadInstance>,
        plugins: &[crate::plugins::PluginInfo],
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        line_h: f32,
        baseline: f32,
        ui: f32,
    ) {
        let theme = &config.colors;
        let cell_w = font_sys.metrics.cell_width;
        let header_h = (line_h + 6.0).round();
        let item_h = (line_h + 4.0).round();

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
            &format!("PLUGINS ({})", plugins.len()),
            x + 10.0,
            y + (header_h - line_h) * 0.5 + baseline,
            theme.ansi[4].to_f32_rgba(1.0),
        );

        let max_chars = ((width - 44.0 * ui) / cell_w).floor().max(6.0) as usize;
        let visible_rows = ((height - header_h - item_h) / item_h).floor().max(0.0) as usize;

        for (index, plugin) in plugins.iter().take(visible_rows).enumerate() {
            let row_y = y + header_h + index as f32 * item_h;
            let color = if plugin.enabled {
                theme.ansi[2]
            } else {
                theme.foreground
            };
            icons::emit(
                font_sys,
                atlas,
                fg_instances,
                IconId::Plugins,
                x + 7.0 * ui,
                row_y + (item_h - 12.0 * ui) * 0.5,
                12.0 * ui,
                color.to_f32_rgba(if plugin.enabled { 0.95 } else { 0.42 }),
            );
            let version = if plugin.version.is_empty() {
                String::new()
            } else {
                format!("v{}", plugin.version)
            };
            let state = if plugin.enabled { "on" } else { "off" };
            let label = format!("{}  {}  [{}]", plugin.name, version, state);
            let label: String = label.chars().take(max_chars).collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                fg_instances,
                &label,
                x + 24.0 * ui,
                row_y + (item_h - line_h) * 0.5 + baseline,
                color.to_f32_rgba(if plugin.enabled { 0.88 } else { 0.48 }),
            );
        }

        emit_simple_string(
            config,
            font_sys,
            atlas,
            fg_instances,
            "ocean +plugin-install <path|url>",
            x + 8.0 * ui,
            y + height - (line_h + 4.0 * ui),
            theme.foreground.to_f32_rgba(0.40),
        );
    }

    #[allow(clippy::too_many_arguments)]
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
        ui: f32,
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

            let indent_px = (entry.depth as f32 * 12.0 * ui).min(sw - 40.0);
            let icon_size = 12.0 * ui;

            if entry.is_dir {
                icons::emit(
                    font_sys,
                    atlas,
                    fg_instances,
                    if entry.is_expanded {
                        IconId::ChevronDown
                    } else {
                        IconId::ChevronRight
                    },
                    sx + 5.0 * ui + indent_px,
                    item_y + (item_h - 9.0 * ui) * 0.5,
                    9.0 * ui,
                    theme.foreground.to_f32_rgba(0.55),
                );
            }
            let file_icon = icons::file_icon_for(&entry.path, entry.is_dir, entry.is_expanded);
            let file_icon_color = icons::icon_color(file_icon, theme, 0.9);
            icons::emit(
                font_sys,
                atlas,
                fg_instances,
                file_icon,
                sx + 15.0 * ui + indent_px,
                item_y + (item_h - icon_size) * 0.5,
                icon_size,
                file_icon_color,
            );

            let max_chars = ((sw - 30.0 * ui - indent_px) / cell_w).floor().max(1.0) as usize;
            let truncated: String = entry.name.chars().take(max_chars).collect();

            let item_baseline = item_y + (item_h - line_h) * 0.5 + baseline;
            let text_color = if entry.is_dir || is_selected {
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
                sx + 30.0 * ui + indent_px,
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
    let cell_w = font_sys.metrics.cell_width;
    let baseline = font_sys.metrics.baseline;

    // Neovim-style bottom command line: results sit above a `:` prompt.
    let pw = (vp.w - 32.0).max(240.0);
    let px = (vp.x + (vp.w - pw) * 0.5).round();
    let prompt_h = (line_h + 8.0).round();
    let item_h = (line_h + 4.0).round();
    let max_items = (((vp.h * 0.45) / item_h).floor() as usize).clamp(3, 12);
    let visible_rows = if cp.filtered_indices.is_empty() {
        1
    } else {
        cp.filtered_indices.len().min(max_items)
    };
    let ph = (prompt_h + item_h * visible_rows as f32 + 6.0).round();
    let py = (vp.y + vp.h - ph - 6.0).max(vp.y + 6.0);

    overlays.push(QuadInstance::solid(
        px,
        py,
        pw,
        ph,
        theme.background.to_f32_rgba(0.97),
    ));
    overlays.push(QuadInstance::solid(
        px,
        py,
        pw,
        1.0,
        theme.cursor_color.to_f32_rgba(0.50),
    ));

    let offset = cp.selected.saturating_sub(max_items.saturating_sub(1));
    let list_y = (py + 3.0).round();
    let title_cols = ((pw * 0.52) / cell_w).floor().max(12.0) as usize;

    for (visible_idx, (item_idx, _)) in cp
        .filtered_indices
        .iter()
        .skip(offset)
        .take(max_items)
        .enumerate()
    {
        let display_idx = offset + visible_idx;
        let is_selected = display_idx == cp.selected;
        let iy = list_y + visible_idx as f32 * item_h;
        let item = &cp.all_items[*item_idx];

        if is_selected {
            overlays.push(QuadInstance::solid(
                px,
                iy,
                pw,
                item_h,
                theme.selection_background.to_f32_rgba(0.34),
            ));
            overlays.push(QuadInstance::solid(
                px,
                iy,
                2.0,
                item_h,
                theme.cursor_color.to_f32_rgba(1.0),
            ));
        }

        let title: String = item.title.chars().take(title_cols).collect();
        emit_simple_string(
            config,
            font_sys,
            atlas,
            overlays,
            &title,
            px + 12.0,
            iy + (item_h - line_h) * 0.5 + baseline,
            if is_selected {
                theme.cursor_color.to_f32_rgba(1.0)
            } else {
                theme.foreground.to_f32_rgba(0.92)
            },
        );

        if pw > 520.0 {
            let desc_cols = ((pw * 0.44) / cell_w).floor().max(8.0) as usize;
            let desc: String = item.description.chars().take(desc_cols).collect();
            emit_simple_string(
                config,
                font_sys,
                atlas,
                overlays,
                &desc,
                px + (pw * 0.54).round(),
                iy + (item_h - line_h) * 0.5 + baseline,
                theme.foreground.to_f32_rgba(0.45),
            );
        }
    }

    if cp.filtered_indices.is_empty() {
        emit_simple_string(
            config,
            font_sys,
            atlas,
            overlays,
            "No matches",
            px + 12.0,
            list_y + (item_h - line_h) * 0.5 + baseline,
            theme.foreground.to_f32_rgba(0.45),
        );
    }

    let prompt_y = (py + ph - prompt_h).round();
    overlays.push(QuadInstance::solid(
        px,
        prompt_y,
        pw,
        prompt_h,
        theme.selection_background.to_f32_rgba(0.16),
    ));
    overlays.push(QuadInstance::solid(
        px,
        prompt_y,
        pw,
        1.0,
        theme.cursor_color.to_f32_rgba(0.28),
    ));

    let count = format!("{} / {}", cp.filtered_indices.len(), cp.all_items.len());
    let prompt_cols = ((pw / cell_w).floor() as usize).saturating_sub(count.chars().count() + 4);
    let prompt: String = format!(": {}|", cp.query)
        .chars()
        .take(prompt_cols.max(4))
        .collect();
    emit_simple_string(
        config,
        font_sys,
        atlas,
        overlays,
        &prompt,
        px + 10.0,
        prompt_y + (prompt_h - line_h) * 0.5 + baseline,
        theme.foreground.to_f32_rgba(1.0),
    );

    let count_x = (px + pw - count.chars().count() as f32 * cell_w - 10.0).round();
    emit_simple_string(
        config,
        font_sys,
        atlas,
        overlays,
        &count,
        count_x,
        prompt_y + (prompt_h - line_h) * 0.5 + baseline,
        theme.foreground.to_f32_rgba(0.45),
    );
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
    let cell_w = font_sys.metrics.cell_width;
    let baseline = font_sys.metrics.baseline;

    // Neovim-style `/` search line pinned to the bottom of the editor.
    let fh = (line_h + 6.0).round();
    let fy = (vp.y + vp.h - fh).round();
    overlays.push(QuadInstance::solid(
        vp.x,
        fy,
        vp.w,
        fh,
        theme.background.to_f32_rgba(0.96),
    ));
    overlays.push(QuadInstance::solid(
        vp.x,
        fy,
        vp.w,
        1.0,
        theme.cursor_color.to_f32_rgba(0.45),
    ));

    let counter = if fb.match_count == 0 {
        "[0/0]".to_string()
    } else {
        format!("[{}/{}]", fb.current_match, fb.match_count)
    };
    let prompt_cols =
        ((vp.w / cell_w).floor() as usize).saturating_sub(counter.chars().count() + 3);
    let prompt: String = format!("/{}|", fb.query)
        .chars()
        .take(prompt_cols.max(4))
        .collect();
    emit_simple_string(
        config,
        font_sys,
        atlas,
        overlays,
        &prompt,
        vp.x + 8.0,
        fy + (fh - line_h) * 0.5 + baseline,
        theme.foreground.to_f32_rgba(1.0),
    );

    let counter_x = (vp.x + vp.w - counter.chars().count() as f32 * cell_w - 8.0).round();
    emit_simple_string(
        config,
        font_sys,
        atlas,
        overlays,
        &counter,
        counter_x,
        fy + (fh - line_h) * 0.5 + baseline,
        theme.ansi[3].to_f32_rgba(0.95),
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
    emit_simple_string_scaled(
        config, font_sys, atlas, instances, text, start_x, baseline_y, color, 1.0,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_simple_string_scaled(
    config: &Config,
    font_sys: &mut FontSystem,
    atlas: &mut GlyphAtlas,
    instances: &mut Vec<QuadInstance>,
    text: &str,
    start_x: f32,
    baseline_y: f32,
    color: [f32; 4],
    scale: f32,
) {
    let advance = font_sys.metrics.cell_width * scale;
    let font_px = font_sys.metrics.font_px * scale;
    let mut cur_x = start_x;
    for ch in text.chars() {
        let font_id = font_sys.resolve_font_for_char(ch, FontStyle::Regular);
        let gid = font_sys.font(font_id).glyph_id(ch);
        if gid != 0 {
            let entry =
                atlas.get_or_rasterize(font_sys, font_id, gid, font_px, config.font_thicken);
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
        cur_x += advance;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_inside(quads: &[QuadInstance], vp: &EditorViewport) {
        assert!(!quads.is_empty());
        for quad in quads {
            assert!(
                quad.pos[0] >= vp.x - 2.0
                    && quad.pos[0] + quad.size[0] <= vp.x + vp.w + 2.0
                    && quad.pos[1] >= vp.y - 2.0
                    && quad.pos[1] + quad.size[1] <= vp.y + vp.h + 2.0,
                "quad outside viewport: pos={:?} size={:?}",
                quad.pos,
                quad.size
            );
        }
    }

    #[test]
    fn neovim_prompts_fit_inside_editor_viewport() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut atlas = GlyphAtlas::new(512);
        let vp = EditorViewport {
            x: 0.0,
            y: 20.0,
            w: 900.0,
            h: 600.0,
        };

        let mut palette = CommandPalette::new();
        palette.populate_items(std::path::Path::new("."), &[]);
        palette.set_query("file".to_string());
        let mut palette_quads = Vec::new();
        emit_command_palette(
            &config,
            &mut font_sys,
            &mut atlas,
            &mut palette_quads,
            &palette,
            &vp,
        );
        assert_inside(&palette_quads, &vp);

        let find_bar = FindBar {
            query: "main".to_string(),
            match_count: 3,
            current_match: 1,
            is_active: true,
        };
        let mut find_quads = Vec::new();
        emit_find_bar(
            &config,
            &mut font_sys,
            &mut atlas,
            &mut find_quads,
            &find_bar,
            &vp,
        );
        assert_inside(&find_quads, &vp);
    }

    #[test]
    fn git_panel_renders_inside_sidebar() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut atlas = GlyphAtlas::new(512);
        let mut git = crate::editor::git::GitStatus::default();
        git.is_repo = true;
        git.branch = Some("main".to_string());
        git.changes.push(crate::editor::git::GitChange {
            path: std::path::PathBuf::from("src/main.rs"),
            relative: "src/main.rs".to_string(),
            kind: crate::editor::git::GitChangeKind::Modified,
            staged: true,
        });
        let vp = EditorViewport {
            x: 0.0,
            y: 20.0,
            w: 260.0,
            h: 500.0,
        };
        let line_h = font_sys.metrics.cell_height;
        let baseline = font_sys.metrics.baseline;
        let mut bg = Vec::new();
        let mut fg = Vec::new();
        EditorView::emit_git(
            &config,
            &mut font_sys,
            &mut atlas,
            &mut bg,
            &mut fg,
            &git,
            vp.x,
            vp.y,
            vp.w,
            vp.h,
            line_h,
            baseline,
            1.0,
        );
        assert_inside(&bg, &vp);
        assert_inside(&fg, &vp);
    }
}
