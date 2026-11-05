use std::collections::HashMap;
use std::env;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::Duration;

use lsp_types::{DiagnosticSeverity, Uri};
use serde_json::{Value, json};
use winit::event_loop::EventLoopProxy;

use crate::terminal::AppEvent;

#[derive(Debug, Clone)]
pub struct EditorDiagnostic {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct EditorCompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub kind: Option<lsp_types::CompletionItemKind>,
}

#[derive(Debug, Clone)]
pub enum LspEvent {
    Diagnostics(PathBuf),
    Completions(PathBuf),
    Hover(PathBuf),
    StatusChanged,
}

#[derive(Debug, Clone, Copy)]
struct LspLaunch {
    program: &'static str,
    args: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct LspFileType {
    server_key: &'static str,
    language_id: &'static str,
    label: &'static str,
    candidates: &'static [LspLaunch],
}

const RUST: &[LspLaunch] = &[LspLaunch {
    program: "rust-analyzer",
    args: &[],
}];
const GO: &[LspLaunch] = &[LspLaunch {
    program: "gopls",
    args: &[],
}];
const TS: &[LspLaunch] = &[
    LspLaunch {
        program: "vtsls",
        args: &[],
    },
    LspLaunch {
        program: "typescript-language-server",
        args: &["--stdio"],
    },
];
const PYTHON: &[LspLaunch] = &[
    LspLaunch {
        program: "pylsp",
        args: &[],
    },
    LspLaunch {
        program: "pyright-langserver",
        args: &["--stdio"],
    },
];
const CLANGD: &[LspLaunch] = &[LspLaunch {
    program: "clangd",
    args: &[],
}];
const HTML: &[LspLaunch] = &[LspLaunch {
    program: "vscode-html-language-server",
    args: &["--stdio"],
}];
const CSS: &[LspLaunch] = &[LspLaunch {
    program: "vscode-css-language-server",
    args: &["--stdio"],
}];
const JSON: &[LspLaunch] = &[LspLaunch {
    program: "vscode-json-language-server",
    args: &["--stdio"],
}];
const YAML: &[LspLaunch] = &[LspLaunch {
    program: "yaml-language-server",
    args: &["--stdio"],
}];
const TOML: &[LspLaunch] = &[LspLaunch {
    program: "taplo",
    args: &["lsp", "stdio"],
}];
const SHELL: &[LspLaunch] = &[LspLaunch {
    program: "bash-language-server",
    args: &["start"],
}];
const MARKDOWN: &[LspLaunch] = &[LspLaunch {
    program: "marksman",
    args: &["server"],
}];
const DOCKER: &[LspLaunch] = &[LspLaunch {
    program: "docker-langserver",
    args: &["--stdio"],
}];

fn file_type(
    server_key: &'static str,
    language_id: &'static str,
    label: &'static str,
    candidates: &'static [LspLaunch],
) -> LspFileType {
    LspFileType {
        server_key,
        language_id,
        label,
        candidates,
    }
}

fn detect_file_type(path: &Path) -> Option<LspFileType> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name == "dockerfile" {
        return Some(file_type(
            "docker",
            "dockerfile",
            "docker-langserver",
            DOCKER,
        ));
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    Some(match ext.as_str() {
        "rs" => file_type("rust", "rust", "rust-analyzer", RUST),
        "go" => file_type("go", "go", "gopls", GO),
        "ts" => file_type("typescript", "typescript", "vtsls", TS),
        "tsx" => file_type("typescript", "typescriptreact", "vtsls", TS),
        "js" | "mjs" | "cjs" => file_type("typescript", "javascript", "vtsls", TS),
        "jsx" => file_type("typescript", "javascriptreact", "vtsls", TS),
        "py" => file_type("python", "python", "pylsp", PYTHON),
        "c" | "h" => file_type("c", "c", "clangd", CLANGD),
        "cpp" | "cc" | "cxx" | "hpp" => file_type("c", "cpp", "clangd", CLANGD),
        "html" | "htm" => file_type("html", "html", "vscode-html-language-server", HTML),
        "css" => file_type("css", "css", "vscode-css-language-server", CSS),
        "scss" => file_type("css", "scss", "vscode-css-language-server", CSS),
        "json" => file_type("json", "json", "vscode-json-language-server", JSON),
        "jsonc" => file_type("json", "jsonc", "vscode-json-language-server", JSON),
        "yaml" | "yml" => file_type("yaml", "yaml", "yaml-language-server", YAML),
        "toml" => file_type("toml", "toml", "taplo", TOML),
        "sh" | "bash" | "zsh" => file_type("shell", "shellscript", "bash-language-server", SHELL),
        "md" | "markdown" => file_type("markdown", "markdown", "marksman", MARKDOWN),
        _ => return None,
    })
}

struct ServerInstance {
    cmd_tx: Sender<WorkerCmd>,
    resp_rx: Receiver<WorkerMsg>,
    program: &'static str,
}

enum WorkerCmd {
    DidOpen {
        path: PathBuf,
        language_id: &'static str,
        text: String,
        version: i64,
    },
    DidChange {
        path: PathBuf,
        text: String,
        version: i64,
    },
    DidSave {
        path: PathBuf,
    },
    DidClose {
        path: PathBuf,
    },
    Completion {
        path: PathBuf,
        line: u32,
        character: u32,
    },
    Hover {
        path: PathBuf,
        line: u32,
        character: u32,
    },
    Inbound(Value),
    Shutdown,
}

enum PendingKind {
    Completion(PathBuf),
    Hover(PathBuf),
}

enum WorkerMsg {
    Diagnostics {
        path: PathBuf,
        diagnostics: Vec<EditorDiagnostic>,
    },
    Completions {
        path: PathBuf,
        items: Vec<EditorCompletionItem>,
    },
    Hover {
        path: PathBuf,
        content: Option<String>,
    },
    Ready {
        name: String,
    },
    Failed {
        message: String,
    },
}

pub struct LspClient {
    instances: HashMap<&'static str, ServerInstance>,
    diagnostics: HashMap<PathBuf, Vec<EditorDiagnostic>>,
    completions: HashMap<PathBuf, Vec<EditorCompletionItem>>,
    hovers: HashMap<PathBuf, String>,
    statuses: HashMap<&'static str, String>,
}

impl Default for LspClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LspClient {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
            diagnostics: HashMap::new(),
            completions: HashMap::new(),
            hovers: HashMap::new(),
            statuses: HashMap::new(),
        }
    }

    /// Start the language server required by `path`, if it is not already running.
    /// Returns `true` when a server is available for this file.
    pub fn start_for_file(
        &mut self,
        path: &Path,
        workspace_dir: &Path,
        proxy: EventLoopProxy<AppEvent>,
    ) -> bool {
        let Some(ft) = detect_file_type(path) else {
            return false;
        };

        if self.instances.contains_key(ft.server_key) {
            return true;
        }

        let Some(launch) = ft
            .candidates
            .iter()
            .find(|l| find_in_path(l.program).is_some())
        else {
            self.statuses
                .insert(ft.server_key, format!("LSP: {} not found", ft.label));
            return false;
        };

        let root = find_project_root(path, workspace_dir, ft.server_key);
        let (cmd_tx, cmd_rx) = channel::<WorkerCmd>();
        let inbound_tx = cmd_tx.clone();
        let (resp_tx, resp_rx) = channel::<WorkerMsg>();

        thread::spawn(move || {
            run_lsp_worker(
                launch.program,
                launch.args,
                root,
                cmd_rx,
                inbound_tx,
                resp_tx,
                proxy,
            );
        });

        self.statuses
            .insert(ft.server_key, format!("LSP: {}", launch.program));
        self.instances.insert(
            ft.server_key,
            ServerInstance {
                cmd_tx,
                resp_rx,
                program: launch.program,
            },
        );
        true
    }

    /// Drain worker messages. Returns events the editor may need to react to
    /// (for example, opening a completion popup when data actually arrives).
    pub fn poll_events(&mut self) -> Vec<LspEvent> {
        let mut events = Vec::new();
        let mut dead_servers = Vec::new();

        for (key, instance) in self.instances.iter() {
            loop {
                match instance.resp_rx.try_recv() {
                    Ok(msg) => match msg {
                        WorkerMsg::Diagnostics { path, diagnostics } => {
                            self.diagnostics.insert(path.clone(), diagnostics);
                            events.push(LspEvent::Diagnostics(path));
                        }
                        WorkerMsg::Completions { path, items } => {
                            self.completions.insert(path.clone(), items);
                            events.push(LspEvent::Completions(path));
                        }
                        WorkerMsg::Hover { path, content } => {
                            match content {
                                Some(text) => {
                                    self.hovers.insert(path.clone(), text);
                                }
                                None => {
                                    self.hovers.remove(&path);
                                }
                            }
                            events.push(LspEvent::Hover(path));
                        }
                        WorkerMsg::Ready { name } => {
                            self.statuses.insert(key, format!("LSP: {name}"));
                            events.push(LspEvent::StatusChanged);
                        }
                        WorkerMsg::Failed { message } => {
                            self.statuses.insert(key, format!("LSP: {message}"));
                            dead_servers.push(*key);
                            events.push(LspEvent::StatusChanged);
                        }
                    },
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if !dead_servers.contains(key) {
                            dead_servers.push(*key);
                        }
                        break;
                    }
                }
            }
        }

        for key in dead_servers {
            if let Some(instance) = self.instances.remove(&key) {
                self.statuses
                    .insert(key, format!("LSP: {} stopped", instance.program));
            }
        }

        events
    }

    pub fn did_open(&self, path: &Path, text: &str, version: i64) {
        let Some(ft) = detect_file_type(path) else {
            return;
        };
        if let Some(instance) = self.instances.get(ft.server_key) {
            let _ = instance.cmd_tx.send(WorkerCmd::DidOpen {
                path: path.to_path_buf(),
                language_id: ft.language_id,
                text: text.to_string(),
                version,
            });
        }
    }

    pub fn did_change(&self, path: &Path, text: &str, version: i64) {
        if let Some(instance) = self.instance_for(path) {
            let _ = instance.cmd_tx.send(WorkerCmd::DidChange {
                path: path.to_path_buf(),
                text: text.to_string(),
                version,
            });
        }
    }

    pub fn did_save(&self, path: &Path) {
        if let Some(instance) = self.instance_for(path) {
            let _ = instance.cmd_tx.send(WorkerCmd::DidSave {
                path: path.to_path_buf(),
            });
        }
    }

    pub fn did_close(&self, path: &Path) {
        if let Some(instance) = self.instance_for(path) {
            let _ = instance.cmd_tx.send(WorkerCmd::DidClose {
                path: path.to_path_buf(),
            });
        }
    }

    pub fn request_completion(&self, path: &Path, line: u32, character: u32) -> bool {
        let Some(instance) = self.instance_for(path) else {
            return false;
        };
        let _ = instance.cmd_tx.send(WorkerCmd::Completion {
            path: path.to_path_buf(),
            line,
            character,
        });
        true
    }

    pub fn request_hover(&self, path: &Path, line: u32, character: u32) -> bool {
        let Some(instance) = self.instance_for(path) else {
            return false;
        };
        let _ = instance.cmd_tx.send(WorkerCmd::Hover {
            path: path.to_path_buf(),
            line,
            character,
        });
        true
    }

    pub fn diagnostics_for(&self, path: &Path) -> &[EditorDiagnostic] {
        self.diagnostics.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn latest_completions_for(&self, path: &Path) -> Vec<EditorCompletionItem> {
        self.completions.get(path).cloned().unwrap_or_default()
    }

    pub fn latest_hover_for(&self, path: &Path) -> Option<String> {
        self.hovers.get(path).cloned()
    }

    pub fn has_server_for(&self, path: &Path) -> bool {
        detect_file_type(path)
            .map(|ft| self.instances.contains_key(ft.server_key))
            .unwrap_or(false)
    }

    pub fn status_for(&self, path: &Path) -> Option<String> {
        let ft = detect_file_type(path)?;
        self.statuses.get(ft.server_key).cloned()
    }

    pub fn restart_for(
        &mut self,
        path: &Path,
        workspace_dir: &Path,
        proxy: EventLoopProxy<AppEvent>,
    ) -> bool {
        if let Some(ft) = detect_file_type(path) {
            if let Some(instance) = self.instances.remove(ft.server_key) {
                let _ = instance.cmd_tx.send(WorkerCmd::Shutdown);
            }
            self.statuses.remove(ft.server_key);
        }
        self.start_for_file(path, workspace_dir, proxy)
    }

    pub fn clear_file_state(&mut self, path: &Path) {
        self.diagnostics.remove(path);
        self.completions.remove(path);
        self.hovers.remove(path);
    }

    fn instance_for(&self, path: &Path) -> Option<&ServerInstance> {
        let ft = detect_file_type(path)?;
        self.instances.get(ft.server_key)
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        for instance in self.instances.values() {
            let _ = instance.cmd_tx.send(WorkerCmd::Shutdown);
        }
    }
}

