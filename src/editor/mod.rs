pub mod buffer;
pub mod document;
pub mod explorer;
pub mod git;
pub mod lsp;
pub mod myanmar;
pub mod overlay;
pub mod plugin;
pub mod settings;
pub mod syntax;
pub mod view;
pub mod vim;

use std::path::{Path, PathBuf};

use arboard::Clipboard;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::config::Config;
use crate::editor::document::{EditorDocument, SelectionKind};
pub use crate::editor::explorer::{ExplorerEntry, FileExplorer};
use crate::editor::git::GitStatus;
use crate::editor::lsp::{LspClient, LspEvent};
use crate::editor::overlay::{
    CommandPalette, CompletionOverlay, FindBar, HoverOverlay, PaletteAction,
};
use crate::editor::plugin::PluginEngine;
pub use crate::editor::settings::{SettingsCategory, SettingsDraft, SettingsState};
use crate::editor::view::{EditorView, EditorViewport};
use crate::editor::vim::{VimMode, VimOperator, VimState};
use crate::font::FontSystem;
use crate::renderer::QuadInstance;
use crate::renderer::atlas::GlyphAtlas;
pub use crate::split::SplitAxis;
use crate::terminal::AppEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    /// Mode A: Pure Terminal Emulator (Ocean default)
    PureTerminal,
    /// Mode B: Split Mode (Editor pane on Top/Left, Embedded PTY Terminal on Bottom/Right)
    Split,
    /// Mode C: Pure Editor Viewport
    PureEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarPanel {
    Explorer,
    Search,
    Problems,
    Plugins,
    Git,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub preview: String,
}

#[derive(Debug, Default)]
pub struct WorkspaceSearch {
    pub query: String,
    pub replace: String,
    pub field: usize,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub active: bool,
    pub results: Vec<SearchHit>,
    pub selected: usize,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProblemEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub line: usize,
    pub col: usize,
    pub severity: lsp_types::DiagnosticSeverity,
    pub message: String,
}

pub struct EditorWorkspace {
    pub mode: WorkspaceMode,
    pub documents: Vec<EditorDocument>,
    pub active_doc_idx: usize,
    pub split_axis: SplitAxis,
    pub split_ratio: f32,
    pub editor_focused: bool,
    pub lsp: LspClient,
    pub plugins: PluginEngine,
    pub palette: Option<CommandPalette>,
    pub completion: Option<CompletionOverlay>,
    pub find_bar: FindBar,
    pub hover: Option<HoverOverlay>,
    pub explorer: FileExplorer,
    pub workspace_dir: PathBuf,
    pub dragging_split_divider: bool,
    pub last_click_pos: (f32, f32),
    pub click_count: u8,
    pub last_click_time: std::time::Instant,
    pub scroll_accum_y: f64,
    pub scroll_accum_x: f64,
    pub dragging_scrollbar: bool,
    pub scrollbar_drag_offset_y: f32,
    pub scrollbar_hovered: bool,
    pub completion_request: Option<PathBuf>,
    pub hover_request: Option<(PathBuf, usize, usize)>,
    pub vim: VimState,
    pub active_panel: SidebarPanel,
    pub activity_bar_visible: bool,
    pub close_confirm_requested: bool,
    pub ui_scale: f32,
    pub menu_height: f32,
    pub recent_files: Vec<PathBuf>,
    pub search: WorkspaceSearch,
    pub git: GitStatus,
    pub metrics_label: Option<String>,
    pub settings_state: SettingsState,
    pending_replace_all: Option<usize>,
}

impl Default for EditorWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorWorkspace {
    pub fn new() -> Self {
        let docs = vec![EditorDocument::new()];

        Self {
            mode: WorkspaceMode::PureTerminal,
            documents: docs,
            active_doc_idx: 0,
            split_axis: SplitAxis::Vertical, // Top: Editor, Bottom: Terminal
            split_ratio: 0.60,
            editor_focused: true,
            lsp: LspClient::new(),
            plugins: PluginEngine::new(),
            palette: None,
            completion: None,
            find_bar: FindBar::new(),
            hover: None,
            explorer: FileExplorer::new(),
            workspace_dir: PathBuf::new(),
            dragging_split_divider: false,
            last_click_pos: (0.0, 0.0),
            click_count: 0,
            last_click_time: std::time::Instant::now(),
            scroll_accum_y: 0.0,
            scroll_accum_x: 0.0,
            dragging_scrollbar: false,
            scrollbar_drag_offset_y: 0.0,
            scrollbar_hovered: false,
            completion_request: None,
            hover_request: None,
            vim: VimState::new(false),
            active_panel: SidebarPanel::Explorer,
            activity_bar_visible: false,
            close_confirm_requested: false,
            ui_scale: 1.0,
            menu_height: 0.0,
            recent_files: Vec::new(),
            search: WorkspaceSearch::default(),
            git: GitStatus::default(),
            metrics_label: None,
            settings_state: SettingsState::default(),
            pending_replace_all: None,
        }
    }

    pub fn recent_files(&self) -> &[PathBuf] {
        &self.recent_files
    }

    pub fn take_replace_all_request(&mut self) -> Option<usize> {
        self.pending_replace_all.take()
    }

    pub fn run_search(&mut self) {
        self.search.results.clear();
        self.search.selected = 0;
        let query = self.search.query.clone();
        if query.is_empty() {
            self.search.message = Some("Type a search query".to_string());
            return;
        }

        let root = if self.workspace_dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            self.workspace_dir.clone()
        };
        let walker = ignore::WalkBuilder::new(&root)
            .hidden(true)
            .git_ignore(true)
            .max_depth(Some(12))
            .build();
        let mut count = 0usize;
        let mut files = 0usize;

        for entry in walker.flatten() {
            if count >= 500 {
                break;
            }
            let path = entry.path();
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if path
                .metadata()
                .map(|metadata| metadata.len() > 2 * 1024 * 1024)
                .unwrap_or(true)
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            files += 1;
            for (line_idx, line) in text.lines().enumerate() {
                for (start, _end) in find_matches(
                    line,
                    &query,
                    self.search.case_sensitive,
                    self.search.whole_word,
                ) {
                    count += 1;
                    self.search.results.push(SearchHit {
                        path: path.to_path_buf(),
                        line: line_idx,
                        col: start,
                        preview: line.trim().chars().take(160).collect(),
                    });
                    if count >= 500 {
                        break;
                    }
                }
                if count >= 500 {
                    break;
                }
            }
        }

