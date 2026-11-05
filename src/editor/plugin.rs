use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use extism::{Function, Manifest, Plugin, PluginBuilder, UserData, ValType, Wasm};

#[derive(Default, Clone)]
pub struct HostSharedState {
    pub buffer_text: Arc<Mutex<String>>,
    pub pending_inserts: Arc<Mutex<Vec<(usize, String)>>>,
    pub registered_commands: Arc<Mutex<Vec<(String, String)>>>,
    pub status_items: Arc<Mutex<Vec<String>>>,
}

extism::host_fn!(pub ocean_get_buffer_slice(user_data: HostSharedState; offset: i64, len: i64) -> String {
    let state_lock = user_data.get()?;
    let state = state_lock.lock().unwrap();
    let text = state.buffer_text.lock().unwrap();
    let start = (offset.max(0) as usize).min(text.len());
    let end = ((offset + len).max(0) as usize).min(text.len());
    Ok(text[start..end].to_string())
});

extism::host_fn!(pub ocean_insert_text(user_data: HostSharedState; offset: i64, text: String) -> () {
    let state_lock = user_data.get()?;
    let state = state_lock.lock().unwrap();
    state.pending_inserts.lock().unwrap().push((offset.max(0) as usize, text));
    Ok(())
});

extism::host_fn!(pub ocean_register_command(user_data: HostSharedState; name: String, callback: String) -> () {
    let state_lock = user_data.get()?;
    let state = state_lock.lock().unwrap();
    state.registered_commands.lock().unwrap().push((name, callback));
    Ok(())
});

extism::host_fn!(pub ocean_add_status_item(user_data: HostSharedState; text: String) -> () {
    let state_lock = user_data.get()?;
    let state = state_lock.lock().unwrap();
    state.status_items.lock().unwrap().push(text);
    Ok(())
});

pub struct LoadedPlugin {
    pub name: String,
    pub path: PathBuf,
    pub plugin: Plugin,
}

pub struct PluginEngine {
    pub state: HostSharedState,
    pub plugins: Vec<LoadedPlugin>,
}

impl Default for PluginEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            state: HostSharedState::default(),
            plugins: Vec::new(),
        };
        engine.reload_plugins();
        engine
    }

    pub fn plugins_dir() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".config")
                .join("ocean")
                .join("plugins"),
        )
    }

    pub fn reload_plugins(&mut self) {
        self.plugins.clear();
        let Some(dir) = Self::plugins_dir() else {
            return;
        };

        if !dir.exists() {
            let _ = fs::create_dir_all(&dir);
            return;
        }

        let Ok(entries) = fs::read_dir(&dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("plugin")
                    .to_string();

                if let Ok(wasm_bytes) = fs::read(&path) {
                    let manifest = Manifest::new([Wasm::data(wasm_bytes)]);

                    let f_slice = Function::new(
                        "ocean_get_buffer_slice",
                        [ValType::I64, ValType::I64],
                        [ValType::I64],
                        UserData::new(self.state.clone()),
                        ocean_get_buffer_slice,
                    );

                    let f_insert = Function::new(
                        "ocean_insert_text",
                        [ValType::I64, ValType::I64],
                        [],
                        UserData::new(self.state.clone()),
                        ocean_insert_text,
                    );

                    let f_cmd = Function::new(
                        "ocean_register_command",
                        [ValType::I64, ValType::I64],
                        [],
                        UserData::new(self.state.clone()),
                        ocean_register_command,
                    );

                    let f_status = Function::new(
                        "ocean_add_status_item",
                        [ValType::I64],
                        [],
                        UserData::new(self.state.clone()),
                        ocean_add_status_item,
                    );

                    let builder = PluginBuilder::new(manifest)
                        .with_wasi(true)
                        .with_functions([f_slice, f_insert, f_cmd, f_status]);

                    if let Ok(mut plugin) = builder.build() {
                        if plugin.function_exists("_start") {
                            let _ = plugin.call::<(), ()>("_start", ());
                        }
                        if plugin.function_exists("init") {
                            let _ = plugin.call::<(), ()>("init", ());
                        }
                        self.plugins.push(LoadedPlugin { name, path, plugin });
                    }
                }
            }
        }
    }

    pub fn sync_buffer_text(&self, text: &str) {
        let mut b = self.state.buffer_text.lock().unwrap();
        *b = text.to_string();
    }

    pub fn drain_pending_inserts(&self) -> Vec<(usize, String)> {
        let mut inserts = self.state.pending_inserts.lock().unwrap();
        inserts.drain(..).collect()
    }

    pub fn status_items(&self) -> Vec<String> {
        let items = self.state.status_items.lock().unwrap();
        items.clone()
    }

    pub fn registered_commands(&self) -> Vec<(String, String)> {
        let cmds = self.state.registered_commands.lock().unwrap();
        cmds.clone()
    }

    pub fn execute_command(
        &mut self,
        plugin_name: &str,
        func_name: &str,
    ) -> Result<String, String> {
        for p in &mut self.plugins {
            if p.name == plugin_name && p.plugin.function_exists(func_name) {
                return p
                    .plugin
                    .call::<(), String>(func_name, ())
                    .map_err(|e| format!("WASM plugin error: {e}"));
            }
        }
        Err(format!(
            "Plugin {plugin_name} or function {func_name} not found"
        ))
    }
}