fn run_lsp_worker(
    program: &'static str,
    args: &'static [&'static str],
    root: PathBuf,
    cmd_rx: Receiver<WorkerCmd>,
    inbound_tx: Sender<WorkerCmd>,
    resp_tx: Sender<WorkerMsg>,
    proxy: EventLoopProxy<AppEvent>,
) {
    let mut child = match Command::new(program)
        .args(args)
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            let _ = resp_tx.send(WorkerMsg::Failed {
                message: format!("{program} not found"),
            });
            let _ = proxy.send_event(AppEvent::Wakeup);
            return;
        }
    };

    let Some(stdin) = child.stdin.take() else {
        let _ = resp_tx.send(WorkerMsg::Failed {
            message: "stdin unavailable".to_string(),
        });
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = resp_tx.send(WorkerMsg::Failed {
            message: "stdout unavailable".to_string(),
        });
        return;
    };

    let (out_tx, out_rx) = channel::<Value>();
    thread::spawn(move || {
        let mut writer = BufWriter::new(stdin);
        while let Ok(msg) = out_rx.recv() {
            if write_message(&mut writer, &msg).is_err() {
                return;
            }
        }
    });

    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_message(&mut reader) {
                Ok(Some(value)) => {
                    if inbound_tx.send(WorkerCmd::Inbound(value)).is_err() {
                        return;
                    }
                }
                _ => {
                    let _ = inbound_tx.send(WorkerCmd::Inbound(json!({
                        "__ocean_eof": true
                    })));
                    return;
                }
            }
        }
    });

    let mut next_id: i64 = 1;
    let init_id = next_id;
    next_id += 1;
    let mut pending: HashMap<i64, PendingKind> = HashMap::new();
    let mut initialized = false;
    let mut buffered: Vec<WorkerCmd> = Vec::new();

    let _ = out_tx.send(initialize_message(&root, init_id));

    loop {
        let received = if initialized {
            cmd_rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            cmd_rx.recv_timeout(Duration::from_secs(20))
        };

        let cmd = match received {
            Ok(cmd) => cmd,
            Err(RecvTimeoutError::Timeout) => {
                let _ = resp_tx.send(WorkerMsg::Failed {
                    message: format!("{program} did not initialize"),
                });
                let _ = proxy.send_event(AppEvent::Wakeup);
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };

        match cmd {
            WorkerCmd::Inbound(value) => {
                if value.get("__ocean_eof").and_then(Value::as_bool) == Some(true) {
                    let _ = resp_tx.send(WorkerMsg::Failed {
                        message: format!("{program} exited"),
                    });
                    let _ = proxy.send_event(AppEvent::Wakeup);
                    break;
                }

                let id = value.get("id").cloned();
                let method = value.get("method").and_then(Value::as_str);

                match (id, method) {
                    (Some(id), Some(method)) => {
                        let _ = out_tx.send(server_request_response(&id, method, &value));
                    }
                    (Some(id), None) => {
                        let Some(id_num) = id.as_i64() else {
                            continue;
                        };
                        if id_num == init_id && !initialized {
                            if value.get("error").is_some() {
                                let _ = resp_tx.send(WorkerMsg::Failed {
                                    message: format!("{program} initialization failed"),
                                });
                                let _ = proxy.send_event(AppEvent::Wakeup);
                                break;
                            }
                            initialized = true;
                            let _ = out_tx.send(json!({
                                "jsonrpc": "2.0",
                                "method": "initialized",
                                "params": {}
                            }));
                            let _ = resp_tx.send(WorkerMsg::Ready {
                                name: program.to_string(),
                            });
                            let _ = proxy.send_event(AppEvent::Wakeup);
                            for queued in buffered.drain(..) {
                                dispatch_command(queued, &out_tx, &mut next_id, &mut pending);
                            }
                            continue;
                        }

                        match pending.remove(&id_num) {
                            Some(PendingKind::Completion(path)) => {
                                let items = value
                                    .get("result")
                                    .map(parse_completion)
                                    .unwrap_or_default();
                                let _ = resp_tx.send(WorkerMsg::Completions { path, items });
                                let _ = proxy.send_event(AppEvent::Wakeup);
                            }
                            Some(PendingKind::Hover(path)) => {
                                let content = value.get("result").and_then(parse_hover);
                                let _ = resp_tx.send(WorkerMsg::Hover { path, content });
                                let _ = proxy.send_event(AppEvent::Wakeup);
                            }
                            None => {}
                        }
                    }
                    (None, Some("textDocument/publishDiagnostics")) => {
                        if let Some((path, diagnostics)) = parse_diagnostics(&value) {
                            let _ = resp_tx.send(WorkerMsg::Diagnostics { path, diagnostics });
                            let _ = proxy.send_event(AppEvent::Wakeup);
                        }
                    }
                    _ => {}
                }
            }
            WorkerCmd::Shutdown => {
                let id = next_id;
                let _ = out_tx.send(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "shutdown",
                    "params": null
                }));
                let _ = out_tx.send(json!({
                    "jsonrpc": "2.0",
                    "method": "exit",
                    "params": null
                }));
                break;
            }
            other => {
                if initialized {
                    dispatch_command(other, &out_tx, &mut next_id, &mut pending);
                } else {
                    buffered.push(other);
                }
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}

fn dispatch_command(
    cmd: WorkerCmd,
    out_tx: &Sender<Value>,
    next_id: &mut i64,
    pending: &mut HashMap<i64, PendingKind>,
) {
    match cmd {
        WorkerCmd::DidOpen {
            path,
            language_id,
            text,
            version,
        } => {
            if let Some(uri) = path_to_uri(&path) {
                let _ = out_tx.send(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didOpen",
                    "params": {
                        "textDocument": {
                            "uri": uri.as_str(),
                            "languageId": language_id,
                            "version": version,
                            "text": text,
                        }
                    }
                }));
            }
        }
        WorkerCmd::DidChange {
            path,
            text,
            version,
        } => {
            if let Some(uri) = path_to_uri(&path) {
                let _ = out_tx.send(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didChange",
                    "params": {
                        "textDocument": { "uri": uri.as_str(), "version": version },
                        "contentChanges": [{ "text": text }]
                    }
                }));
            }
        }
        WorkerCmd::DidSave { path } => {
            if let Some(uri) = path_to_uri(&path) {
                let _ = out_tx.send(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didSave",
                    "params": { "textDocument": { "uri": uri.as_str() } }
                }));
            }
        }
        WorkerCmd::DidClose { path } => {
            if let Some(uri) = path_to_uri(&path) {
                let _ = out_tx.send(json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didClose",
                    "params": { "textDocument": { "uri": uri.as_str() } }
                }));
            }
        }
        WorkerCmd::Completion {
            path,
            line,
            character,
        } => {
            if let Some(uri) = path_to_uri(&path) {
                let id = *next_id;
                *next_id += 1;
                pending.insert(id, PendingKind::Completion(path));
                let _ = out_tx.send(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "textDocument/completion",
                    "params": {
                        "textDocument": { "uri": uri.as_str() },
                        "position": { "line": line, "character": character },
                        "context": { "triggerKind": 1 }
                    }
                }));
            }
        }
        WorkerCmd::Hover {
            path,
            line,
            character,
        } => {
            if let Some(uri) = path_to_uri(&path) {
                let id = *next_id;
                *next_id += 1;
                pending.insert(id, PendingKind::Hover(path));
                let _ = out_tx.send(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "textDocument/hover",
                    "params": {
                        "textDocument": { "uri": uri.as_str() },
                        "position": { "line": line, "character": character }
                    }
                }));
            }
        }
        WorkerCmd::Inbound(_) | WorkerCmd::Shutdown => {}
    }
}

