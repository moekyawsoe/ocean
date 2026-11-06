use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;

use serde::{Deserialize, Serialize};
use winit::event_loop::EventLoopProxy;

use crate::terminal::AppEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEditorRequest {
    pub pane_id: u64,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcSplitRequest {
    pub pane_id: u64,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("ocean-{}.sock", std::process::id()))
}

pub fn start_listener(proxy: EventLoopProxy<AppEvent>) -> std::io::Result<PathBuf> {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_ok() {
                let trimmed = line.trim();
                if let Ok(split_req) = serde_json::from_str::<IpcSplitRequest>(trimmed) {
                    if !split_req.command.is_empty() {
                        let _ = proxy.send_event(AppEvent::IpcSplitRequest(split_req));
                        continue;
                    }
                }
                if let Ok(request) = serde_json::from_str::<IpcEditorRequest>(trimmed) {
                    let _ = proxy.send_event(AppEvent::IpcEditorRequest(request));
                }
            }
        }
    });

    Ok(path)
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn newest_running_socket() -> Option<PathBuf> {
    let directory = runtime_dir();
    let entries = std::fs::read_dir(directory).ok()?;
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("ocean-") || !name.ends_with(".sock") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, path)| path)
}

/// Ask the Ocean instance that owns the current shell pane to switch into editor mode.
/// Falls back to the most recently active Ocean window when invoked from outside one.
pub fn try_send_edit_request(path: Option<&Path>) -> bool {
    let socket_from_env = std::env::var_os("OCEAN_SOCKET").map(PathBuf::from);
    let pane_id = std::env::var("OCEAN_PANE_ID")
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or(0);

    let Some(socket) = socket_from_env.or_else(newest_running_socket) else {
        return false;
    };

    let cwd = std::env::current_dir().ok();
    let target = path.map(|path| {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.as_ref()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|| path.to_path_buf())
        };
        absolute.canonicalize().unwrap_or(absolute)
    });
    let request = IpcEditorRequest {
        pane_id,
        path: target.map(|path| path.to_string_lossy().into_owned()),
        cwd: cwd.map(|path| path.to_string_lossy().into_owned()),
    };

    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    if serde_json::to_writer(&mut stream, &request).is_ok() {
        let _ = stream.write_all(b"\n");
        let _ = stream.flush();
        true
    } else {
        false
    }
}

/// Ask the Ocean instance that owns the current shell pane to split and launch a command.
/// Falls back to the newest Ocean window if invoked without pane context.
pub fn try_send_split_request(
    command: &str,
    args: &[String],
    direction: Option<&str>,
    cwd: Option<&Path>,
) -> bool {
    let socket_from_env = std::env::var_os("OCEAN_SOCKET").map(PathBuf::from);
    let pane_id = std::env::var("OCEAN_PANE_ID")
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or(0);

    let Some(socket) = socket_from_env.or_else(newest_running_socket) else {
        return false;
    };

    let cwd_path = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());

    let request = IpcSplitRequest {
        pane_id,
        command: command.to_string(),
        args: args.to_vec(),
        direction: direction.map(|s| s.to_string()),
        cwd: cwd_path.map(|p| p.to_string_lossy().into_owned()),
    };

    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    if serde_json::to_writer(&mut stream, &request).is_ok() {
        let _ = stream.write_all(b"\n");
        let _ = stream.flush();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_through_json() {
        let request = IpcEditorRequest {
            pane_id: 42,
            path: Some("/tmp/hello world.rs".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        let encoded = serde_json::to_string(&request).expect("encode");
        let decoded: IpcEditorRequest = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded.pane_id, 42);
        assert_eq!(decoded.path.as_deref(), Some("/tmp/hello world.rs"));
    }

    #[test]
    fn split_request_round_trips_through_json() {
        let request = IpcSplitRequest {
            pane_id: 17,
            command: "ocean".to_string(),
            args: vec!["file".to_string(), "sample.pdf".to_string()],
            direction: Some("right".to_string()),
            cwd: Some("/home".to_string()),
        };
        let encoded = serde_json::to_string(&request).expect("encode");
        let decoded: IpcSplitRequest = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded.pane_id, 17);
        assert_eq!(decoded.command, "ocean");
        assert_eq!(decoded.args.len(), 2);
        assert_eq!(decoded.direction.as_deref(), Some("right"));
    }
}
