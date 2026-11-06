use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use winit::event_loop::EventLoopProxy;

use crate::editor::document::EditorDocument;
use crate::editor::{EditorWorkspace, WorkspaceMode};
use crate::terminal::AppEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub clean_exit: bool,
    pub saved_at_unix: u64,
    pub editor: EditorState,
    pub documents: Vec<DocumentState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorState {
    pub mode: String,
    pub workspace_dir: Option<PathBuf>,
    pub active_doc: usize,
    pub split_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentState {
    pub path: Option<PathBuf>,
    pub backup: Option<String>,
    pub dirty: bool,
    pub cursor_line: usize,
    pub cursor_cluster: usize,
    pub scroll_y: usize,
    pub scroll_x: f32,
}

pub fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(dir).join("ocean")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("ocean")
    } else {
        std::env::temp_dir().join("ocean-state")
    }
}

pub fn session_path() -> PathBuf {
    state_dir().join("session.json")
}

pub fn backups_dir() -> PathBuf {
    state_dir().join("backups")
}

pub fn load() -> Option<Session> {
    let text = fs::read_to_string(session_path()).ok()?;
    let session: Session = serde_json::from_str(&text).ok()?;
    if session.version != 1 {
        return None;
    }
    Some(session)
}

fn mode_name(mode: WorkspaceMode) -> &'static str {
    match mode {
        WorkspaceMode::PureTerminal => "pure_terminal",
        WorkspaceMode::Split => "split",
        WorkspaceMode::PureEditor => "pure_editor",
    }
}

fn parse_mode(mode: &str) -> WorkspaceMode {
    match mode {
        "pure_editor" => WorkspaceMode::PureEditor,
        "split" => WorkspaceMode::Split,
        _ => WorkspaceMode::PureTerminal,
    }
}

pub fn save(editor: &EditorWorkspace, clean_exit: bool) -> Result<(), String> {
    let dir = state_dir();
    fs::create_dir_all(backups_dir()).map_err(|err| err.to_string())?;

    let mut documents = Vec::with_capacity(editor.documents.len());
    for (index, document) in editor.documents.iter().enumerate() {
        let mut backup = None;
        if !clean_exit && document.is_dirty() {
            let backup_name = format!("doc-{index}.bak");
            let backup_path = backups_dir().join(&backup_name);
            write_file_atomic(&backup_path, document.buffer.rope.to_string().as_bytes())?;
            backup = Some(backup_name);
        }
        documents.push(DocumentState {
            path: document.path().cloned(),
            backup,
            dirty: !clean_exit && document.is_dirty(),
            cursor_line: document.cursor.line,
            cursor_cluster: document.cursor.cluster_idx,
            scroll_y: document.scroll_y,
            scroll_x: document.scroll_x,
        });
    }

    let session = Session {
        version: 1,
        clean_exit,
        saved_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
        editor: EditorState {
            mode: mode_name(editor.mode).to_string(),
            workspace_dir: if editor.workspace_dir.as_os_str().is_empty() {
                None
            } else {
                Some(editor.workspace_dir.clone())
            },
            active_doc: editor.active_doc_idx,
            split_ratio: editor.split_ratio,
        },
        documents,
    };

    let json = serde_json::to_string_pretty(&session).map_err(|err| err.to_string())?;
    write_file_atomic(&dir.join("session.json"), json.as_bytes())?;
    if clean_exit {
        clear_backups();
    }
    Ok(())
}

pub fn clear_backups() {
    let dir = backups_dir();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn restore_editor(
    editor: &mut EditorWorkspace,
    session: &Session,
    proxy: EventLoopProxy<AppEvent>,
    crash_recovery: bool,
) {
    if let Some(workspace) = &session.editor.workspace_dir
        && workspace.is_dir()
    {
        editor.open_folder(workspace);
    }

    for document in &session.documents {
        let backup_text = if crash_recovery && !session.clean_exit {
            document
                .backup
                .as_ref()
                .and_then(|name| fs::read_to_string(backups_dir().join(name)).ok())
        } else {
            None
        };

        let index = if let Some(text) = backup_text {
            let restored = EditorDocument::from_text(&text, document.path.clone(), true);
            editor.documents.push(restored);
            Some(editor.documents.len() - 1)
        } else if let Some(path) = &document.path {
            if path.exists() {
                let _ = editor.open_file(path, proxy.clone());
                editor
                    .documents
                    .iter()
                    .position(|doc| doc.path() == Some(path))
            } else {
                None
            }
        } else {
            None
        };

        if let Some(index) = index
            && let Some(doc) = editor.documents.get_mut(index)
        {
            doc.cursor =
                crate::editor::document::Cursor::new(document.cursor_line, document.cursor_cluster);
            doc.scroll_y = document.scroll_y;
            doc.scroll_x = document.scroll_x;
            doc.scroll_to_cursor_requested = true;
        }
    }

    if editor.documents.is_empty() {
        editor.documents.push(EditorDocument::new());
    }
    editor.active_doc_idx = session
        .editor
        .active_doc
        .min(editor.documents.len().saturating_sub(1));
    editor.mode = parse_mode(&session.editor.mode);
    editor.split_ratio = session.editor.split_ratio.clamp(0.15, 0.85);
    editor.editor_focused = editor.is_active();
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_round_trip() {
        assert_eq!(
            parse_mode(mode_name(WorkspaceMode::Split)),
            WorkspaceMode::Split
        );
        assert_eq!(
            parse_mode(mode_name(WorkspaceMode::PureEditor)),
            WorkspaceMode::PureEditor
        );
        assert_eq!(
            parse_mode(mode_name(WorkspaceMode::PureTerminal)),
            WorkspaceMode::PureTerminal
        );
    }
}