fn initialize_message(root: &Path, id: i64) -> Value {
    let root_uri = path_to_uri(root);
    let root_uri_str = root_uri.as_ref().map(|u| u.as_str().to_string());
    let folders = root_uri_str.as_ref().map(|uri| {
        json!([{
            "uri": uri,
            "name": root.file_name().and_then(|n| n.to_str()).unwrap_or("workspace")
        }])
    });

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootPath": root.to_string_lossy(),
            "rootUri": root_uri_str,
            "workspaceFolders": folders,
            "clientInfo": { "name": "Ocean Editor" },
            "capabilities": {
                "workspace": {
                    "workspaceFolders": true,
                    "configuration": true,
                    "didChangeConfiguration": { "dynamicRegistration": true },
                    "didChangeWatchedFiles": { "dynamicRegistration": true }
                },
                "textDocument": {
                    "synchronization": {
                        "dynamicRegistration": true,
                        "didSave": true,
                        "willSave": false,
                        "willSaveWaitUntil": false
                    },
                    "completion": {
                        "dynamicRegistration": true,
                        "contextSupport": true,
                        "completionItem": {
                            "snippetSupport": true,
                            "commitCharactersSupport": true,
                            "documentationFormat": ["markdown", "plaintext"],
                            "insertReplaceSupport": true,
                            "resolveSupport": {
                                "properties": ["documentation", "detail", "additionalTextEdits"]
                            }
                        }
                    },
                    "hover": {
                        "dynamicRegistration": true,
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "publishDiagnostics": {
                        "relatedInformation": true,
                        "versionSupport": true
                    },
                    "definition": { "dynamicRegistration": true },
                    "references": { "dynamicRegistration": true },
                    "documentSymbol": { "dynamicRegistration": true }
                },
                "window": {
                    "workDoneProgress": true,
                    "showMessage": {}
                }
            }
        }
    })
}

