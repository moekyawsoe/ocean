use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub entry: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginRegistry {
    #[serde(default)]
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub path: PathBuf,
    pub sha256: String,
}

pub fn config_root() -> Option<PathBuf> {
    crate::config::theme::dirs_config_dir().map(|dir| dir.join("ocean"))
}

pub fn plugins_dir() -> Option<PathBuf> {
    let dir = config_root()?.join("plugins");
    let _ = fs::create_dir_all(&dir);
    Some(dir)
}

fn registry_path() -> Option<PathBuf> {
    plugins_dir().map(|dir| dir.join("registry.json"))
}

pub fn load_registry() -> PluginRegistry {
    let Some(path) = registry_path() else {
        return PluginRegistry::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_registry(registry: &PluginRegistry) -> Result<(), String> {
    let Some(path) = registry_path() else {
        return Err("Could not resolve the Ocean config directory".to_string());
    };
    let json = serde_json::to_string_pretty(registry).map_err(|err| err.to_string())?;
    write_file_atomic(&path, json.as_bytes())
}

fn manifest_path(id: &str) -> Option<PathBuf> {
    plugins_dir().map(|dir| dir.join(format!("{id}.json")))
}

pub fn load_manifest(id: &str) -> Option<PluginManifest> {
    let path = manifest_path(id)?;
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn save_manifest(manifest: &PluginManifest) -> Result<(), String> {
    let Some(path) = manifest_path(&manifest.id) else {
        return Err("Could not resolve the Ocean plugins directory".to_string());
    };
    let json = serde_json::to_string_pretty(manifest).map_err(|err| err.to_string())?;
    fs::write(path, json).map_err(|err| err.to_string())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!(
            "Invalid plugin id `{id}` (use letters, digits, '-' or '_')"
        ));
    }
    Ok(())
}

fn wasm_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[..4] == b"\0asm"
}

pub fn list() -> Vec<PluginInfo> {
    let Some(dir) = plugins_dir() else {
        return Vec::new();
    };
    let registry = load_registry();
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("wasm") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("plugin")
            .to_string();
        let manifest = load_manifest(&id);
        let embedded_hash = fs::read(&path)
            .map(|bytes| sha256_hex(&bytes))
            .unwrap_or_default();
        out.push(PluginInfo {
            id: id.clone(),
            name: manifest
                .as_ref()
                .map(|manifest| manifest.name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| id.clone()),
            version: manifest
                .as_ref()
                .map(|manifest| manifest.version.clone())
                .unwrap_or_default(),
            description: manifest
                .as_ref()
                .map(|manifest| manifest.description.clone())
                .unwrap_or_default(),
            enabled: !registry.disabled.contains(&id),
            path,
            sha256: manifest
                .as_ref()
                .map(|manifest| manifest.sha256.clone())
                .filter(|hash| !hash.is_empty())
                .unwrap_or(embedded_hash),
        });
    }
    out.sort_by_key(|plugin| plugin.name.to_lowercase());
    out
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let mut registry = load_registry();
    registry.disabled.retain(|entry| entry != id);
    if !enabled {
        registry.disabled.push(id.to_string());
    }
    save_registry(&registry)
}

pub fn install_local(
    source: &Path,
    force: bool,
    source_label: Option<&str>,
) -> Result<PluginInfo, String> {
    if source.extension().and_then(|ext| ext.to_str()) != Some("wasm") {
        return Err("Plugin install expects a .wasm file".to_string());
    }
    let bytes = fs::read(source).map_err(|err| format!("Read {}: {err}", source.display()))?;
    if !wasm_magic(&bytes) {
        return Err("File is not a valid WebAssembly module".to_string());
    }
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Plugin exceeds the 32 MiB size limit".to_string());
    }

    let id = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plugin")
        .to_string();
    validate_id(&id)?;
    let dir = plugins_dir().ok_or_else(|| "Could not resolve plugins directory".to_string())?;
    let target = dir.join(format!("{id}.wasm"));
    if target.exists() && !force {
        return Err(format!(
            "Plugin `{id}` already exists (pass --force to overwrite)"
        ));
    }
    if target.canonicalize().ok() != source.canonicalize().ok() {
        fs::copy(source, &target).map_err(|err| format!("Install {}: {err}", target.display()))?;
    }

    let hash = sha256_hex(&bytes);
    let mut manifest = load_manifest(&id).unwrap_or(PluginManifest {
        id: id.clone(),
        name: id.clone(),
        version: "0.0.0".to_string(),
        description: String::new(),
        entry: format!("{id}.wasm"),
        sha256: hash.clone(),
        source: String::new(),
    });
    manifest.entry = format!("{id}.wasm");
    manifest.sha256 = hash.clone();
    if let Some(label) = source_label {
        manifest.source = label.to_string();
    } else if manifest.source.is_empty() {
        manifest.source = source.display().to_string();
    }
    save_manifest(&manifest)?;
    set_enabled(&id, true)?;

    Ok(PluginInfo {
        id,
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        enabled: true,
        path: target,
        sha256: hash,
    })
}

pub fn install_url(url: &str, force: bool) -> Result<PluginInfo, String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Plugin URL must start with http:// or https://".to_string());
    }
    let temp = std::env::temp_dir().join(format!("ocean-plugin-{}.wasm", std::process::id()));
    let status = Command::new("curl")
        .args(["-fSL", "--connect-timeout", "5", "--max-time", "120", "-o"])
        .arg(&temp)
        .arg(url)
        .status()
        .map_err(|err| format!("Could not run curl: {err}"))?;
    if !status.success() {
        let _ = fs::remove_file(&temp);
        return Err(format!("Download failed for {url}"));
    }
    let result = install_local(&temp, force, Some(url));
    let _ = fs::remove_file(&temp);
    result
}

pub fn remove(id: &str, force: bool) -> Result<(), String> {
    validate_id(id)?;
    let info = list().into_iter().find(|info| info.id == id);
    let Some(info) = info else {
        return Err(format!("Plugin `{id}` is not installed"));
    };
    if info.enabled && !force {
        return Err(format!(
            "Plugin `{id}` is enabled; disable it first or pass --force"
        ));
    }
    let _ = fs::remove_file(&info.path);
    if let Some(path) = manifest_path(id) {
        let _ = fs::remove_file(path);
    }
    let mut registry = load_registry();
    registry.disabled.retain(|entry| entry != id);
    save_registry(&registry)
}

pub fn verify(id: &str) -> Result<bool, String> {
    let info = list()
        .into_iter()
        .find(|info| info.id == id)
        .ok_or_else(|| format!("Plugin `{id}` is not installed"))?;
    let bytes = fs::read(&info.path).map_err(|err| err.to_string())?;
    let actual = sha256_hex(&bytes);
    Ok(info.sha256.is_empty() || actual == info.sha256)
}

pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp).map_err(|err| err.to_string())?;
        file.write_all(bytes).map_err(|err| err.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_plugin_ids() {
        assert!(validate_id("format").is_ok());
        assert!(validate_id("../escape").is_err());
        assert!(validate_id("").is_err());
    }

    #[test]
    fn detects_wasm_magic() {
        assert!(wasm_magic(b"\0asm\x01\0\0\0"));
        assert!(!wasm_magic(b"not wasm"));
    }
}
