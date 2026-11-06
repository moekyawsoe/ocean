use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher, Utf32Str};

use crate::editor::lsp::EditorCompletionItem;

#[derive(Debug, Clone)]
pub enum PaletteAction {
    OpenFile(PathBuf),
    SaveFile,
    CloseFile,
    NewFile,
    FindInFile,
    ToggleTerminalSplit,
    SetWorkspaceMode(crate::editor::WorkspaceMode),
    ReloadPlugins,
    RunPluginCommand(String, String),
    TriggerLspCompletion,
    RestartLsp,
    ToggleFileExplorer,
}

#[derive(Debug, Clone)]
pub struct PaletteItem {
    pub title: String,
    pub description: String,
    pub action: PaletteAction,
}

pub struct CommandPalette {
    pub query: String,
    pub selected: usize,
    pub all_items: Vec<PaletteItem>,
    pub filtered_indices: Vec<(usize, u32)>, // (index in all_items, score)
    matcher: Matcher,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            all_items: Vec::new(),
            filtered_indices: Vec::new(),
            matcher: Matcher::new(MatcherConfig::DEFAULT),
        }
    }

    pub fn populate_items(
        &mut self,
        workspace_dir: &Path,
        plugin_commands: &[(String, String, String)],
    ) {
        self.all_items.clear();

        // Built-in editor actions
        self.all_items.push(PaletteItem {
            title: "File: Save Current File (Ctrl+S)".to_string(),
            description: "Save active document to disk".to_string(),
            action: PaletteAction::SaveFile,
        });
        self.all_items.push(PaletteItem {
            title: "File: New Empty Document".to_string(),
            description: "Create an untitled scratch document".to_string(),
            action: PaletteAction::NewFile,
        });
        self.all_items.push(PaletteItem {
            title: "File: Close Active Document (Ctrl+W)".to_string(),
            description: "Close active editor tab".to_string(),
            action: PaletteAction::CloseFile,
        });
        self.all_items.push(PaletteItem {
            title: "Edit: Find / Search in File (Ctrl+F)".to_string(),
            description: "Open in-buffer interactive search bar".to_string(),
            action: PaletteAction::FindInFile,
        });
        self.all_items.push(PaletteItem {
            title: "View: Toggle File Explorer Sidebar (Ctrl+B)".to_string(),
            description: "Show or hide file tree explorer sidebar".to_string(),
            action: PaletteAction::ToggleFileExplorer,
        });
        self.all_items.push(PaletteItem {
            title: "View: Toggle Terminal Split (Mode B)".to_string(),
            description: "Split viewport between Code Editor and Terminal".to_string(),
            action: PaletteAction::ToggleTerminalSplit,
        });
        self.all_items.push(PaletteItem {
            title: "View: Pure Editor Viewport (Mode C)".to_string(),
            description: "Maximize editor to entire window".to_string(),
            action: PaletteAction::SetWorkspaceMode(crate::editor::WorkspaceMode::PureEditor),
        });
        self.all_items.push(PaletteItem {
            title: "View: Pure Terminal Mode (Mode A)".to_string(),
            description: "Switch to full terminal emulator".to_string(),
            action: PaletteAction::SetWorkspaceMode(crate::editor::WorkspaceMode::PureTerminal),
        });
        self.all_items.push(PaletteItem {
            title: "LSP: Trigger Autocompletion (Ctrl+Space)".to_string(),
            description: "Request language server code completions".to_string(),
            action: PaletteAction::TriggerLspCompletion,
        });
        self.all_items.push(PaletteItem {
            title: "LSP: Restart Language Server".to_string(),
            description: "Restart the language server for the active file".to_string(),
            action: PaletteAction::RestartLsp,
        });
        self.all_items.push(PaletteItem {
            title: "Plugins: Reload WASM Plugins".to_string(),
            description: "Reload plugins from ~/.config/ocean/plugins/".to_string(),
            action: PaletteAction::ReloadPlugins,
        });

        // Add plugin commands
        for (plugin, title, callback) in plugin_commands {
            self.all_items.push(PaletteItem {
                title: format!("Plugin: {title}"),
                description: format!("{plugin} — execute `{callback}`"),
                action: PaletteAction::RunPluginCommand(plugin.clone(), callback.clone()),
            });
        }

        // Walk workspace directory for files (respecting .gitignore)
        let walker = WalkBuilder::new(workspace_dir)
            .hidden(true)
            .git_ignore(true)
            .max_depth(Some(8))
            .build();

        for entry in walker.flatten() {
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                let p = entry.into_path();
                let rel = p
                    .strip_prefix(workspace_dir)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .to_string();
                if rel.starts_with(".git") || rel.starts_with("target") {
                    continue;
                }
                self.all_items.push(PaletteItem {
                    title: format!("Open: {rel}"),
                    description: p.display().to_string(),
                    action: PaletteAction::OpenFile(p),
                });
            }
        }

        self.update_filter();
    }

    pub fn set_query(&mut self, q: String) {
        self.query = q;
        self.selected = 0;
        self.update_filter();
    }

    pub fn insert_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.update_filter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.update_filter();
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else if !self.filtered_indices.is_empty() {
            self.selected = self.filtered_indices.len() - 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered_indices.is_empty() && self.selected + 1 < self.filtered_indices.len() {
            self.selected += 1;
        } else {
            self.selected = 0;
        }
    }

    pub fn selected_item(&self) -> Option<&PaletteItem> {
        let (item_idx, _) = self.filtered_indices.get(self.selected)?;
        self.all_items.get(*item_idx)
    }

    fn update_filter(&mut self) {
        if self.query.is_empty() {
            self.filtered_indices = self
                .all_items
                .iter()
                .enumerate()
                .take(50)
                .map(|(i, _)| (i, 100))
                .collect();
            return;
        }

        let pattern = Pattern::new(
            &self.query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );

        let mut scored = Vec::new();
        for (i, item) in self.all_items.iter().enumerate() {
            let mut u32_buf = Vec::new();
            let haystack = Utf32Str::new(&item.title, &mut u32_buf);
            if let Some(score) = pattern.score(haystack, &mut self.matcher) {
                scored.push((i, score));
            }
        }

        scored.sort_by_key(|b| std::cmp::Reverse(b.1));
        scored.truncate(50);
        self.filtered_indices = scored;
    }
}