fn server_request_response(id: &Value, method: &str, message: &Value) -> Value {
    let result = match method {
        "workspace/configuration" => {
            let count = message
                .get("params")
                .and_then(|p| p.get("items"))
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0);
            Value::Array(vec![Value::Null; count])
        }
        "client/registerCapability" | "client/unregisterCapability" => json!({}),
        "workspace/workspaceFolders" => json!([]),
        "window/workDoneProgress/create" | "window/showMessageRequest" => Value::Null,
        _ => Value::Null,
    };

    json!({
        "jsonrpc": "2.0",
        "id": id.clone(),
        "result": result
    })
}

fn parse_diagnostics(value: &Value) -> Option<(PathBuf, Vec<EditorDiagnostic>)> {
    let params = value.get("params")?;
    let uri_str = params.get("uri")?.as_str()?;
    let uri = Uri::from_str(uri_str).ok()?;
    let path = uri_to_path(&uri);

    let diagnostics = params
        .get("diagnostics")?
        .as_array()?
        .iter()
        .filter_map(|d| {
            let range = d.get("range")?;
            let start = range.get("start")?;
            let end = range.get("end")?;
            let severity = match d.get("severity").and_then(Value::as_u64) {
                Some(2) => DiagnosticSeverity::WARNING,
                Some(3) => DiagnosticSeverity::INFORMATION,
                Some(4) => DiagnosticSeverity::HINT,
                _ => DiagnosticSeverity::ERROR,
            };
            Some(EditorDiagnostic {
                start_line: start.get("line")?.as_u64()? as usize,
                start_col: start.get("character")?.as_u64()? as usize,
                end_line: end.get("line")?.as_u64()? as usize,
                end_col: end.get("character")?.as_u64()? as usize,
                severity,
                message: d.get("message")?.as_str()?.to_string(),
            })
        })
        .collect();

    Some((path, diagnostics))
}

