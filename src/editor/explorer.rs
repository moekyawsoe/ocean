use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub is_expanded: bool,
}

#[derive(Debug, Clone)]
pub struct FileExplorer {
    pub root_dir: PathBuf,
    pub root_name: String,
    pub visible: bool,
    pub width: f32,
    pub entries: Vec<ExplorerEntry>,
    pub expanded_dirs: HashSet<PathBuf>,
    pub selected_path: Option<PathBuf>,
    pub hovered_idx: Option<usize>,
    pub scroll_y: f32,
    pub dragging_divider: bool,
}

impl Default for FileExplorer {
    fn default() -> Self {
        Self::new()
    }
}

impl FileExplorer {
    pub fn new() -> Self {
        Self {
            root_dir: PathBuf::new(),
            root_name: String::new(),
            visible: true,
            width: 210.0,
            entries: Vec::new(),
            expanded_dirs: HashSet::new(),
            selected_path: None,
            hovered_idx: None,
            scroll_y: 0.0,
            dragging_divider: false,
        }
    }

    pub fn is_open(&self) -> bool {
        !self.root_name.is_empty()
    }

    pub fn open_folder<P: AsRef<Path>>(&mut self, path: P) {
        let p = path.as_ref();
        let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let name = canonical
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                p.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string())
            });

        self.root_dir = canonical;
        self.root_name = if name.is_empty() {
            "/".to_string()
        } else {
            name
        };
        self.visible = true;
        self.expanded_dirs.clear();
        self.expanded_dirs.insert(self.root_dir.clone());
        self.scroll_y = 0.0;
        self.refresh();
    }

    pub fn refresh(&mut self) {
        if self.root_dir.as_os_str().is_empty() {
            self.entries.clear();
            return;
        }

        let mut out = Vec::new();
        Self::scan_dir(&self.root_dir, 0, &self.expanded_dirs, &mut out);
        self.entries = out;
    }

    fn scan_dir(
        dir: &Path,
        depth: usize,
        expanded: &HashSet<PathBuf>,
        out: &mut Vec<ExplorerEntry>,
    ) {
        if depth > 12 {
            return;
        }

        let rd = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };

        let mut items = Vec::new();
        for entry in rd.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();

            // Skip noisy directories and hidden items
            if fname == ".git"
                || fname == "target"
                || fname == "node_modules"
                || fname == ".cache"
                || fname == ".DS_Store"
            {
                continue;
            }

            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            items.push((fname, path, is_dir));
        }

        // Directories first (case-insensitive), then files (case-insensitive)
        items.sort_by(
            |(name_a, _, dir_a), (name_b, _, dir_b)| match dir_b.cmp(dir_a) {
                std::cmp::Ordering::Equal => name_a.to_lowercase().cmp(&name_b.to_lowercase()),
                other => other,
            },
        );

        for (name, path, is_dir) in items {
            let is_expanded = is_dir && expanded.contains(&path);
            out.push(ExplorerEntry {
                path: path.clone(),
                name,
                is_dir,
                depth,
                is_expanded,
            });

            if is_expanded {
                Self::scan_dir(&path, depth + 1, expanded, out);
            }
        }
    }

    pub fn toggle_expand(&mut self, idx: usize) -> Option<PathBuf> {
        if idx >= self.entries.len() {
            return None;
        }
        let entry = &self.entries[idx];
        if entry.is_dir {
            let p = entry.path.clone();
            if self.expanded_dirs.contains(&p) {
                self.expanded_dirs.remove(&p);
            } else {
                self.expanded_dirs.insert(p);
            }
            self.refresh();
            None
        } else {
            let p = entry.path.clone();
            self.selected_path = Some(p.clone());
            Some(p)
        }
    }

    pub fn scroll(&mut self, delta_lines: f32) {
        let max_scroll = (self.entries.len() as f32 * 22.0).max(0.0);
        self.scroll_y = (self.scroll_y - delta_lines * 22.0).clamp(0.0, max_scroll);
    }

    pub fn scroll_pixels(&mut self, dy_pixels: f32) {
        let max_scroll = (self.entries.len() as f32 * 22.0).max(0.0);
        self.scroll_y = (self.scroll_y - dy_pixels).clamp(0.0, max_scroll);
    }

    pub fn entry_at_point(
        &self,
        px: f32,
        py: f32,
        vp_x: f32,
        vp_y: f32,
        header_h: f32,
        item_h: f32,
    ) -> Option<usize> {
        if px < vp_x || px > vp_x + self.width {
            return None;
        }
        let content_y = vp_y + header_h;
        if py < content_y {
            return None;
        }
        let rel_y = py - content_y + self.scroll_y;
        if rel_y < 0.0 {
            return None;
        }
        let idx = (rel_y / item_h).floor() as usize;
        if idx < self.entries.len() {
            Some(idx)
        } else {
            None
        }
    }

    pub fn update_hover(
        &mut self,
        px: f32,
        py: f32,
        vp_x: f32,
        vp_y: f32,
        header_h: f32,
        item_h: f32,
    ) {
        self.hovered_idx = self.entry_at_point(px, py, vp_x, vp_y, header_h, item_h);
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_explorer_scan_and_expand() {
        let temp_dir = std::env::temp_dir().join(format!("ocean_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("subfolder")).unwrap();
        fs::write(temp_dir.join("a_file.txt"), "hello").unwrap();
        fs::write(temp_dir.join("subfolder").join("child.txt"), "child").unwrap();

        let mut explorer = FileExplorer::new();
        explorer.open_folder(&temp_dir);

        assert!(explorer.is_open());
        assert!(!explorer.entries.is_empty());
        // Directory "subfolder" should be sorted before "a_file.txt"
        assert_eq!(explorer.entries[0].name, "subfolder");
        assert!(explorer.entries[0].is_dir);
        assert!(!explorer.entries[0].is_expanded);

        // Expand subfolder
        let file = explorer.toggle_expand(0);
        assert!(file.is_none());
        assert!(explorer.entries[0].is_expanded);
        // Now child.txt should be visible at depth 1
        assert!(explorer.entries.len() >= 3);
        let child = explorer
            .entries
            .iter()
            .find(|e| e.name == "child.txt")
            .unwrap();
        assert_eq!(child.depth, 1);

        // Click file
        let file_idx = explorer
            .entries
            .iter()
            .position(|e| e.name == "a_file.txt")
            .unwrap();
        let clicked = explorer.toggle_expand(file_idx);
        assert!(clicked.is_some());
        assert_eq!(clicked.unwrap().file_name().unwrap(), "a_file.txt");

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