pub struct CompletionOverlay {
    pub items: Vec<EditorCompletionItem>,
    pub selected: usize,
    pub filter: String,
}

impl Default for CompletionOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl CompletionOverlay {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            filter: String::new(),
        }
    }

    pub fn set_items(&mut self, items: Vec<EditorCompletionItem>) {
        self.items = items;
        self.selected = 0;
        self.filter.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else if !self.items.is_empty() {
            self.selected = self.items.len() - 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.items.is_empty() && self.selected + 1 < self.items.len() {
            self.selected += 1;
        } else {
            self.selected = 0;
        }
    }

    pub fn selected_item(&self) -> Option<&EditorCompletionItem> {
        self.items.get(self.selected)
    }
}

pub struct FindBar {
    pub query: String,
    pub match_count: usize,
    pub current_match: usize,
    pub is_active: bool,
}

impl Default for FindBar {
    fn default() -> Self {
        Self::new()
    }
}

impl FindBar {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            match_count: 0,
            current_match: 0,
            is_active: false,
        }
    }

    pub fn open(&mut self) {
        self.is_active = true;
    }

    pub fn close(&mut self) {
        self.is_active = false;
    }
}

pub struct HoverOverlay {
    pub text: String,
    pub line: usize,
    pub cluster_idx: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_overlay_navigation() {
        let mut comp = CompletionOverlay::new();
        comp.set_items(vec![
            EditorCompletionItem {
                label: "println!".to_string(),
                detail: Some("macro".to_string()),
                insert_text: "println!(\"{}\", $0);".to_string(),
                kind: None,
            },
            EditorCompletionItem {
                label: "eprintln!".to_string(),
                detail: Some("macro".to_string()),
                insert_text: "eprintln!(\"{}\", $0);".to_string(),
                kind: None,
            },
        ]);

        assert_eq!(comp.selected, 0);
        assert_eq!(comp.selected_item().unwrap().label, "println!");

        comp.move_down();
        assert_eq!(comp.selected, 1);
        assert_eq!(comp.selected_item().unwrap().label, "eprintln!");

        // Wrap around
        comp.move_down();
        assert_eq!(comp.selected, 0);

        comp.move_up();
        assert_eq!(comp.selected, 1);
    }

    #[test]
    fn test_find_bar_toggle() {
        let mut fb = FindBar::new();
        assert!(!fb.is_active);

        fb.open();
        assert!(fb.is_active);

        fb.query.push_str("search term");
        assert_eq!(fb.query, "search term");

        fb.close();
        assert!(!fb.is_active);
    }

    #[test]
    fn test_command_palette_filtering() {
        let mut cp = CommandPalette::new();
        cp.all_items = vec![
            PaletteItem {
                title: "View: Toggle Pure Editor Mode".to_string(),
                description: "Editor".to_string(),
                action: PaletteAction::SetWorkspaceMode(crate::editor::WorkspaceMode::PureEditor),
            },
            PaletteItem {
                title: "View: Toggle Split Terminal Mode".to_string(),
                description: "Editor".to_string(),
                action: PaletteAction::SetWorkspaceMode(crate::editor::WorkspaceMode::Split),
            },
            PaletteItem {
                title: "Plugins: Reload WASM Plugins".to_string(),
                description: "Plugins".to_string(),
                action: PaletteAction::ReloadPlugins,
            },
        ];
        cp.update_filter();
        assert_eq!(cp.filtered_indices.len(), 3);

        cp.query = "WASM".to_string();
        cp.update_filter();
        assert_eq!(cp.filtered_indices.len(), 1);
        let idx = cp.filtered_indices[0].0;
        let matched = &cp.all_items[idx];
        assert!(matched.title.contains("WASM"));
    }
}