fn parse_completion(result: &Value) -> Vec<EditorCompletionItem> {
    let items = result
        .as_array()
        .cloned()
        .or_else(|| result.get("items").and_then(Value::as_array).cloned())
        .unwrap_or_default();

    items
        .into_iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            let detail = item
                .get("detail")
                .and_then(Value::as_str)
                .map(str::to_string);
            let insert_text = item
                .get("insertText")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    item.get("textEdit")
                        .and_then(|te| te.get("newText"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| label.clone());
            Some(EditorCompletionItem {
                label,
                detail,
                insert_text,
                kind: None,
            })
        })
        .collect()
}

fn parse_hover(result: &Value) -> Option<String> {
    let contents = result.get("contents")?;
    let parts: Vec<String> = match contents {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Object(obj) => obj.get("value").and_then(Value::as_str).map(str::to_string),
                _ => None,
            })
            .collect(),
        Value::Object(obj) => obj
            .get("value")
            .and_then(Value::as_str)
            .map(|text| vec![text.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn write_message(writer: &mut impl Write, msg: &Value) -> std::io::Result<()> {
    let serialized = serde_json::to_string(msg).unwrap_or_else(|_| "{}".to_string());
    write!(
        writer,
        "Content-Length: {}\r\n\r\n{}",
        serialized.len(),
        serialized
    )?;
    writer.flush()
}

fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut content_len: Option<usize> = None;
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:")
            && let Ok(len) = value.trim().parse::<usize>()
        {
            content_len = Some(len);
        }
    }

    let Some(len) = content_len else {
        return Ok(None);
    };
    let mut body = vec![0u8; len];
    std::io::Read::read_exact(reader, &mut body)?;
    Ok(serde_json::from_slice(&body).ok())
}