        self.search.message = Some(format!(
            "{} result{} in {} file{}",
            count,
            if count == 1 { "" } else { "s" },
            files,
            if files == 1 { "" } else { "s" }
        ));
    }

    pub fn replace_all_search(&mut self) {
        let query = self.search.query.clone();
        let replacement = self.search.replace.clone();
        if query.is_empty() {
            return;
        }
        let mut paths: Vec<PathBuf> = Vec::new();
        for hit in &self.search.results {
            if !paths.contains(&hit.path) {
                paths.push(hit.path.clone());
            }
        }

        let mut replaced = 0usize;
        for path in paths {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (updated, count) = replace_in_text(
                &text,
                &query,
                &replacement,
                self.search.case_sensitive,
                self.search.whole_word,
            );
            if count == 0 {
                continue;
            }
            replaced += count;
            if write_file_atomic(&path, updated.as_bytes()).is_err() {
                continue;
            }
            if let Some(index) = self
                .documents
                .iter()
                .position(|document| document.path() == Some(&path))
            {
                let old_cursor = self.documents[index].cursor;
                let old_scroll = self.documents[index].scroll_y;
                if let Ok(mut document) = EditorDocument::open(&path) {
                    let last_line = document.buffer.len_lines().saturating_sub(1);
                    let line = old_cursor.line.min(last_line);
                    document.cursor = crate::editor::document::Cursor::new(line, 0);
                    document.scroll_y = old_scroll.min(last_line);
                    document.scroll_to_cursor_requested = true;
                    self.documents[index] = document;
                }
            }
        }

        self.search.results.clear();
        self.run_search();
        self.search.message = Some(format!("Replaced {replaced} occurrence(s)"));
        self.refresh_git();
    }

    fn handle_search_key(&mut self, event: &KeyEvent, modifiers: ModifiersState) {
        let ctrl = modifiers.control_key();
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.search.active = false;
            }
            Key::Named(NamedKey::Tab) => {
                self.search.field = (self.search.field + 1) % 2;
            }
            Key::Named(NamedKey::Enter) => {
                if ctrl {
                    if !self.search.results.is_empty() {
                        self.pending_replace_all = Some(self.search.results.len());
                    }
                } else {
                    self.run_search();
                }
            }
            Key::Named(NamedKey::Backspace) => {
                if self.search.field == 0 {
                    self.search.query.pop();
                } else {
                    self.search.replace.pop();
                }
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.search.selected = self.search.selected.saturating_sub(1);
            }
            Key::Named(NamedKey::ArrowDown) => {
                if !self.search.results.is_empty() {
                    self.search.selected =
                        (self.search.selected + 1).min(self.search.results.len() - 1);
                }
            }
            Key::Character(text) if !ctrl && !modifiers.alt_key() => {
                if self.search.field == 0 {
                    self.search.query.push_str(text);
                } else {
                    self.search.replace.push_str(text);
                }
            }
            _ => {}
        }
    }

    fn record_recent_file(&mut self, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.recent_files.retain(|existing| existing != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(20);
    }

    pub fn has_unsaved(&self) -> bool {
        self.documents.iter().any(EditorDocument::is_dirty)
    }

    pub fn dirty_documents(&self) -> Vec<(String, bool)> {
        self.documents
            .iter()
            .filter(|document| document.is_dirty())
            .map(|document| (document.file_name(), document.path().is_some()))
            .collect()
    }

    pub fn request_close_active(&mut self) {
        if self.active_doc_ref().is_dirty() {
            self.close_confirm_requested = true;
        } else {
            self.close_active_document();
        }
    }

    pub fn take_close_confirm(&mut self) -> bool {
        std::mem::take(&mut self.close_confirm_requested)
    }

    pub fn save_active_document(&mut self) -> Result<(), String> {
        let name = self.active_doc_ref().file_name();
        let path = self.active_doc().path().cloned();
        match self.active_doc().save() {
            Ok(()) => {
                if let Some(path) = path {
                    self.lsp.did_save(&path);
                }
                self.refresh_git();
                Ok(())
            }
            Err(err) => Err(format!("Could not save {name}: {err}")),
        }
    }

    pub fn save_all_dirty(&mut self) -> Result<(), String> {
        let mut saved_paths = Vec::new();
        for document in &mut self.documents {
            if !document.is_dirty() {
                continue;
            }
            let name = document.file_name();
            let Some(path) = document.path().cloned() else {
                return Err(format!(
                    "{name} has no file path; save it manually before quitting."
                ));
            };
            if let Err(err) = document.save() {
                return Err(format!("Could not save {name}: {err}"));
            }
            saved_paths.push(path);
        }
        for path in saved_paths {
            self.lsp.did_save(&path);
        }
        self.refresh_git();
        Ok(())
    }

    pub fn editor_tab_height(line_h: f32) -> f32 {
        line_h + 6.0
    }

    pub fn breadcrumb_height(&self, _line_h: f32) -> f32 {
        0.0
    }

    pub fn open_settings(&mut self, config: &Config) {
        self.settings_state.reload_from_config(config);
        if let Some(idx) = self.documents.iter().position(|d| d.is_settings) {
            self.active_doc_idx = idx;
            return;
        }
        let doc = EditorDocument::new_settings();
        self.documents.push(doc);
        self.active_doc_idx = self.documents.len() - 1;
    }

    pub fn open_settings_default(&mut self) {
        let cfg = Config::load(None);
        self.open_settings(&cfg);
    }

    pub fn content_top(&self, vp_y: f32, line_h: f32) -> f32 {
        vp_y + Self::editor_tab_height(line_h) + self.breadcrumb_height(line_h)
    }

    pub fn body_viewport(&self, vp: &EditorViewport) -> EditorViewport {
        EditorViewport {
            x: vp.x,
            y: vp.y + self.menu_height,
            w: vp.w,
            h: (vp.h - self.menu_height).max(10.0),
        }
    }

    pub fn sidebar_rect(&self, vp: &EditorViewport) -> Option<(f32, f32, f32, f32)> {
        let body = self.body_viewport(vp);
        let panel_open = self.explorer.visible
            && (self.active_panel != SidebarPanel::Explorer || self.explorer.is_open());
        if !panel_open {
            return None;
        }
        let activity_w = if self.activity_bar_visible {
            44.0 * self.ui_scale
        } else {
            0.0
        };
        let width = match self.active_panel {
            SidebarPanel::Search => self
                .explorer
                .width
                .max(280.0)
                .min((body.w - 160.0).max(80.0)),
            SidebarPanel::Problems | SidebarPanel::Plugins | SidebarPanel::Git => self
                .explorer
                .width
                .max(240.0)
                .min((body.w - 160.0).max(80.0)),
            SidebarPanel::Explorer => self.explorer.width.min(body.w - 100.0).max(80.0),
        };
        Some((body.x + activity_w, body.y, width, body.h))
    }

    pub fn update_explorer_hover(
        &mut self,
        px: f32,
        py: f32,
        vp: &EditorViewport,
        font_sys: &mut FontSystem,
    ) -> bool {
        let previous = self.explorer.hovered_idx;
        let body = self.body_viewport(vp);
        if self.active_panel == SidebarPanel::Explorer
            && let Some((sx, sy, sw, sh)) = self.sidebar_rect(vp)
            && px >= sx
            && px < sx + sw
            && py >= sy
            && py <= sy + sh
        {
            let header_h = (font_sys.metrics.cell_height + 6.0).round();
            let item_h = (font_sys.metrics.cell_height + 3.0).round();
            self.explorer
                .update_hover(px, py, sx, body.y, header_h, item_h);
        } else {
            self.explorer.hovered_idx = None;
        }
        self.explorer.hovered_idx != previous
    }

    pub fn is_active(&self) -> bool {
        matches!(self.mode, WorkspaceMode::Split | WorkspaceMode::PureEditor)
    }

    /// Whether text input (IME commits, plain typing) should reach the document.
    pub fn accepts_text_input(&self) -> bool {
        !self.vim.enabled || self.vim.mode == VimMode::Insert
    }

    pub fn insert_ime_commit(&mut self, text: &str, font_sys: &mut FontSystem) -> bool {
        if !self.is_active() || !self.accepts_text_input() {
            return false;
        }
        self.active_doc().insert_text_all(text, font_sys);
        self.notify_doc_change();
        true
    }

    pub fn active_doc(&mut self) -> &mut EditorDocument {
        if self.active_doc_idx >= self.documents.len() {
            self.active_doc_idx = self.documents.len().saturating_sub(1);
        }
        &mut self.documents[self.active_doc_idx]
    }

    pub fn active_doc_ref(&self) -> &EditorDocument {
        let idx = self
            .active_doc_idx
            .min(self.documents.len().saturating_sub(1));
        &self.documents[idx]
    }

    pub fn open_folder<P: AsRef<Path>>(&mut self, path: P) {
        let p = path.as_ref();
        self.workspace_dir = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        self.explorer.open_folder(p);
        self.refresh_git();
    }

    pub fn refresh_git(&mut self) {
        if self.workspace_dir.as_os_str().is_empty() {
            self.git = GitStatus::default();
            return;
        }
        if let Some(last) = self.git.last_refresh
            && last.elapsed() < std::time::Duration::from_millis(600)
        {
            return;
        }
        self.git.refresh(&self.workspace_dir);
        if let Some(path) = self.active_doc_ref().path().cloned() {
            self.git.refresh_file(&self.workspace_dir, &path);
            self.git.refresh_blame(&self.workspace_dir, &path);
        }
    }

    pub fn refresh_active_git_file(&mut self) {
        if !self.git.is_repo {
            self.refresh_git();
            return;
        }
        if let Some(path) = self.active_doc_ref().path().cloned() {
            self.git.refresh_file(&self.workspace_dir, &path);
            self.git.refresh_blame(&self.workspace_dir, &path);
        }
    }

    pub fn show_git_diff(&mut self) {
        let Some(path) = self.active_doc_ref().path().cloned() else {
            self.git.error = Some("Open a tracked file to view its diff".to_string());
            self.active_panel = SidebarPanel::Git;
            self.explorer.visible = true;
            return;
        };
        if self.workspace_dir.as_os_str().is_empty() {
            return;
        }
        self.git.refresh(&self.workspace_dir);
        self.git.refresh_file(&self.workspace_dir, &path);
        let Some(diff) = self.git.diff_text(&self.workspace_dir, &path) else {
            self.git.error = Some(format!("No changes for {}", path.display()));
            self.active_panel = SidebarPanel::Git;
            self.explorer.visible = true;
            return;
        };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file");
        let mut document = EditorDocument::from_text(&diff, None, false);
        document.display_name = Some(format!("diff: {name}"));
        self.documents.push(document);
        self.active_doc_idx = self.documents.len() - 1;
        self.mode = WorkspaceMode::PureEditor;
        self.editor_focused = true;
        self.vim.reset_to_normal();
    }

    pub fn open_command_palette(&mut self, workspace_dir: &Path) {
        let mut palette = CommandPalette::new();
        let commands = self.plugins.registered_commands();
        palette.populate_items(workspace_dir, &commands);
        self.palette = Some(palette);
    }

    pub fn undo_active(&mut self, font_sys: &mut FontSystem) {
        self.active_doc().undo(font_sys);
        self.notify_doc_change();
    }

    pub fn redo_active(&mut self, font_sys: &mut FontSystem) {
        self.active_doc().redo(font_sys);
        self.notify_doc_change();
    }

    pub fn open_file<P: AsRef<Path>>(
        &mut self,
        path: P,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Result<(), String> {
        let p = path.as_ref();
        let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let p = p.as_path();
        self.record_recent_file(p);
        self.explorer.selected_path = Some(p.to_path_buf());

        if self.workspace_dir.as_os_str().is_empty()
            && let Some(parent) = p.parent()
        {
            self.workspace_dir = parent.to_path_buf();
            self.explorer.open_folder(parent);
            self.git.refresh(&self.workspace_dir);
        }

        // Check if already open
        if let Some(pos) = self
            .documents
            .iter()
            .position(|d| d.path() == Some(&p.to_path_buf()))
        {
            self.active_doc_idx = pos;
            self.active_doc().clear_extra_cursors();
            self.vim.reset_to_normal();
            self.completion = None;
            self.hover = None;
            self.refresh_active_git_file();
            return Ok(());
        }

        match EditorDocument::open(p) {
            Ok(doc) => {
                // If the only document is untitled and untouched, replace it
                if self.documents.len() == 1
                    && self.documents[0].path().is_none()
                    && !self.documents[0].is_dirty()
                    && self.documents[0].buffer.is_empty()
                {
                    self.documents[0] = doc;
                    self.active_doc_idx = 0;
                } else {
                    self.documents.push(doc);
                    self.active_doc_idx = self.documents.len() - 1;
                }

                // Start LSP for this file if supported
                let text = self.active_doc().buffer.rope.to_string();
                if self
                    .lsp
                    .start_for_file(p, &self.workspace_dir.clone(), proxy)
                {
                    self.lsp.did_open(p, &text, 1);
                }
                self.plugins.sync_buffer_text(&text);
                self.active_doc().clear_extra_cursors();
                self.completion = None;
                self.hover = None;
                self.completion_request = None;
                self.hover_request = None;
                self.refresh_active_git_file();
                Ok(())
            }
            Err(e) => Err(format!("Failed opening {}: {e}", p.display())),
        }
    }

    pub fn new_document(&mut self) {
        self.documents.push(EditorDocument::new());
        self.active_doc_idx = self.documents.len() - 1;
        self.vim.reset_to_normal();
        self.active_doc().clear_extra_cursors();
    }

    pub fn close_active_document(&mut self) {
        let closing_path = self
            .documents
            .get(self.active_doc_idx)
            .and_then(|d| d.path().cloned());

        if self.documents.len() > 1 {
            self.documents.remove(self.active_doc_idx);
            if self.active_doc_idx >= self.documents.len() {
                self.active_doc_idx = self.documents.len() - 1;
            }
        } else {
            // Reset to empty document
            self.documents[0] = EditorDocument::new();
        }

        if let Some(path) = closing_path {
            self.lsp.did_close(&path);
            self.lsp.clear_file_state(&path);
        }
        self.completion = None;
        self.hover = None;
        self.completion_request = None;
        self.hover_request = None;
        self.active_doc().clear_extra_cursors();
        self.vim.reset_to_normal();
    }

    pub fn next_document(&mut self) {
        if !self.documents.is_empty() {
            self.active_doc_idx = (self.active_doc_idx + 1) % self.documents.len();
            self.vim.reset_to_normal();
            self.active_doc().clear_extra_cursors();
            self.completion = None;
            self.hover = None;
        }
    }

    pub fn prev_document(&mut self) {
        if !self.documents.is_empty() {
            self.active_doc_idx = if self.active_doc_idx == 0 {
                self.documents.len() - 1
            } else {
                self.active_doc_idx - 1
            };
            self.vim.reset_to_normal();
            self.active_doc().clear_extra_cursors();
            self.completion = None;
            self.hover = None;
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn compute_layout(
        &self,
        win_w: f32,
        win_h: f32,
        tab_h: f32,
        status_h: f32,
    ) -> (
        EditorViewport,
        Option<(f32, f32, f32, f32)>,
        Option<(f32, f32, f32, f32)>,
    ) {
        let avail_y = tab_h;
        let avail_h = (win_h - tab_h - status_h).max(10.0);

        match self.mode {
            WorkspaceMode::PureTerminal => (
                EditorViewport {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                },
                None,
                Some((0.0, avail_y, win_w, avail_h)),
            ),
            WorkspaceMode::PureEditor => (
                EditorViewport {
                    x: 0.0,
                    y: avail_y,
                    w: win_w,
                    h: avail_h,
                },
                None,
                None,
            ),
            WorkspaceMode::Split => {
                let div_thick = 4.0_f32;
                match self.split_axis {
                    SplitAxis::Vertical => {
                        let total_h = avail_h - div_thick;
                        let ed_h = (total_h * self.split_ratio)
                            .round()
                            .clamp(60.0, total_h - 60.0);
                        let term_h = total_h - ed_h;
                        let div_y = avail_y + ed_h;
                        let term_y = div_y + div_thick;

                        (
                            EditorViewport {
                                x: 0.0,
                                y: avail_y,
                                w: win_w,
                                h: ed_h,
                            },
                            Some((0.0, div_y, win_w, div_thick)),
                            Some((0.0, term_y, win_w, term_h)),
                        )
                    }
                    SplitAxis::Horizontal => {
                        let total_w = win_w - div_thick;
                        let ed_w = (total_w * self.split_ratio)
                            .round()
                            .clamp(80.0, total_w - 80.0);
                        let term_w = total_w - ed_w;
                        let div_x = ed_w;
                        let term_x = div_x + div_thick;

                        (
                            EditorViewport {
                                x: 0.0,
                                y: avail_y,
                                w: ed_w,
                                h: avail_h,
                            },
                            Some((div_x, avail_y, div_thick, avail_h)),
                            Some((term_x, avail_y, term_w, avail_h)),
                        )
                    }
                }
            }
        }
    }

    pub fn poll_events(&mut self, font_sys: &mut FontSystem) -> bool {
        let events = self.lsp.poll_events();
        let mut redraw = !events.is_empty();
        let active_path = self.active_doc_ref().path().cloned();

        for event in events {
            match event {
                LspEvent::Completions(path) => {
                    if self.completion_request.as_ref() == Some(&path)
                        && active_path.as_ref() == Some(&path)
                    {
                        let items = self.lsp.latest_completions_for(&path);
                        self.completion_request = None;
                        if !items.is_empty() {
                            let mut overlay = CompletionOverlay::new();
                            overlay.set_items(items);
                            self.completion = Some(overlay);
                            redraw = true;
                        }
                    }
                }
                LspEvent::Hover(path) => {
                    if let Some((request_path, line, cluster)) = self.hover_request.clone()
                        && request_path == path
                        && active_path.as_ref() == Some(&path)
                    {
                        if let Some(text) = self.lsp.latest_hover_for(&path) {
                            self.hover = Some(HoverOverlay {
                                text,
                                line,
                                cluster_idx: cluster,
                            });
                            redraw = true;
                        }
                        self.hover_request = None;
                    }
                }
                LspEvent::Diagnostics(_) | LspEvent::StatusChanged => {}
            }
        }

        // Apply any pending text insertions from WASM plugins
        let pending = self.plugins.drain_pending_inserts();
        if !pending.is_empty() {
            redraw = true;
            for (offset, text) in pending {
                let doc = self.active_doc();
                let (line, col) = doc.buffer.char_to_line_col(offset);
                doc.cursor.line = line;
                doc.cursor.cluster_idx = col;
                doc.insert_text(&text, font_sys);
            }
        }

        redraw
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_key(
        &mut self,
        event: &KeyEvent,
        modifiers: ModifiersState,
        font_sys: &mut FontSystem,
        clipboard: &mut Option<Clipboard>,
        workspace_dir: &Path,
        proxy: EventLoopProxy<AppEvent>,
    ) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }

        let ctrl = modifiers.control_key();
        let shift = modifiers.shift_key();
        let alt = modifiers.alt_key();

        // 1. Command Palette handling
        if let Some(cp) = &mut self.palette {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.palette = None;
                    return true;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    cp.move_up();
                    return true;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    cp.move_down();
                    return true;
                }
                Key::Named(NamedKey::Enter) => {
                    if let Some(item) = cp.selected_item() {
                        let action = item.action.clone();
                        self.palette = None;
                        self.execute_palette_action(action, proxy, font_sys);
                    } else {
                        self.palette = None;
                    }
                    return true;
                }
                Key::Named(NamedKey::Backspace) => {
                    cp.backspace();
                    return true;
                }
                Key::Character(s) if !ctrl && !alt => {
                    for c in s.chars() {
                        cp.insert_char(c);
                    }
                    return true;
                }
                _ => return true,
            }
        }

        // 2. Global Shortcuts
        // Ctrl+P: Open Command Palette
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("p")) {
            let mut cp = CommandPalette::new();
            let plugin_cmds = self.plugins.registered_commands();
            cp.populate_items(workspace_dir, &plugin_cmds);
            self.palette = Some(cp);
            return true;
        }

        // Ctrl+B: Toggle File Explorer Sidebar
        if ctrl
            && !shift
            && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("b"))
        {
            self.explorer.toggle_visible();
            return true;
        }

        // Ctrl+, : Open Settings
        if ctrl && !shift && matches!(&event.logical_key, Key::Character(s) if s == ",") {
            self.open_settings_default();
            return true;
        }

        // Ctrl+\ : Toggle Split Mode
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s == "\\" || s == "|") {
            self.mode = match self.mode {
                WorkspaceMode::PureTerminal => WorkspaceMode::Split,
                WorkspaceMode::Split => WorkspaceMode::PureEditor,
                WorkspaceMode::PureEditor => WorkspaceMode::Split,
            };
            self.editor_focused = true;
            return true;
        }

        // Ctrl+Shift+E: Toggle Editor directly
        if ctrl
            && shift
            && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("e"))
        {
            self.mode = match self.mode {
                WorkspaceMode::PureTerminal => WorkspaceMode::PureEditor,
                _ => WorkspaceMode::PureTerminal,
            };
            self.editor_focused = true;
            return true;
        }

        if !self.is_active() || (!self.editor_focused && self.mode != WorkspaceMode::PureEditor) {
            return false;
        }

        if self.active_panel == SidebarPanel::Search && self.search.active {
            self.handle_search_key(event, modifiers);
            return true;
        }

        // 3. Find Bar handling
        if self.find_bar.is_active {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.find_bar.close();
                    return true;
                }
                Key::Named(NamedKey::Enter) => {
                    self.jump_to_next_match(font_sys, shift);
                    return true;
                }
                Key::Named(NamedKey::Backspace) => {
                    self.find_bar.query.pop();
                    self.update_find_matches();
                    return true;
                }
                Key::Character(s) if !ctrl && !alt => {
                    self.find_bar.query.push_str(s);
                    self.update_find_matches();
                    return true;
                }
                _ => {}
            }
        }

        // 4. Completion Overlay navigation
        if let Some(comp) = &mut self.completion {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    self.completion = None;
                    return true;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    comp.move_up();
                    return true;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    comp.move_down();
                    return true;
                }
                Key::Named(NamedKey::Tab) | Key::Named(NamedKey::Enter) => {
                    if let Some(item) = comp.selected_item() {
                        let text = item.insert_text.clone();
                        self.completion = None;
                        self.active_doc().insert_text_all(&text, font_sys);
                    } else {
                        self.completion = None;
                    }
                    return true;
                }
                _ => {
                    self.completion = None;
                }
            }
        }

        if alt
            && !ctrl
            && !shift
            && let Key::Character(key) = &event.logical_key
        {
            let handled = match key.as_str() {
                "1" => {
                    if self.active_panel == SidebarPanel::Explorer && self.explorer.visible {
                        self.explorer.visible = false;
                    } else {
                        self.active_panel = SidebarPanel::Explorer;
                        self.explorer.visible = true;
                    }
                    true
                }
                "2" => {
                    self.active_panel = SidebarPanel::Search;
                    self.explorer.visible = true;
                    self.search.active = true;
                    true
                }
                "3" => {
                    self.active_panel = SidebarPanel::Problems;
                    self.explorer.visible = true;
                    true
                }
                "4" => {
                    self.active_panel = SidebarPanel::Plugins;
                    self.explorer.visible = true;
                    true
                }
                "5" => {
                    self.active_panel = SidebarPanel::Git;
                    self.explorer.visible = true;
                    self.refresh_git();
                    true
                }
                _ => false,
            };
            if handled {
                return true;
            }
        }

        // 4b. Vim modal dispatch (opt-in via `editor-keymap = vim`)
        if self.vim.enabled {
            match self.vim.mode {
                VimMode::Insert => {
                    if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                        self.vim.mode = VimMode::Normal;
                        self.vim.count.clear();
                        self.vim.operator = None;
                        self.vim.pending_g = false;
                        self.vim.pending_replace = false;
                        self.active_doc().move_left(false, font_sys);
                        return true;
                    }
                    // Insert mode falls through to the normal editing path.
                }
                _ => {
                    let vim_ctrl = ctrl
                        && matches!(
                            &event.logical_key,
                            Key::Character(s)
                                if s.eq_ignore_ascii_case("r")
                                    || s.eq_ignore_ascii_case("d")
                                    || s.eq_ignore_ascii_case("u")
                        );
                    if !ctrl || vim_ctrl {
                        self.handle_vim_key(event, modifiers, font_sys);
                        return true;
                    }
                }
            }
        }

        // 5. Standard Editor Shortcuts (TTT-style Modeless)
        // Ctrl+S: Save File
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("s")) {
            if let Err(e) = self.active_doc().save() {
                eprintln!("Save error: {e}");
            } else if let Some(path) = self.active_doc().path().cloned() {
                self.lsp.did_save(&path);
            }
            return true;
        }

        // Ctrl+C: Copy Selection
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("c")) {
            if let Some(text) = self.active_doc().selected_text(font_sys)
                && let Some(cb) = clipboard
            {
                let _ = cb.set_text(text);
            }
            return true;
        }

        // Ctrl+V: Paste from Clipboard
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("v")) {
            if let Some(cb) = clipboard
                && let Ok(text) = cb.get_text()
            {
                self.active_doc().insert_text_all(&text, font_sys);
                self.notify_doc_change();
            }
            return true;
        }

        // Ctrl+X: Cut Selection
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("x")) {
            if let Some(text) = self.active_doc().selected_text(font_sys) {
                if let Some(cb) = clipboard {
                    let _ = cb.set_text(text);
                }
                self.active_doc().delete_selection(font_sys);
                self.notify_doc_change();
            }
            return true;
        }

        // Ctrl+Z: Undo
        if ctrl
            && !shift
            && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("z"))
        {
            self.active_doc().clear_extra_cursors();
            self.active_doc().undo(font_sys);
            self.notify_doc_change();
            return true;
        }

        // Ctrl+Y / Ctrl+Shift+Z: Redo
        if (ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("y")))
            || (ctrl
                && shift
                && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("z")))
        {
            self.active_doc().clear_extra_cursors();
            self.active_doc().redo(font_sys);
            self.notify_doc_change();
            return true;
        }

        // Ctrl+Shift+G: Git Changes
        if ctrl
            && shift
            && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("g"))
        {
            self.active_panel = SidebarPanel::Git;
            self.explorer.visible = true;
            self.refresh_git();
            return true;
        }

        // Ctrl+Shift+F: Workspace Search
        if ctrl
            && shift
            && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("f"))
        {
            self.active_panel = SidebarPanel::Search;
            self.explorer.visible = true;
            self.search.active = true;
            return true;
        }

        // Ctrl+F: Open Find
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("f")) {
            self.find_bar.open();
            return true;
        }

        // Ctrl+W: Close Document
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("w")) {
            self.request_close_active();
            return true;
        }

        // Ctrl+Tab / Ctrl+Shift+Tab: Document switching
        if ctrl && matches!(&event.logical_key, Key::Named(NamedKey::Tab)) {
            if shift {
                self.prev_document();
            } else {
                self.next_document();
            }
            return true;
        }

        // Ctrl+Space: Autocompletion trigger
        if ctrl && matches!(&event.logical_key, Key::Named(NamedKey::Space)) {
            self.trigger_autocompletion(font_sys);
            return true;
        }

        // Ctrl+K: Hover / Docs trigger
        if ctrl && matches!(&event.logical_key, Key::Character(s) if s.eq_ignore_ascii_case("k")) {
            self.trigger_hover(font_sys);
            return true;
        }

        if ctrl
            && alt
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::ArrowUp) | Key::Named(NamedKey::ArrowDown)
            )
        {
            let delta = if matches!(event.logical_key, Key::Named(NamedKey::ArrowUp)) {
                -1
            } else {
                1
            };
            self.active_doc().add_caret_vertical(delta, font_sys);
            return true;
        }
        if matches!(event.logical_key, Key::Named(NamedKey::Escape))
            && self.active_doc_ref().caret_count() > 1
            && (!self.vim.enabled || self.vim.mode == VimMode::Insert)
        {
            self.active_doc().clear_extra_cursors();
            return true;
        }

        // 6. Navigation and Editing Keys
        match &event.logical_key {
            Key::Named(NamedKey::ArrowLeft) => {
                self.hover = None;
                self.active_doc().move_carets(-1, 0, shift, font_sys);
                return true;
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.hover = None;
                self.active_doc().move_carets(1, 0, shift, font_sys);
                return true;
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.hover = None;
                if ctrl {
                    self.active_doc().scroll_y = self.active_doc().scroll_y.saturating_sub(1);
                } else {
                    self.active_doc().move_carets(0, -1, shift, font_sys);
                }
                return true;
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.hover = None;
                if ctrl {
                    let max_scroll = self.active_doc().buffer.len_lines().saturating_sub(1);
                    self.active_doc().scroll_y = (self.active_doc().scroll_y + 1).min(max_scroll);
                } else {
                    self.active_doc().move_carets(0, 1, shift, font_sys);
                }
                return true;
            }
            Key::Named(NamedKey::Home) => {
                self.hover = None;
                self.active_doc().move_home(shift, font_sys);
                return true;
            }
            Key::Named(NamedKey::End) => {
                self.hover = None;
                self.active_doc().move_end(shift, font_sys);
                return true;
            }
            Key::Named(NamedKey::PageUp) => {
                self.hover = None;
                self.active_doc().page_up(20, shift, font_sys);
                return true;
            }
            Key::Named(NamedKey::PageDown) => {
                self.hover = None;
                self.active_doc().page_down(20, shift, font_sys);
                return true;
            }
            Key::Named(NamedKey::Backspace) => {
                self.hover = None;
                self.active_doc().backspace_all(font_sys);
                self.notify_doc_change();
                return true;
            }
            Key::Named(NamedKey::Delete) => {
                self.hover = None;
                self.active_doc().delete_all(font_sys);
                self.notify_doc_change();
                return true;
            }
            Key::Named(NamedKey::Enter) => {
                self.hover = None;
                // Smart auto-indent: copy leading whitespace of current line
                let cur_line = self.active_doc().cursor.line;
                let line_str = self.active_doc().buffer.line_without_newline(cur_line);
                let indent: String = line_str.chars().take_while(|c| c.is_whitespace()).collect();
                let insert = format!("\n{indent}");
                self.active_doc().insert_text_all(&insert, font_sys);
                self.notify_doc_change();
                return true;
            }
            Key::Named(NamedKey::Tab) => {
                self.hover = None;
                self.active_doc().insert_text_all("    ", font_sys);
                self.notify_doc_change();
                return true;
            }
            Key::Named(NamedKey::Space) if !ctrl && !alt => {
                self.hover = None;
                self.active_doc().insert_text_all(" ", font_sys);
                self.notify_doc_change();
                return true;
            }
            Key::Character(s) if !ctrl && !alt => {
                self.hover = None;
                self.active_doc().insert_text_all(s, font_sys);
                self.notify_doc_change();

                // Trigger completion if alphanumeric
                if s.chars().any(|c| c.is_alphabetic()) {
                    self.trigger_autocompletion(font_sys);
                }
                return true;
            }
            _ => {}
        }

        // Compose sequences, dead keys, and non-IME layouts can deliver text
        // through `event.text` without a printable `logical_key`.
        if !ctrl
            && !alt
            && let Some(text) = event.text.as_ref()
            && !text.is_empty()
            && !text.chars().any(char::is_control)
        {
            self.hover = None;
            self.active_doc().insert_text_all(text, font_sys);
            self.notify_doc_change();
            return true;
        }

        false
    }

    fn handle_vim_key(
        &mut self,
        event: &KeyEvent,
        modifiers: ModifiersState,
        font_sys: &mut FontSystem,
    ) {
        let outcome = {
            let vim = &mut self.vim;
            let doc = &mut self.documents[self.active_doc_idx];
            vim_process(vim, doc, event, modifiers, font_sys)
        };

        if outcome.changed {
            self.notify_doc_change();
        }
        if outcome.open_find {
            self.find_bar.open();
        }
        if let Some(reverse) = outcome.next_match {
            self.jump_to_next_match(font_sys, reverse);
        }
        let mut save_failed = false;
        if outcome.save
            && let Err(err) = self.save_active_document()
        {
            self.vim.set_status(err);
            save_failed = true;
        }
        if outcome.confirm_close {
            self.request_close_active();
        }
        if outcome.close && !save_failed {
            self.close_active_document();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_mouse_click(
        &mut self,
        button: MouseButton,
        px: f32,
        py: f32,
        vp: &EditorViewport,
        tab_h: f32,
        font_sys: &mut FontSystem,
        modifiers: ModifiersState,
        proxy: EventLoopProxy<AppEvent>,
        config: &mut Config,
    ) -> bool {
        if button != MouseButton::Left {
            return false;
        }

        let body_vp = self.body_viewport(vp);
        let vp = &body_vp;
        let line_h = font_sys.metrics.cell_height;
        let cell_w = font_sys.metrics.cell_width;
        let _ = tab_h;

        let activity_w = if self.activity_bar_visible {
            44.0 * self.ui_scale
        } else {
            0.0
        };
        if self.activity_bar_visible && px >= vp.x && px < vp.x + activity_w {
            let ui = self.ui_scale.max(0.5);
            let index = ((py - vp.y - 8.0 * ui) / (44.0 * ui)).floor();
            if index >= 0.0 {
                match index as usize {
                    0 => {
                        if self.active_panel == SidebarPanel::Explorer && self.explorer.visible {
                            self.explorer.visible = false;
                        } else {
                            self.active_panel = SidebarPanel::Explorer;
                            self.explorer.visible = true;
                        }
                    }
                    1 => {
                        self.active_panel = SidebarPanel::Search;
                        self.explorer.visible = true;
                        self.search.active = true;
                    }
                    2 => {
                        if self.active_panel == SidebarPanel::Problems && self.explorer.visible {
                            self.explorer.visible = false;
                        } else {
                            self.active_panel = SidebarPanel::Problems;
                            self.explorer.visible = true;
                        }
                    }
                    3 => {
                        if self.active_panel == SidebarPanel::Plugins && self.explorer.visible {
                            self.explorer.visible = false;
                        } else {
                            self.active_panel = SidebarPanel::Plugins;
                            self.explorer.visible = true;
                        }
                    }
                    4 => {
                        if self.active_panel == SidebarPanel::Git && self.explorer.visible {
                            self.explorer.visible = false;
                        } else {
                            self.active_panel = SidebarPanel::Git;
                            self.explorer.visible = true;
                            self.refresh_git();
                        }
                    }
                    _ => {}
                }
            }
            return true;
        }

        let panel_open = self.explorer.visible
            && (self.active_panel != SidebarPanel::Explorer || self.explorer.is_open());
        let sidebar_w = if panel_open {
            match self.active_panel {
                SidebarPanel::Search => {
                    self.explorer.width.max(280.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Problems | SidebarPanel::Plugins | SidebarPanel::Git => {
                    self.explorer.width.max(240.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Explorer => self.explorer.width.min(vp.w - 100.0).max(80.0),
            }
        } else {
            0.0
        };
        let sx = vp.x + activity_w;

        if panel_open {
            // Check click on divider
            if (px - (sx + sidebar_w)).abs() <= 4.0 {
                self.explorer.dragging_divider = true;
                return true;
            }

            // Check click within sidebar
            if px >= sx && px < sx + sidebar_w {
                let header_h = (line_h + 6.0).round();
                let ui = self.ui_scale.max(0.5);

                if py >= vp.y && py < vp.y + header_h {
                    if px >= sx + 4.0 * ui && px < sx + 54.0 * ui {
                        self.active_panel = SidebarPanel::Explorer;
                        self.explorer.visible = true;
                        return true;
                    } else if px >= sx + 54.0 * ui && px < sx + 98.0 * ui {
                        self.active_panel = SidebarPanel::Search;
                        self.explorer.visible = true;
                        self.search.active = true;
                        return true;
                    } else if px >= sx + 98.0 * ui && px < sx + 170.0 * ui {
                        self.active_panel = SidebarPanel::Git;
                        self.explorer.visible = true;
                        self.refresh_git();
                        return true;
                    } else if px >= sx + sidebar_w - 48.0 * ui && px < sx + sidebar_w - 24.0 * ui {
                        self.explorer.visible = false;
                        return true;
                    } else if px >= sx + sidebar_w - 24.0 * ui && px <= sx + sidebar_w {
                        self.open_settings(config);
                        return true;
                    }
                    return true;
                }

                match self.active_panel {
                    SidebarPanel::Explorer => {
                        let item_h = (line_h + 3.0).round();

                        if let Some(idx) = self
                            .explorer
                            .entry_at_point(px, py, sx, vp.y, header_h, item_h)
                            && let Some(file_to_open) = self.explorer.toggle_expand(idx)
                        {
                            let _ = self.open_file(&file_to_open, proxy);
                        }
                    }
                    SidebarPanel::Search => {
                        let header_h = (line_h + 6.0).round();
                        let field_h = (line_h + 4.0).round();
                        let item_h = (line_h + 3.0).round();
                        let query_y = vp.y + header_h;
                        let replace_y = query_y + field_h;
                        let options_y = replace_y + field_h;
                        let results_y = options_y + field_h;

                        if py >= query_y && py < replace_y {
                            self.search.active = true;
                            self.search.field = 0;
                        } else if py >= replace_y && py < options_y {
                            self.search.active = true;
                            self.search.field = 1;
                        } else if py >= options_y && py < results_y {
                            if px < sx + 44.0 * self.ui_scale.max(0.5) {
                                self.search.case_sensitive = !self.search.case_sensitive;
                            } else {
                                self.search.whole_word = !self.search.whole_word;
                            }
                        } else if py >= results_y {
                            let row = ((py - results_y) / item_h).floor();
                            if row >= 0.0
                                && let Some(hit) = self.search.results.get(row as usize).cloned()
                            {
                                let _ = self.open_file(&hit.path, proxy);
                                self.active_doc().move_to_line(hit.line, font_sys);
                                self.editor_focused = true;
                                self.search.active = false;
                            }
                        }
                    }
                    SidebarPanel::Problems => {
                        let header_h = (line_h + 6.0).round();
                        let item_h = (line_h + 3.0).round();
                        let problems = self.collect_problems();
                        let row = ((py - vp.y - header_h) / item_h).floor();
                        if row >= 0.0
                            && let Some(problem) = problems.get(row as usize).cloned()
                        {
                            let _ = self.open_file(&problem.path, proxy);
                            self.active_doc().move_to_line(problem.line, font_sys);
                            self.editor_focused = true;
                        }
                    }
                    SidebarPanel::Plugins => {
                        let header_h = (line_h + 6.0).round();
                        let item_h = (line_h + 4.0).round();
                        if py >= vp.y && py < vp.y + header_h {
                            self.plugins.reload_plugins();
                            return true;
                        }
                        let row = ((py - vp.y - header_h) / item_h).floor();
                        if row >= 0.0
                            && let Some(info) = crate::plugins::list().get(row as usize).cloned()
                        {
                            let _ = crate::plugins::set_enabled(&info.id, !info.enabled);
                            self.plugins.reload_plugins();
                        }
                    }
                    SidebarPanel::Git => {
                        let header_h = (line_h + 6.0).round();
                        let item_h = (line_h + 3.0).round();
                        if py >= vp.y && py < vp.y + header_h {
                            self.refresh_git();
                            return true;
                        }
                        let content_y = vp.y
                            + header_h
                            + if self.git.branch.is_some() {
                                item_h
                            } else {
                                0.0
                            };
                        let row = ((py - content_y) / item_h).floor();
                        if row >= 0.0
                            && let Some(change) = self.git.changes.get(row as usize).cloned()
                        {
                            let _ = self.open_file(&change.path, proxy);
                            self.refresh_active_git_file();
                            self.editor_focused = true;
                        }
                    }
                }
                return true;
            }
        }

        let code_vp = EditorViewport {
            x: sx + sidebar_w,
            y: vp.y,
            w: (vp.w - activity_w - sidebar_w).max(10.0),
            h: vp.h,
        };

        // Check if clicked in the always-visible document tab bar
        let ed_tab_h = (line_h + 6.0).round();
        let ui = self.ui_scale.max(0.5);
        if py >= code_vp.y && py <= code_vp.y + ed_tab_h {
            let collapsed_pad = if !panel_open { 46.0 * ui } else { 0.0 };
            if !panel_open {
                if px >= code_vp.x + 4.0 * ui && px < code_vp.x + 24.0 * ui {
                    self.explorer.visible = true;
                    return true;
                } else if px >= code_vp.x + 24.0 * ui && px < code_vp.x + 44.0 * ui {
                    self.open_settings(config);
                    return true;
                }
            }

            let count = self.documents.len().max(1);
            let tab_w = (180.0_f32)
                .min((code_vp.w - 40.0 - collapsed_pad) / count as f32)
                .max(60.0);
            let relative = px - code_vp.x - 4.0 - collapsed_pad;
            let tab_idx = (relative / (tab_w + 4.0)).floor();
            if tab_idx >= 0.0 {
                let tab_idx = tab_idx as usize;
                if tab_idx < self.documents.len() {
                    let tab_x = code_vp.x + 4.0 + collapsed_pad + tab_idx as f32 * (tab_w + 4.0);
                    self.active_doc_idx = tab_idx;
                    if px >= tab_x + tab_w - 22.0 * ui && px <= tab_x + tab_w {
                        self.request_close_active();
                    }
                    return true;
                }
            }

            let plus_x = code_vp.x + 4.0 + collapsed_pad + count as f32 * (tab_w + 4.0);
            if plus_x + 22.0 * ui <= code_vp.x + code_vp.w
                && px >= plus_x
                && px <= plus_x + (ed_tab_h - 4.0 * ui)
            {
                self.new_document();
                self.active_doc_idx = self.documents.len() - 1;
                return true;
            }
        }

        if self.active_doc().is_settings {
            let content_y = code_vp.y + ed_tab_h;
            let status_h = (line_h * 0.80 + 4.0).round();
            let content_h = (code_vp.y + code_vp.h - status_h - content_y).max(1.0);
            return self.handle_settings_click(
                px, py, code_vp.x, content_y, code_vp.w, content_h, ui, line_h, config,
            );
        }

        // Check if clicked in Code text area or Scrollbar
        let max_line_digits = (self.active_doc().buffer.len_lines() + 1)
            .to_string()
            .len()
            .max(3);
        let gutter_w = (max_line_digits as f32 + 3.0) * cell_w;
        let scrollbar_w = 8.0_f32;
        let scrollbar_x = code_vp.x + code_vp.w - scrollbar_w;

        let _ = tab_h;
        let content_y = self.content_top(code_vp.y, line_h);
        let status_h = (line_h + 4.0).round();
        let content_h = (code_vp.y + code_vp.h - status_h - content_y).max(1.0);

        // Check click on Scrollbar track or thumb
        if px >= scrollbar_x - 4.0
            && px <= code_vp.x + code_vp.w
            && py >= content_y
            && py <= content_y + content_h
        {
            let total_lines = self.active_doc().buffer.len_lines();
            let visible_lines = ((content_h / line_h).ceil() as usize).max(1);
            let thumb_h = (content_h
                * (visible_lines as f32 / total_lines as f32).clamp(0.04, 1.0))
            .clamp(24.0, content_h);
            let track_avail = (content_h - thumb_h).max(0.0);
            let max_scroll = total_lines.saturating_sub(visible_lines).max(1);

            let scroll_ratio =
                (self.active_doc().scroll_y as f32 / max_scroll as f32).clamp(0.0, 1.0);
            let thumb_y = content_y + scroll_ratio * track_avail;

            if py >= thumb_y && py <= thumb_y + thumb_h {
                self.dragging_scrollbar = true;
                self.scrollbar_drag_offset_y = py - thumb_y;
            } else {
                let ratio = if track_avail > 0.0 {
                    ((py - content_y - thumb_h * 0.5) / track_avail).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                self.active_doc().scroll_y = (ratio * max_scroll as f32).round() as usize;
                self.dragging_scrollbar = true;
                self.scrollbar_drag_offset_y = thumb_h * 0.5;
            }
            return true;
        }

        if modifiers.alt_key() {
            self.active_doc()
                .extra_caret_at_pixel(px, py, code_vp.x, content_y, gutter_w, line_h, font_sys);
            return true;
        }

        let now = std::time::Instant::now();
        let is_double_click = now.duration_since(self.last_click_time).as_millis() < 350
            && (px - self.last_click_pos.0).abs() < 5.0
            && (py - self.last_click_pos.1).abs() < 5.0;
        self.last_click_time = now;
        self.last_click_pos = (px, py);

        if is_double_click {
            self.click_count = (self.click_count + 1) % 3;
        } else {
            self.click_count = 1;
        }

        let select = modifiers.shift_key() || self.vim.mode.is_visual();
        self.active_doc().set_cursor_from_pixel(
            px, py, code_vp.x, content_y, gutter_w, line_h, font_sys, select,
        );

        if self.click_count == 2 {
            self.active_doc().select_word_at_cursor(font_sys);
        } else if self.click_count == 0 {
            self.active_doc().select_line_at_cursor(font_sys);
        }

        true
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle_settings_click(
        &mut self,
        px: f32,
        py: f32,
        vx: f32,
        vy: f32,
        vw: f32,
        vh: f32,
        ui: f32,
        line_h: f32,
        config: &mut Config,
    ) -> bool {
        let cat_h = (28.0 * ui).max(line_h);

        // 1. Categories bar click
        if py >= vy && py <= vy + cat_h {
            if px >= vx + 16.0 * ui && px < vx + 76.0 * ui {
                self.settings_state.active_category = SettingsCategory::General;
                self.settings_state.scroll_y = 0.0;
                return true;
            } else if px >= vx + 76.0 * ui && px < vx + 136.0 * ui {
                self.settings_state.active_category = SettingsCategory::Editor;
                self.settings_state.scroll_y = 0.0;
                return true;
            } else if px >= vx + 136.0 * ui && px < vx + 236.0 * ui {
                self.settings_state.active_category = SettingsCategory::Appearance;
                self.settings_state.scroll_y = 0.0;
                return true;
            } else if px >= vx + 236.0 * ui && px < vx + 306.0 * ui {
                self.settings_state.active_category = SettingsCategory::Sidebar;
                self.settings_state.scroll_y = 0.0;
                return true;
            } else if px >= vx + 306.0 * ui && px < vx + 386.0 * ui {
                self.settings_state.active_category = SettingsCategory::Terminal;
                self.settings_state.scroll_y = 0.0;
                return true;
            } else if px >= vx + 386.0 * ui && px < vx + 450.0 * ui {
                self.settings_state.active_category = SettingsCategory::Diff;
                self.settings_state.scroll_y = 0.0;
                return true;
            }
        }

        // 2. Bottom buttons (Cancel / Apply)
        let bottom_h = 42.0 * ui;
        let bottom_y = vy + vh - bottom_h;
        if py >= bottom_y {
            let apply_w = 64.0 * ui;
            let apply_x = vx + vw - apply_w - 20.0 * ui;
            let cancel_w = 64.0 * ui;
            let cancel_x = apply_x - cancel_w - 12.0 * ui;

            if px >= apply_x && px <= apply_x + apply_w {
                let _ = self.settings_state.apply(config);
                return true;
            } else if px >= cancel_x && px <= cancel_x + cancel_w {
                self.settings_state.reload_from_config(config);
                self.request_close_active();
                return true;
            }
            return true;
        }

        // 3. Settings items content
        let content_y = vy + cat_h + 24.0 * ui - self.settings_state.scroll_y;
        let item_h = (26.0 * ui).max(line_h);

        match self.settings_state.active_category {
            SettingsCategory::General => {
                let row0 = content_y + item_h + 8.0 * ui;
                let row1 = row0 + item_h;
                let row2 = row1 + item_h + 20.0 * ui + item_h + 8.0 * ui;
                let row3 = row2 + item_h + 20.0 * ui + item_h + 8.0 * ui;

                if py >= row0 && py < row0 + item_h {
                    self.settings_state
                        .toggle_option(SettingsCategory::General, 0);
                    return true;
                } else if py >= row1 && py < row1 + item_h {
                    self.settings_state
                        .toggle_option(SettingsCategory::General, 1);
                    return true;
                } else if py >= row2 && py < row2 + item_h {
                    self.settings_state
                        .toggle_option(SettingsCategory::General, 2);
                    return true;
                } else if py >= row3 && py < row3 + item_h {
                    self.settings_state
                        .toggle_option(SettingsCategory::General, 3);
                    return true;
                }
            }
            SettingsCategory::Editor => {
                let mut cur = content_y + item_h + 8.0 * ui;
                for idx in 0..8 {
                    if idx == 2 {
                        cur += 20.0 * ui + item_h + 8.0 * ui;
                    }
                    if py >= cur && py < cur + item_h {
                        self.settings_state
                            .toggle_option(SettingsCategory::Editor, idx);
                        return true;
                    }
                    cur += item_h;
                }
            }
            SettingsCategory::Appearance => {
                let mut cur = content_y + item_h + 8.0 * ui;
                for idx in 0..7 {
                    if idx == 5 {
                        cur += 20.0 * ui + item_h + 8.0 * ui;
                    }
                    if py >= cur && py < cur + item_h {
                        self.settings_state
                            .toggle_option(SettingsCategory::Appearance, idx);
                        return true;
                    }
                    cur += item_h;
                }
            }
            SettingsCategory::Sidebar => {
                let mut cur = content_y + item_h + 8.0 * ui;
                for idx in 0..6 {
                    if idx == 3 {
                        cur += 20.0 * ui + item_h + 8.0 * ui;
                    }
                    if py >= cur && py < cur + item_h {
                        self.settings_state
                            .toggle_option(SettingsCategory::Sidebar, idx);
                        return true;
                    }
                    cur += item_h;
                }
            }
            SettingsCategory::Terminal => {
                let mut cur = content_y + item_h + 8.0 * ui;
                for idx in 0..3 {
                    if py >= cur && py < cur + item_h {
                        self.settings_state
                            .toggle_option(SettingsCategory::Terminal, idx);
                        return true;
                    }
                    cur += item_h;
                }
            }
            SettingsCategory::Diff => {
                let mut cur = content_y + item_h + 8.0 * ui;
                for idx in 0..4 {
                    if py >= cur && py < cur + item_h {
                        self.settings_state
                            .toggle_option(SettingsCategory::Diff, idx);
                        return true;
                    }
                    cur += item_h;
                }
            }
        }

        true
    }

    pub fn handle_mouse_drag(
        &mut self,
        px: f32,
        py: f32,
        vp: &EditorViewport,
        tab_h: f32,
        font_sys: &mut FontSystem,
    ) {
        let body_vp = self.body_viewport(vp);
        let vp = &body_vp;
        let _ = tab_h;
        let line_h = font_sys.metrics.cell_height;
        let cell_w = font_sys.metrics.cell_width;
        let activity_w = if self.activity_bar_visible {
            44.0 * self.ui_scale
        } else {
            0.0
        };
        let panel_open = self.explorer.visible
            && (self.active_panel != SidebarPanel::Explorer || self.explorer.is_open());
        let sidebar_w = if panel_open {
            match self.active_panel {
                SidebarPanel::Search => {
                    self.explorer.width.max(280.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Problems | SidebarPanel::Plugins | SidebarPanel::Git => {
                    self.explorer.width.max(240.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Explorer => self.explorer.width.min(vp.w - 100.0).max(80.0),
            }
        } else {
            0.0
        };
        let code_vp = EditorViewport {
            x: vp.x + activity_w + sidebar_w,
            y: vp.y,
            w: (vp.w - activity_w - sidebar_w).max(10.0),
            h: vp.h,
        };

        let max_line_digits = (self.active_doc().buffer.len_lines() + 1)
            .to_string()
            .len()
            .max(3);
        let gutter_w = (max_line_digits as f32 + 3.0) * cell_w;
        let content_y = self.content_top(code_vp.y, line_h);
        let status_h = (line_h + 4.0).round();
        let content_h = (code_vp.y + code_vp.h - status_h - content_y).max(1.0);

        // Auto-scroll when dragging mouse selection past top or bottom of viewport
        if py < content_y {
            let diff = ((content_y - py) / line_h).ceil() as usize;
            self.active_doc().scroll_y = self.active_doc().scroll_y.saturating_sub(diff.min(4));
        } else if py > content_y + content_h {
            let diff = ((py - (content_y + content_h)) / line_h).ceil() as usize;
            let max_scroll = self.active_doc().buffer.len_lines().saturating_sub(1);
            self.active_doc().scroll_y = (self.active_doc().scroll_y + diff.min(4)).min(max_scroll);
        }

        self.active_doc().set_cursor_from_pixel(
            px, py, code_vp.x, content_y, gutter_w, line_h, font_sys, true,
        );
    }

    pub fn handle_scrollbar_drag(
        &mut self,
        py: f32,
        vp: &EditorViewport,
        tab_h: f32,
        font_sys: &mut FontSystem,
    ) -> bool {
        if !self.dragging_scrollbar {
            return false;
        }

        let body_vp = self.body_viewport(vp);
        let vp = &body_vp;
        let _ = tab_h;
        let line_h = font_sys.metrics.cell_height;
        let activity_w = if self.activity_bar_visible {
            44.0 * self.ui_scale
        } else {
            0.0
        };
        let panel_open = self.explorer.visible
            && (self.active_panel != SidebarPanel::Explorer || self.explorer.is_open());
        let sidebar_w = if panel_open {
            match self.active_panel {
                SidebarPanel::Search => {
                    self.explorer.width.max(280.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Problems | SidebarPanel::Plugins | SidebarPanel::Git => {
                    self.explorer.width.max(240.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Explorer => self.explorer.width.min(vp.w - 100.0).max(80.0),
            }
        } else {
            0.0
        };
        let code_vp = EditorViewport {
            x: vp.x + activity_w + sidebar_w,
            y: vp.y,
            w: (vp.w - activity_w - sidebar_w).max(10.0),
            h: vp.h,
        };

        let content_y = self.content_top(code_vp.y, line_h);
        let status_h = (line_h + 4.0).round();
        let content_h = (code_vp.y + code_vp.h - status_h - content_y).max(1.0);

        let total_lines = self.active_doc().buffer.len_lines();
        let visible_lines = ((content_h / line_h).ceil() as usize).max(1);
        let thumb_h = (content_h * (visible_lines as f32 / total_lines as f32).clamp(0.04, 1.0))
            .clamp(24.0, content_h);
        let track_avail = (content_h - thumb_h).max(0.0);
        let max_scroll = total_lines.saturating_sub(visible_lines).max(1);

        let ratio = if track_avail > 0.0 {
            ((py - self.scrollbar_drag_offset_y - content_y) / track_avail).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let new_scroll_y = (ratio * max_scroll as f32).round() as usize;
        let changed = self.active_doc().scroll_y != new_scroll_y;
        self.active_doc().scroll_y = new_scroll_y;
        changed
    }

    pub fn update_scrollbar_hover(
        &mut self,
        px: f32,
        py: f32,
        vp: &EditorViewport,
        tab_h: f32,
        font_sys: &mut FontSystem,
    ) -> bool {
        let body_vp = self.body_viewport(vp);
        let vp = &body_vp;
        let _ = tab_h;
        let line_h = font_sys.metrics.cell_height;
        let activity_w = if self.activity_bar_visible {
            44.0 * self.ui_scale
        } else {
            0.0
        };
        let panel_open = self.explorer.visible
            && (self.active_panel != SidebarPanel::Explorer || self.explorer.is_open());
        let sidebar_w = if panel_open {
            match self.active_panel {
                SidebarPanel::Search => {
                    self.explorer.width.max(280.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Problems | SidebarPanel::Plugins | SidebarPanel::Git => {
                    self.explorer.width.max(240.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Explorer => self.explorer.width.min(vp.w - 100.0).max(80.0),
            }
        } else {
            0.0
        };
        let code_vp = EditorViewport {
            x: vp.x + activity_w + sidebar_w,
            y: vp.y,
            w: (vp.w - activity_w - sidebar_w).max(10.0),
            h: vp.h,
        };

        let scrollbar_w = 8.0_f32;
        let scrollbar_x = code_vp.x + code_vp.w - scrollbar_w;
        let content_y = self.content_top(code_vp.y, line_h);
        let status_h = (line_h + 4.0).round();
        let content_h = (code_vp.y + code_vp.h - status_h - content_y).max(1.0);

        let is_hovered = px >= scrollbar_x - 4.0
            && px <= code_vp.x + code_vp.w
            && py >= content_y
            && py <= content_y + content_h;

        let changed = self.scrollbar_hovered != is_hovered;
        self.scrollbar_hovered = is_hovered;
        changed
    }

    pub fn handle_scroll(
        &mut self,
        delta: MouseScrollDelta,
        font_sys: &mut FontSystem,
        px: f32,
        _py: f32,
        vp: &EditorViewport,
        multiplier: f32,
    ) -> bool {
        let body_vp = self.body_viewport(vp);
        let vp = &body_vp;
        let line_h = font_sys.metrics.cell_height;
        let cell_w = font_sys.metrics.cell_width;
        let activity_w = if self.activity_bar_visible {
            44.0 * self.ui_scale
        } else {
            0.0
        };
        let panel_open = self.explorer.visible
            && (self.active_panel != SidebarPanel::Explorer || self.explorer.is_open());
        let sidebar_w = if panel_open {
            match self.active_panel {
                SidebarPanel::Search => {
                    self.explorer.width.max(280.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Problems | SidebarPanel::Plugins | SidebarPanel::Git => {
                    self.explorer.width.max(240.0).min((vp.w - 160.0).max(80.0))
                }
                SidebarPanel::Explorer => self.explorer.width.min(vp.w - 100.0).max(80.0),
            }
        } else {
            0.0
        };
        let sidebar_x = vp.x + activity_w;

        if px >= vp.x && px < vp.x + activity_w {
            return true;
        }

        if panel_open && px >= sidebar_x && px < sidebar_x + sidebar_w {
            if self.active_panel == SidebarPanel::Explorer {
                match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        self.explorer.scroll(y * multiplier);
                    }
                    MouseScrollDelta::PixelDelta(p) => {
                        self.explorer.scroll_pixels(p.y as f32 * multiplier);
                    }
                }
            }
            return true;
        }

        if self.active_doc().is_settings {
            let dy = match delta {
                MouseScrollDelta::LineDelta(_, y) => y * multiplier * 24.0,
                MouseScrollDelta::PixelDelta(p) => p.y as f32 * multiplier,
            };
            self.settings_state.scroll_y = (self.settings_state.scroll_y - dy).max(0.0);
            return true;
        }

        match delta {
            MouseScrollDelta::LineDelta(dx, dy) => {
                let lines = (dy * multiplier * 3.0).round() as i32;
                let cols = (dx * multiplier * 4.0).round() as i32;

                let doc = self.active_doc();
                let max_scroll = doc.buffer.len_lines().saturating_sub(1);

                if lines > 0 {
                    doc.scroll_y = doc.scroll_y.saturating_sub(lines as usize);
                } else if lines < 0 {
                    doc.scroll_y = (doc.scroll_y + (-lines as usize)).min(max_scroll);
                }

                if cols > 0 {
                    doc.scroll_x = (doc.scroll_x - cols as f32 * cell_w).max(0.0);
                } else if cols < 0 {
                    doc.scroll_x += (-cols as f32) * cell_w;
                }
            }
            MouseScrollDelta::PixelDelta(pos) => {
                self.scroll_accum_y += pos.y * multiplier as f64;
                self.scroll_accum_x += pos.x * multiplier as f64;

                let cell_h = line_h as f64;
                let steps = (self.scroll_accum_y / cell_h).trunc() as i32;
                if steps != 0 {
                    self.scroll_accum_y -= steps as f64 * cell_h;
                }

                let doc = self.active_doc();
                let max_scroll = doc.buffer.len_lines().saturating_sub(1);
                if steps > 0 {
                    doc.scroll_y = doc.scroll_y.saturating_sub(steps as usize);
                } else if steps < 0 {
                    doc.scroll_y = (doc.scroll_y + (-steps as usize)).min(max_scroll);
                }

                doc.scroll_x = (doc.scroll_x - (pos.x as f32 * multiplier)).max(0.0);
            }
        }

        true
    }

    fn notify_doc_change(&mut self) {
        let path = self.active_doc().path().cloned();
        let text = self.active_doc().buffer.rope.to_string();
        let rev = self.active_doc().buffer.revision as i64 + 1;

        if let Some(p) = path {
            self.lsp.did_change(&p, &text, rev);
        }
        self.plugins.sync_buffer_text(&text);
    }

    fn trigger_autocompletion(&mut self, font_sys: &mut FontSystem) {
        if let Some(p) = self.active_doc().path().cloned() {
            let (line, character) = self.active_doc().cursor_utf16_position(font_sys);
            if self.lsp.request_completion(&p, line, character) {
                self.completion_request = Some(p);
                return;
            }
        }

        // Buffer words fallback when no language server is available.
        let cur_word = self.get_word_under_cursor();
        let mut items = Vec::new();
        let buffer_text = self.active_doc().buffer.rope.to_string();
        let mut seen = std::collections::HashSet::new();
        for word in buffer_text.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if word.len() > 2
                && word.starts_with(&cur_word)
                && word != cur_word
                && seen.insert(word.to_string())
            {
                items.push(crate::editor::lsp::EditorCompletionItem {
                    label: word.to_string(),
                    detail: Some("buffer".to_string()),
                    insert_text: word.to_string(),
                    kind: None,
                });
                if items.len() >= 12 {
                    break;
                }
            }
        }

        if !items.is_empty() {
            let mut overlay = CompletionOverlay::new();
            overlay.set_items(items);
            self.completion = Some(overlay);
        }
    }

    fn trigger_hover(&mut self, font_sys: &mut FontSystem) {
        if let Some(p) = self.active_doc().path().cloned() {
            let (line, character) = self.active_doc().cursor_utf16_position(font_sys);
            let doc_line = self.active_doc().cursor.line;
            let cluster = self.active_doc().cursor.cluster_idx;
            if self.lsp.request_hover(&p, line, character) {
                self.hover_request = Some((p, doc_line, cluster));
            }
        }
    }

    fn get_word_under_cursor(&mut self) -> String {
        let line = self.active_doc().cursor.line;
        let line_str = self.active_doc().buffer.line_without_newline(line);
        let cluster = self.active_doc().cursor.cluster_idx;
        let chars: Vec<char> = line_str.chars().collect();
        if chars.is_empty() {
            return String::new();
        }
        let col = cluster.min(chars.len());
        let mut start = col;
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }
        chars[start..col].iter().collect()
    }

    fn update_find_matches(&mut self) {
        let q = self.find_bar.query.clone();
        if q.is_empty() {
            self.find_bar.match_count = 0;
            self.find_bar.current_match = 0;
            return;
        }
        let matches = self.active_doc().buffer.search(&q, false);
        self.find_bar.match_count = matches.len();
        self.find_bar.current_match = if !matches.is_empty() { 1 } else { 0 };
    }

    fn jump_to_next_match(&mut self, font_sys: &mut FontSystem, reverse: bool) {
        let q = self.find_bar.query.clone();
        if q.is_empty() {
            return;
        }
        let matches = self.active_doc().buffer.search(&q, false);
        if matches.is_empty() {
            return;
        }

        let cur_char = self.active_doc().cursor_char_offset(font_sys);
        let target_match = if reverse {
            matches
                .iter()
                .rev()
                .find(|(s, _)| *s < cur_char)
                .or_else(|| matches.last())
        } else {
            matches
                .iter()
                .find(|(s, _)| *s > cur_char)
                .or_else(|| matches.first())
        };

        if let Some(&(start_char, end_char)) = target_match {
            let match_index = matches
                .iter()
                .position(|(start, _)| *start == start_char)
                .map(|index| index + 1)
                .unwrap_or(1);
            self.find_bar.current_match = match_index;

            let (line, col) = self.active_doc().buffer.char_to_line_col(start_char);
            let shaped = self.active_doc().shaped_line(line, font_sys);
            let cluster = shaped.char_to_cluster(col);

            let (end_line, end_col) = self.active_doc().buffer.char_to_line_col(end_char);
            let end_shaped = self.active_doc().shaped_line(end_line, font_sys);
            let end_cluster = end_shaped.char_to_cluster(end_col);

            let doc = self.active_doc();
            doc.selection_anchor = Some(crate::editor::document::Cursor::new(line, cluster));
            doc.cursor = crate::editor::document::Cursor::new(end_line, end_cluster);
            doc.cursor.preferred_visual_x = None;
            doc.scroll_to_cursor_requested = true;
        }
    }

    fn execute_palette_action(
        &mut self,
        action: PaletteAction,
        proxy: EventLoopProxy<AppEvent>,
        font_sys: &mut FontSystem,
    ) {
        match action {
            PaletteAction::OpenFile(path) => {
                let _ = self.open_file(path, proxy);
            }
            PaletteAction::SaveFile => {
                let _ = self.active_doc().save();
            }
            PaletteAction::NewFile => {
                self.new_document();
            }
            PaletteAction::CloseFile => {
                self.request_close_active();
            }
            PaletteAction::FindInFile => {
                self.find_bar.open();
            }
            PaletteAction::ToggleTerminalSplit => {
                self.mode = match self.mode {
                    WorkspaceMode::Split => WorkspaceMode::PureEditor,
                    _ => WorkspaceMode::Split,
                };
            }
            PaletteAction::SetWorkspaceMode(m) => {
                self.mode = m;
            }
            PaletteAction::ReloadPlugins => {
                self.plugins.reload_plugins();
            }
            PaletteAction::RunPluginCommand(plugin, func) => {
                let _ = self.plugins.execute_command(&plugin, &func);
            }
            PaletteAction::TriggerLspCompletion => {
                self.trigger_autocompletion(font_sys);
            }
            PaletteAction::RestartLsp => {
                if let Some(path) = self.active_doc().path().cloned() {
                    let text = self.active_doc().buffer.rope.to_string();
                    let workspace = self.workspace_dir.clone();
                    if self.lsp.restart_for(&path, &workspace, proxy) {
                        self.lsp.did_open(&path, &text, 1);
                    }
                }
            }
            PaletteAction::ToggleFileExplorer => {
                self.explorer.toggle_visible();
            }
            PaletteAction::OpenEditorWindow => {
                if let Ok(exe) = std::env::current_exe() {
                    let mut cmd = std::process::Command::new(exe);
                    cmd.arg("--editor-window");
                    let path = self.active_doc_ref().path().cloned().or_else(|| {
                        (!self.workspace_dir.as_os_str().is_empty())
                            .then(|| self.workspace_dir.clone())
                    });
                    if let Some(path) = path {
                        cmd.arg(path);
                    }
                    let _ = cmd.spawn();
                }
            }
            PaletteAction::ShowGitChanges => {
                self.active_panel = SidebarPanel::Git;
                self.explorer.visible = true;
                self.refresh_git();
            }
            PaletteAction::ShowGitDiff => {
                self.show_git_diff();
            }
            PaletteAction::GitRefresh => {
                self.refresh_git();
            }
        }
    }

    fn collect_problems(&self) -> Vec<ProblemEntry> {
        let mut problems: Vec<ProblemEntry> = Vec::new();
        for document in &self.documents {
            if let Some(document_path) = document.path() {
                let file_name = document.file_name();
                for diag in self.lsp.diagnostics_for(document_path) {
                    problems.push(ProblemEntry {
                        path: document_path.clone(),
                        file_name: file_name.clone(),
                        line: diag.start_line,
                        col: diag.start_col,
                        severity: diag.severity,
                        message: diag.message.clone(),
                    });
                }
            }
        }
        problems.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        problems
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit_scene(
        &mut self,
        config: &Config,
        font_sys: &mut FontSystem,
        atlas: &mut GlyphAtlas,
        bg_instances: &mut Vec<QuadInstance>,
        fg_instances: &mut Vec<QuadInstance>,
        overlay_instances: &mut Vec<QuadInstance>,
        vp: &EditorViewport,
        cursor_blink: bool,
        ime_preedit: Option<&str>,
    ) {
        if !self.is_active() {
            return;
        }

        let is_focused = self.editor_focused;
        let active_idx = self.active_doc_idx;
        let path = self
            .documents
            .get(active_idx)
            .and_then(|d| d.path().cloned());
        let mut diags = path
            .as_ref()
            .map(|p| self.lsp.diagnostics_for(p).to_vec())
            .unwrap_or_default();

        for diag in &mut diags {
            let start = self.documents[active_idx].utf16_col_to_cluster(
                diag.start_line,
                diag.start_col,
                font_sys,
            );
            let end = self.documents[active_idx].utf16_col_to_cluster(
                diag.end_line,
                diag.end_col,
                font_sys,
            );
            diag.start_col = start;
            diag.end_col = end;
        }

        let lsp_status_owned = path.as_ref().and_then(|p| self.lsp.status_for(p));
        let lsp_status = lsp_status_owned.as_deref().or(Some("LSP: Idle"));
        let plugin_items = self.plugins.status_items();

        let problems = self.collect_problems();
        let plugin_infos = self.plugins.plugin_infos().to_vec();

        let doc_titles: Vec<(String, bool)> = self
            .documents
            .iter()
            .map(|d| (d.file_name(), d.is_dirty()))
            .collect();

        let ime_preedit = if self.accepts_text_input() {
            ime_preedit
        } else {
            None
        };
        let vim_view = if self.vim.enabled {
            Some(&self.vim)
        } else {
            None
        };
        let ui_scale = self.ui_scale;

        let doc = &mut self.documents[active_idx];

        EditorView::emit_scene(
            config,
            font_sys,
            atlas,
            bg_instances,
            fg_instances,
            overlay_instances,
            doc,
            &doc_titles,
            active_idx,
            vp,
            cursor_blink,
            is_focused,
            self.palette.as_ref(),
            self.completion.as_ref(),
            Some(&self.find_bar),
            self.hover.as_ref(),
            &diags,
            &plugin_items,
            lsp_status,
            Some(&self.explorer),
            self.dragging_scrollbar,
            self.scrollbar_hovered,
            &problems,
            &plugin_infos,
            &self.search,
            &self.git,
            self.active_panel,
            self.activity_bar_visible,
            vim_view,
            ime_preedit,
            ui_scale,
            self.menu_height,
            self.metrics_label.as_deref(),
            Some(&self.settings_state),
        );
    }
}

fn char_eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

fn is_search_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn find_matches(
    line: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    let hay: Vec<char> = line.chars().collect();
    let needle: Vec<char> = query.chars().collect();
    if needle.is_empty() || hay.len() < needle.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        let matched = (0..needle.len()).all(|offset| {
            if case_sensitive {
                hay[i + offset] == needle[offset]
            } else {
                char_eq_ci(hay[i + offset], needle[offset])
            }
        });
        let word_ok = !whole_word
            || ((i == 0 || !is_search_word_char(hay[i - 1]))
                && (i + needle.len() == hay.len() || !is_search_word_char(hay[i + needle.len()])));
        if matched && word_ok {
            matches.push((i, i + needle.len()));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    matches
}

fn replace_in_text(
    text: &str,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> (String, usize) {
    let hay: Vec<char> = text.chars().collect();
    let needle: Vec<char> = query.chars().collect();
    if needle.is_empty() {
        return (text.to_string(), 0);
    }

    let mut output = String::with_capacity(text.len());
    let mut i = 0;
    let mut last = 0;
    let mut count = 0;
    while i + needle.len() <= hay.len() {
        let matched = (0..needle.len()).all(|offset| {
            if case_sensitive {
                hay[i + offset] == needle[offset]
            } else {
                char_eq_ci(hay[i + offset], needle[offset])
            }
        });
        let word_ok = !whole_word
            || ((i == 0 || !is_search_word_char(hay[i - 1]))
                && (i + needle.len() == hay.len() || !is_search_word_char(hay[i + needle.len()])));
        if matched && word_ok {
            output.extend(hay[last..i].iter());
            output.push_str(replacement);
            i += needle.len();
            last = i;
            count += 1;
        } else {
            i += 1;
        }
    }
    output.extend(hay[last..].iter());
    (output, count)
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ocean");
    let tmp = path.with_file_name(format!(".{name}.{}.ocean-tmp", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|err| err.to_string())?;
    std::fs::rename(&tmp, path).map_err(|err| err.to_string())
}

#[derive(Default)]
struct VimOutcome {
    changed: bool,
    save: bool,
    close: bool,
    confirm_close: bool,
    open_find: bool,
    next_match: Option<bool>,
}

fn key_text(key: &Key) -> Option<&str> {
    match key {
        Key::Character(text) => Some(text.as_str()),
        _ => None,
    }
}

fn vim_process(
    vim: &mut VimState,
    doc: &mut EditorDocument,
    event: &KeyEvent,
    modifiers: ModifiersState,
    font_sys: &mut FontSystem,
) -> VimOutcome {
    let mut out = VimOutcome::default();
    let ctrl = modifiers.control_key();
    let key = &event.logical_key;
    let text = key_text(key);

    if matches!(key, Key::Named(NamedKey::Escape)) {
        if vim.mode.is_visual() {
            doc.selection_anchor = None;
            doc.selection_kind = SelectionKind::Char;
        }
        doc.clear_extra_cursors();
        vim.reset_to_normal();
        return out;
    }

    if vim.pending_replace {
        vim.pending_replace = false;
        if let Some(ch) = text.and_then(|s| s.chars().next()) {
            doc.replace_char(ch, font_sys);
            out.changed = true;
        }
        vim.count.clear();
        return out;
    }

    if vim.mode == VimMode::Command {
        match key {
            Key::Named(NamedKey::Enter) => {
                let command = vim.command_line.take().unwrap_or_default();
                vim.mode = VimMode::Normal;
                execute_vim_ex(&command, doc, vim, font_sys, &mut out);
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some(line) = &mut vim.command_line {
                    line.pop();
                }
            }
            Key::Character(s) if !ctrl => {
                if let Some(line) = &mut vim.command_line {
                    line.push_str(s);
                }
            }
            _ => {}
        }
        return out;
    }

    if ctrl {
        if let Some(s) = text {
            if s.eq_ignore_ascii_case("r") {
                doc.clear_extra_cursors();
                doc.redo(font_sys);
                out.changed = true;
            } else if s.eq_ignore_ascii_case("d") {
                for _ in 0..12 {
                    doc.move_down(vim.mode.is_visual(), font_sys);
                }
            } else if s.eq_ignore_ascii_case("u") {
                for _ in 0..12 {
                    doc.move_up(vim.mode.is_visual(), font_sys);
                }
            }
        }
        return out;
    }

    if let Some(s) = text
        && !s.is_empty()
        && s.chars().all(|c| c.is_ascii_digit())
        && !(s == "0" && vim.count.is_empty())
    {
        vim.count.push_str(s);
        return out;
    }

    if text == Some("g") {
        if vim.pending_g {
            vim.pending_g = false;
            let count = vim.take_count();
            doc.move_to_line(count.saturating_sub(1), font_sys);
        } else {
            vim.pending_g = true;
        }
        return out;
    }
    vim.pending_g = false;

    let visual = vim.mode.is_visual();
    let select = visual;

    // Visual-mode operators
    if visual && matches!(text, Some("d") | Some("x") | Some("y") | Some("c")) {
        let linewise = doc.selection_kind == SelectionKind::Line;
        if let Some((start, end)) = doc.visual_char_range(font_sys) {
            let op = text.unwrap_or("d");
            if op == "y" {
                let yanked = doc.buffer.slice_to_string(start, end);
                vim.register = Some((yanked, linewise));
            } else {
                let removed = doc.delete_char_range(start, end, font_sys);
                vim.register = Some((removed, linewise));
                out.changed = true;
                if op == "c" {
                    vim.mode = VimMode::Insert;
                }
            }
        }
        doc.selection_anchor = None;
        doc.selection_kind = SelectionKind::Char;
        if vim.mode != VimMode::Insert {
            vim.reset_to_normal();
        }
        vim.count.clear();
        return out;
    }

    // Pending operator followed by a motion
    if let Some(op) = vim.operator.take() {
        let count = (vim.operator_count.max(1) * vim.peek_count()).max(1);
        let line = doc.cursor.line;
        let start = doc.cursor_char_offset(font_sys);

        if matches!(op, VimOperator::Indent | VimOperator::Outdent) {
            let lines = match text {
                Some(">") | Some("<") => count,
                Some("j") | Some("k") => count + 1,
                _ => 0,
            };
            if lines > 0 {
                let start_line = if text == Some("k") {
                    line.saturating_sub(count)
                } else {
                    line
                };
                doc.indent_lines(start_line, lines, op == VimOperator::Outdent, font_sys);
                out.changed = true;
            }
            vim.operator = None;
            vim.operator_count = 1;
            vim.count.clear();
            return out;
        }

        let mut linewise = false;
        let range = match text {
            Some("d") | Some("y") | Some("c") => {
                linewise = true;
                let s = doc.line_start_char(line);
                let end_line = (line + count).min(doc.buffer.len_lines());
                let e = if end_line < doc.buffer.len_lines() {
                    doc.line_start_char(end_line)
                } else {
                    doc.buffer.len_chars()
                };
                Some((s, e))
            }
            Some("w") => {
                doc.move_word_forward(count, false, font_sys);
                let end = doc.cursor_char_offset(font_sys);
                Some((start.min(end), start.max(end)))
            }
            Some("b") => {
                doc.move_word_backward(count, false, font_sys);
                let end = doc.cursor_char_offset(font_sys);
                Some((start.min(end), start.max(end)))
            }
            Some("e") => {
                doc.move_word_end(count, false, font_sys);
                let end =
                    (doc.cursor_char_offset(font_sys) + 1).min(doc.line_end_char(doc.cursor.line));
                Some((start.min(end), start.max(end)))
            }
            Some("$") => Some((start, doc.line_end_char(line))),
            Some("0") => Some((doc.line_start_char(line), start)),
            Some("j") => {
                linewise = true;
                let s = doc.line_start_char(line);
                let end_line = (line + count + 1).min(doc.buffer.len_lines());
                let e = if end_line < doc.buffer.len_lines() {
                    doc.line_start_char(end_line)
                } else {
                    doc.buffer.len_chars()
                };
                Some((s, e))
            }
            Some("k") => {
                linewise = true;
                let start_line = line.saturating_sub(count);
                let s = doc.line_start_char(start_line);
                let end_line = (line + 1).min(doc.buffer.len_lines());
                let e = if end_line < doc.buffer.len_lines() {
                    doc.line_start_char(end_line)
                } else {
                    doc.buffer.len_chars()
                };
                Some((s, e))
            }
            _ => None,
        };

        if let Some((range_start, range_end)) = range {
            match op {
                VimOperator::Delete => {
                    let removed = doc.delete_char_range(range_start, range_end, font_sys);
                    if !removed.is_empty() {
                        vim.register = Some((removed, linewise));
                        out.changed = true;
                    }
                }
                VimOperator::Yank => {
                    let yanked = doc.buffer.slice_to_string(range_start, range_end);
                    vim.register = Some((yanked, linewise));
                    doc.set_cursor_char_offset(range_start, font_sys);
                }
                VimOperator::Change => {
                    let removed = doc.delete_char_range(range_start, range_end, font_sys);
                    vim.register = Some((removed, linewise));
                    out.changed = true;
                    vim.mode = VimMode::Insert;
                }
                VimOperator::Indent | VimOperator::Outdent => {}
            }
        }

        vim.operator = None;
        vim.operator_count = 1;
        vim.count.clear();
        return out;
    }

    match text {
        Some("h") => doc.move_left(select, font_sys),
        Some("l") => doc.move_right(select, font_sys),
        Some("j") => doc.move_down(select, font_sys),
        Some("k") => doc.move_up(select, font_sys),
        Some("w") => doc.move_word_forward(vim.take_count(), select, font_sys),
        Some("b") => doc.move_word_backward(vim.take_count(), select, font_sys),
        Some("e") => doc.move_word_end(vim.take_count(), select, font_sys),
        Some("0") => doc.move_home(select, font_sys),
        Some("^") => doc.move_first_non_blank(select, font_sys),
        Some("$") => doc.move_last_char(select, font_sys),
        Some("G") => {
            let count = vim.take_count();
            let target = if count > 1 {
                count - 1
            } else {
                doc.buffer.len_lines().saturating_sub(1)
            };
            doc.move_to_line(target, font_sys);
        }
        Some("i") => {
            vim.mode = VimMode::Insert;
            vim.count.clear();
        }
        Some("a") => {
            if !visual {
                doc.move_right(false, font_sys);
            }
            doc.selection_anchor = None;
            vim.mode = VimMode::Insert;
            vim.count.clear();
        }
        Some("I") => {
            doc.move_first_non_blank(false, font_sys);
            vim.mode = VimMode::Insert;
            vim.count.clear();
        }
        Some("A") => {
            doc.move_end(false, font_sys);
            vim.mode = VimMode::Insert;
            vim.count.clear();
        }
        Some("o") => {
            doc.move_end(false, font_sys);
            doc.insert_text("\n", font_sys);
            out.changed = true;
            vim.mode = VimMode::Insert;
            vim.count.clear();
        }
        Some("O") => {
            let start = doc.line_start_char(doc.cursor.line);
            doc.set_cursor_char_offset(start, font_sys);
            doc.insert_text("\n", font_sys);
            doc.move_up(false, font_sys);
            out.changed = true;
            vim.mode = VimMode::Insert;
            vim.count.clear();
        }
        Some("v") => {
            doc.selection_anchor = Some(doc.cursor);
            doc.selection_kind = SelectionKind::Char;
            vim.mode = VimMode::Visual;
            vim.count.clear();
        }
        Some("V") => {
            doc.selection_anchor = Some(doc.cursor);
            doc.selection_kind = SelectionKind::Line;
            vim.mode = VimMode::VisualLine;
            vim.count.clear();
        }
        Some("d") => {
            vim.operator = Some(VimOperator::Delete);
            vim.operator_count = vim.peek_count();
            vim.count.clear();
        }
        Some("y") => {
            vim.operator = Some(VimOperator::Yank);
            vim.operator_count = vim.peek_count();
            vim.count.clear();
        }
        Some("c") => {
            vim.operator = Some(VimOperator::Change);
            vim.operator_count = vim.peek_count();
            vim.count.clear();
        }
        Some("x") => {
            let count = vim.take_count();
            let mut removed = String::new();
            for _ in 0..count {
                removed.push_str(&doc.delete_next_cluster(font_sys));
            }
            if !removed.is_empty() {
                vim.register = Some((removed, false));
                out.changed = true;
            }
        }
        Some("X") => {
            let count = vim.take_count();
            let mut removed = String::new();
            for _ in 0..count {
                removed.push_str(&doc.delete_prev_cluster(font_sys));
            }
            if !removed.is_empty() {
                vim.register = Some((removed, false));
                out.changed = true;
            }
        }
        Some("p") | Some("P") => {
            let before = text == Some("P");
            if let Some((payload, linewise)) = vim.register.clone() {
                doc.put_text(&payload, linewise, before, font_sys);
                out.changed = true;
            }
            vim.count.clear();
        }
        Some("J") => {
            doc.join_lines(vim.take_count(), font_sys);
            out.changed = true;
        }
        Some("u") => {
            doc.clear_extra_cursors();
            doc.undo(font_sys);
            out.changed = true;
            vim.count.clear();
        }
        Some("r") => {
            vim.pending_replace = true;
            vim.count.clear();
        }
        Some("~") => {
            doc.toggle_case(font_sys);
            out.changed = true;
            vim.count.clear();
        }
        Some(">") => {
            if visual {
                if let Some((start, end)) = doc.visual_char_range(font_sys) {
                    let start_line = doc.buffer.char_to_line_col(start).0;
                    let end_char = end
                        .saturating_sub(1)
                        .min(doc.buffer.len_chars().saturating_sub(1));
                    let end_line = doc.buffer.char_to_line_col(end_char).0;
                    doc.indent_lines(start_line, end_line - start_line + 1, false, font_sys);
                    out.changed = true;
                }
                doc.selection_anchor = None;
                doc.selection_kind = SelectionKind::Char;
                vim.reset_to_normal();
            } else {
                vim.operator = Some(VimOperator::Indent);
                vim.operator_count = vim.peek_count();
                vim.count.clear();
            }
        }
        Some("<") => {
            if visual {
                if let Some((start, end)) = doc.visual_char_range(font_sys) {
                    let start_line = doc.buffer.char_to_line_col(start).0;
                    let end_char = end
                        .saturating_sub(1)
                        .min(doc.buffer.len_chars().saturating_sub(1));
                    let end_line = doc.buffer.char_to_line_col(end_char).0;
                    doc.indent_lines(start_line, end_line - start_line + 1, true, font_sys);
                    out.changed = true;
                }
                doc.selection_anchor = None;
                doc.selection_kind = SelectionKind::Char;
                vim.reset_to_normal();
            } else {
                vim.operator = Some(VimOperator::Outdent);
                vim.operator_count = vim.peek_count();
                vim.count.clear();
            }
        }
        Some(":") => {
            vim.command_line = Some(String::new());
            vim.mode = VimMode::Command;
            vim.count.clear();
        }
        Some("/") => {
            out.open_find = true;
            vim.reset_to_normal();
        }
        Some("n") => {
            out.next_match = Some(false);
            vim.count.clear();
        }
        Some("N") => {
            out.next_match = Some(true);
            vim.count.clear();
        }
        _ => {
            vim.count.clear();
            vim.operator = None;
        }
    }

    out
}

fn execute_vim_ex(
    command: &str,
    doc: &mut EditorDocument,
    vim: &mut VimState,
    font_sys: &mut FontSystem,
    out: &mut VimOutcome,
) {
    let command = command.trim();
    match command {
        "w" => out.save = true,
        "wq" | "wq!" | "x" => {
            out.save = true;
            out.close = true;
        }
        "q" => {
            if doc.is_dirty() {
                out.confirm_close = true;
            } else {
                out.close = true;
            }
        }
        "q!" => out.close = true,
        _ => {
            if let Ok(line) = command.parse::<usize>() {
                doc.move_to_line(line.saturating_sub(1), font_sys);
            } else {
                vim.set_status(format!("E492: Not an editor command: {command}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    #[test]
    fn test_editor_scroll_wheel_and_pixel() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let mut ws = EditorWorkspace::new();
        ws.mode = WorkspaceMode::PureEditor;

        // Populate doc with 100 lines
        let text = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        ws.active_doc().insert_text(&text, &mut font_sys);
        assert_eq!(ws.active_doc().buffer.len_lines(), 100);
        ws.active_doc().scroll_y = 0;

        let vp = EditorViewport {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        };

        // Test notched mouse wheel scrolling down
        let res = ws.handle_scroll(
            MouseScrollDelta::LineDelta(0.0, -2.0),
            &mut font_sys,
            200.0,
            200.0,
            &vp,
            1.0,
        );
        assert!(res);
        assert_eq!(ws.active_doc().scroll_y, 6); // 2.0 * 3.0 = 6 lines

        // Test notched mouse wheel scrolling up
        ws.handle_scroll(
            MouseScrollDelta::LineDelta(0.0, 1.0),
            &mut font_sys,
            200.0,
            200.0,
            &vp,
            1.0,
        );
        assert_eq!(ws.active_doc().scroll_y, 3); // 6 - 3 = 3

        // Test pixel accumulator on touchpad
        let cell_h = font_sys.metrics.cell_height as f64;
        // Half cell step should accumulate without dropping
        ws.handle_scroll(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -(cell_h * 0.6))),
            &mut font_sys,
            200.0,
            200.0,
            &vp,
            1.0,
        );
        assert_eq!(ws.active_doc().scroll_y, 3); // not yet 1 full cell
        // Second half cell step should trigger 1 line scroll
        ws.handle_scroll(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -(cell_h * 0.6))),
            &mut font_sys,
            200.0,
            200.0,
            &vp,
            1.0,
        );
        assert_eq!(ws.active_doc().scroll_y, 4);

        // Test scrollbar hover and drag
        let scrollbar_hover = ws.update_scrollbar_hover(795.0, 300.0, &vp, 0.0, &mut font_sys);
        assert!(scrollbar_hover);
        assert!(ws.scrollbar_hovered);

        ws.dragging_scrollbar = true;
        ws.scrollbar_drag_offset_y = 10.0;
        let drag_res = ws.handle_scrollbar_drag(300.0, &vp, 0.0, &mut font_sys);
        assert!(drag_res);
        assert!(ws.active_doc().scroll_y > 10);
    }

    #[test]
    fn find_next_match_requests_auto_scroll() {
        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let text = (0..60)
            .map(|line| {
                if line == 42 {
                    "needle here".to_string()
                } else {
                    format!("line {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut ws = EditorWorkspace::new();
        ws.documents[0] = EditorDocument::from_text(&text, None, false);
        ws.documents[0].scroll_to_cursor_requested = false;
        ws.find_bar.query = "needle".to_string();
        ws.update_find_matches();

        ws.jump_to_next_match(&mut font_sys, false);

        assert_eq!(ws.active_doc_ref().cursor.line, 42);
        assert!(ws.active_doc_ref().scroll_to_cursor_requested);
        assert_eq!(ws.find_bar.current_match, 1);
    }

    #[test]
    fn explorer_hover_uses_menu_offset_with_clean_sidebar() {
        let mut ws = EditorWorkspace::new();
        ws.activity_bar_visible = false;
        ws.menu_height = 10.0;
        ws.explorer.visible = true;
        ws.explorer.root_name = "workspace".to_string();
        ws.explorer.width = 210.0;
        ws.explorer.entries = (0..3)
            .map(|i| ExplorerEntry {
                path: PathBuf::from(format!("file{i}.rs")),
                name: format!("file{i}.rs"),
                is_dir: false,
                depth: 0,
                is_expanded: false,
            })
            .collect();

        let vp = EditorViewport {
            x: 0.0,
            y: 20.0,
            w: 800.0,
            h: 500.0,
        };
        let (sx, sy, sw, _) = ws.sidebar_rect(&vp).expect("explorer sidebar rect");
        assert_eq!((sx, sy, sw), (0.0, 30.0, 210.0));

        let config = Config::default();
        let mut font_sys = FontSystem::new(&config, 1.0);
        let header_h = (font_sys.metrics.cell_height + 6.0).round();
        let item_h = (font_sys.metrics.cell_height + 3.0).round();
        ws.update_explorer_hover(sx + 12.0, sy + header_h + item_h * 1.5, &vp, &mut font_sys);
        assert_eq!(ws.explorer.hovered_idx, Some(1));

        ws.update_explorer_hover(sx + 12.0, sy + 2.0, &vp, &mut font_sys);
        assert_eq!(ws.explorer.hovered_idx, None);
    }

    #[test]
    fn test_workspace_search_matches_case_and_whole_word() {
        assert_eq!(
            find_matches("let foo = Foo;", "foo", false, false),
            vec![(4, 7), (10, 13)]
        );
        assert_eq!(
            find_matches("let foo = Foo;", "foo", false, true),
            vec![(4, 7), (10, 13)]
        );
        assert_eq!(
            find_matches("let foo = Foo;", "foo", true, false),
            vec![(4, 7)]
        );
    }

    #[test]
    fn test_workspace_search_replace_preserves_myanmar_text() {
        let (updated, count) = replace_in_text("မင်္ဂလာပါ foo", "foo", "ကမ္ဘာ", false, true);
        assert_eq!(count, 1);
        assert_eq!(updated, "မင်္ဂလာပါ ကမ္ဘာ");
    }
}