fn path_to_uri(path: &Path) -> Option<Uri> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().ok()?.join(path)
    };
    let url = url::Url::from_file_path(&absolute).ok()?;
    Uri::from_str(url.as_str()).ok()
}

fn uri_to_path(uri: &Uri) -> PathBuf {
    if let Ok(url) = url::Url::parse(uri.as_str())
        && let Ok(path) = url.to_file_path()
    {
        return path;
    }
    PathBuf::from(uri.as_str().trim_start_matches("file://"))
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{program}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

fn find_project_root(path: &Path, workspace_dir: &Path, server_key: &str) -> PathBuf {
    let start = path.parent().unwrap_or_else(|| Path::new("."));
    let markers: &[&str] = match server_key {
        "rust" => &["Cargo.toml"],
        "go" => &["go.mod"],
        "typescript" => &["tsconfig.json", "jsconfig.json", "package.json"],
        "python" => &[
            "pyproject.toml",
            "setup.py",
            "setup.cfg",
            "requirements.txt",
        ],
        "c" => &[
            "compile_commands.json",
            "CMakeLists.txt",
            "meson.build",
            "Makefile",
        ],
        _ => &[".git"],
    };

    for dir in start.ancestors() {
        if markers.iter().any(|marker| dir.join(marker).exists()) {
            return dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        }
    }

    if !workspace_dir.as_os_str().is_empty() && start.starts_with(workspace_dir) {
        return workspace_dir
            .canonicalize()
            .unwrap_or_else(|_| workspace_dir.to_path_buf());
    }

    start.canonicalize().unwrap_or_else(|_| start.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_language_ids_for_web_and_jsx() {
        assert_eq!(
            detect_file_type(Path::new("index.html"))
                .unwrap()
                .language_id,
            "html"
        );
        assert_eq!(
            detect_file_type(Path::new("style.scss"))
                .unwrap()
                .language_id,
            "scss"
        );
        assert_eq!(
            detect_file_type(Path::new("app.tsx")).unwrap().language_id,
            "typescriptreact"
        );
        assert_eq!(
            detect_file_type(Path::new("app.jsx")).unwrap().language_id,
            "javascriptreact"
        );
        assert_eq!(
            detect_file_type(Path::new("main.h")).unwrap().language_id,
            "c"
        );
        assert_eq!(
            detect_file_type(Path::new("main.hpp")).unwrap().language_id,
            "cpp"
        );
    }

    #[test]
    fn detects_special_filenames() {
        assert_eq!(
            detect_file_type(Path::new("Dockerfile"))
                .unwrap()
                .language_id,
            "dockerfile"
        );
        assert!(detect_file_type(Path::new("README")).is_none());
    }

    #[test]
    fn uri_round_trips_spaces_and_unicode() {
        let dir = std::env::temp_dir().join("ocean lsp test");
        let path = dir.join("မြန်မာ.rs");
        let uri = path_to_uri(&path).expect("uri");
        assert!(uri.as_str().contains("ocean%20lsp%20test"));
        assert_eq!(uri_to_path(&uri), path);
    }

    #[test]
    fn parses_html_css_json_mappings() {
        let html = detect_file_type(Path::new("a.html")).unwrap();
        assert_eq!(html.server_key, "html");
        assert_eq!(html.candidates[0].program, "vscode-html-language-server");
        assert_eq!(html.candidates[0].args, &["--stdio"]);

        let css = detect_file_type(Path::new("a.css")).unwrap();
        assert_eq!(css.server_key, "css");
        assert_eq!(css.candidates[0].program, "vscode-css-language-server");

        let json = detect_file_type(Path::new("a.json")).unwrap();
        assert_eq!(json.server_key, "json");
    }

    #[test]
    fn parses_completion_and_hover_payloads() {
        let completion = json!({
            "isIncomplete": false,
            "items": [
                { "label": "println!", "detail": "macro", "insertText": "println!(\"\")" },
                { "label": "print", "textEdit": { "newText": "print" } }
            ]
        });
        let items = parse_completion(&completion);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].insert_text, "println!(\"\")");
        assert_eq!(items[1].insert_text, "print");

        let hover = json!({ "contents": { "kind": "markdown", "value": "**doc**" } });
        assert_eq!(parse_hover(&hover).as_deref(), Some("**doc**"));
    }
}
