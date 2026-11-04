use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const RELEASES_API_URL: &str = "https://api.github.com/repos/moekyawsoe/ocean/releases/latest";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateCheckMode {
    #[default]
    Daily,
    Always,
    Off,
}

impl UpdateCheckMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "daily" | "24h" | "day" => Some(Self::Daily),
            "always" | "on" | "true" | "every" => Some(Self::Always),
            "off" | "false" | "never" | "disabled" => Some(Self::Off),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Always => "always",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub tag_name: String,
    pub release_name: String,
    pub html_url: String,
    pub deb_url: Option<String>,
    pub rpm_url: Option<String>,
    pub tar_url: Option<String>,
    pub sha256_url: Option<String>,
    pub published_at: String,
    pub body: String,
}

/// Returns the current running version of Ocean.
///
/// Priority:
/// 1. Compile-time `CARGO_PKG_VERSION` from `Cargo.toml`.
/// 2. If installed via system package manager (`dpkg` or `rpm`), queries the package
///    database so installed packages accurately report their true distribution version.
/// 3. Returns the highest valid version found.
pub fn running_version() -> String {
    let compile_ver = env!("CARGO_PKG_VERSION");
    let mut best_ver = compile_ver.to_string();

    // Check Debian / Ubuntu dpkg database
    if let Ok(output) = Command::new("dpkg-query")
        .args(["-W", "-f=${Version}", "ocean"])
        .output()
    {
        if output.status.success() {
            let pkg_raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !pkg_raw.is_empty() {
                let clean = pkg_raw
                    .split('+').next().unwrap_or(&pkg_raw)
                    .split('~').next().unwrap_or(&pkg_raw)
                    .split('-').next().unwrap_or(&pkg_raw)
                    .to_string();
                if is_newer_version(&best_ver, &clean) {
                    best_ver = clean;
                }
            }
        }
    }

    // Check Fedora / RHEL / openSUSE rpm database
    if let Ok(output) = Command::new("rpm")
        .args(["-q", "--qf", "%{VERSION}", "ocean"])
        .output()
    {
        if output.status.success() {
            let pkg_ver = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !pkg_ver.is_empty() && is_newer_version(&best_ver, &pkg_ver) {
                best_ver = pkg_ver;
            }
        }
    }

    best_ver
}

/// Parse and compare semantic version strings (e.g. "1.0.0" vs "v1.0.0", "1.0.0-1" vs "v1.0.0").
/// Strips 'v'/'V', Debian/RPM revisions ('-', '~', '+'), and compares numeric segments.
/// Returns true only if candidate is strictly greater than current.
pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let parse_nums = |s: &str| -> Vec<u64> {
        let clean = s.trim().trim_start_matches(|c| c == 'v' || c == 'V');
        let semver_part = clean
            .split('+').next().unwrap_or(clean)
            .split('~').next().unwrap_or(clean)
            .split('-').next().unwrap_or(clean);
        semver_part
            .split('.')
            .filter_map(|seg| {
                let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
                if digits.is_empty() {
                    None
                } else {
                    digits.parse::<u64>().ok()
                }
            })
            .collect()
    };

    let cur_parts = parse_nums(current);
    let cand_parts = parse_nums(candidate);

    let max_len = cur_parts.len().max(cand_parts.len());
    for i in 0..max_len {
        let cur_val = cur_parts.get(i).copied().unwrap_or(0);
        let cand_val = cand_parts.get(i).copied().unwrap_or(0);
        if cand_val > cur_val {
            return true;
        } else if cand_val < cur_val {
            return false;
        }
    }

    false
}

/// Parse GitHub Release API JSON response and return UpdateInfo if newer than current_version.
pub fn parse_release_json(
    json_str: &str,
    current_version: &str,
) -> Result<Option<UpdateInfo>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid JSON response: {e}"))?;

    let tag_name = v["tag_name"]
        .as_str()
        .ok_or_else(|| "Missing tag_name in release payload".to_string())?
        .trim();

    let clean_version = tag_name
        .trim_start_matches(|c| c == 'v' || c == 'V')
        .to_string();

    if !is_newer_version(current_version, &clean_version) {
        return Ok(None);
    }

    let release_name = v["name"].as_str().unwrap_or(tag_name).to_string();
    let html_url = v["html_url"].as_str().unwrap_or("").to_string();
    let published_at = v["published_at"].as_str().unwrap_or("").to_string();
    let body = v["body"].as_str().unwrap_or("").to_string();

    let mut deb_url = None;
    let mut rpm_url = None;
    let mut tar_url = None;
    let mut sha256_url = None;

    if let Some(assets) = v["assets"].as_array() {
        for a in assets {
            let name = a["name"].as_str().unwrap_or("");
            let url = a["browser_download_url"].as_str().unwrap_or("");
            if url.is_empty() {
                continue;
            }

            if name.ends_with(".deb") && deb_url.is_none() {
                deb_url = Some(url.to_string());
            } else if name.ends_with(".rpm") && rpm_url.is_none() {
                rpm_url = Some(url.to_string());
            } else if (name.ends_with("linux-x86_64.tar.gz") || name.ends_with(".tar.gz"))
                && tar_url.is_none()
            {
                tar_url = Some(url.to_string());
            } else if name == "SHA256SUMS" && sha256_url.is_none() {
                sha256_url = Some(url.to_string());
            }
        }
    }

    Ok(Some(UpdateInfo {
        version: clean_version,
        tag_name: tag_name.to_string(),
        release_name,
        html_url,
        deb_url,
        rpm_url,
        tar_url,
        sha256_url,
        published_at,
        body,
    }))
}

/// Execute online check using system curl tool.
pub fn check_for_update_online(current_version: &str) -> Result<Option<UpdateInfo>, String> {
    let output = Command::new("curl")
        .arg("-sSL")
        .arg("-H")
        .arg(format!("User-Agent: ocean-terminal/{current_version}"))
        .arg("-H")
        .arg("Accept: application/vnd.github+json")
        .arg("--connect-timeout")
        .arg("5")
        .arg("--max-time")
        .arg("10")
        .arg(RELEASES_API_URL)
        .output()
        .map_err(|e| format!("Failed executing curl: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "curl exited with code {}",
            output.status.code().unwrap_or(-1)
        ));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_release_json(&body, current_version)
}

fn cache_file_path() -> Option<PathBuf> {
    if let Ok(cache_home) = std::env::var("XDG_CACHE_HOME") {
        Some(PathBuf::from(cache_home).join("ocean/update_cache.json"))
    } else if let Ok(home) = std::env::var("HOME") {
        Some(PathBuf::from(home).join(".cache/ocean/update_cache.json"))
    } else {
        None
    }
}

pub fn get_cached_update(current_version: &str) -> Option<(u64, Option<UpdateInfo>)> {
    let path = cache_file_path()?;
    let content = fs::read_to_string(&path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;

    let timestamp = val["timestamp"].as_u64()?;
    let tag = val["tag_name"].as_str().unwrap_or("");
    let clean_ver = tag.trim_start_matches(|c| c == 'v' || c == 'V');

    if is_newer_version(current_version, clean_ver) {
        Some((
            timestamp,
            Some(UpdateInfo {
                version: clean_ver.to_string(),
                tag_name: tag.to_string(),
                release_name: val["name"].as_str().unwrap_or(tag).to_string(),
                html_url: val["html_url"].as_str().unwrap_or("").to_string(),
                deb_url: val["deb_url"].as_str().map(|s| s.to_string()),
                rpm_url: val["rpm_url"].as_str().map(|s| s.to_string()),
                tar_url: val["tar_url"].as_str().map(|s| s.to_string()),
                sha256_url: val["sha256_url"].as_str().map(|s| s.to_string()),
                published_at: val["published_at"].as_str().unwrap_or("").to_string(),
                body: val["body"].as_str().unwrap_or("").to_string(),
            }),
        ))
    } else {
        if val.get("tag_name").is_some() {
            save_cached_update(None);
        }
        Some((timestamp, None))
    }
}

pub fn save_cached_update(info: Option<&UpdateInfo>) {
    let Some(path) = cache_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let val = if let Some(u) = info {
        serde_json::json!({
            "timestamp": now,
            "tag_name": u.tag_name,
            "version": u.version,
            "name": u.release_name,
            "html_url": u.html_url,
            "deb_url": u.deb_url,
            "rpm_url": u.rpm_url,
            "tar_url": u.tar_url,
            "sha256_url": u.sha256_url,
            "published_at": u.published_at,
            "body": u.body,
        })
    } else {
        serde_json::json!({
            "timestamp": now,
            "has_update": false,
        })
    };

    let _ = fs::write(&path, val.to_string());
}

/// Detect host package management environment to select the best upgrade path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageSystem {
    Deb,
    Rpm,
    Portable,
}

pub fn detect_package_system() -> PackageSystem {
    if PathBuf::from("/etc/debian_version").exists()
        || Command::new("which")
            .arg("apt-get")
            .output()
            .is_ok_and(|o| o.status.success())
    {
        PackageSystem::Deb
    } else if PathBuf::from("/etc/fedora-release").exists()
        || PathBuf::from("/etc/redhat-release").exists()
        || Command::new("which")
            .arg("dnf")
            .output()
            .is_ok_and(|o| o.status.success())
    {
        PackageSystem::Rpm
    } else {
        PackageSystem::Portable
    }
}

/// Generate a shell command that runs inside an Ocean terminal tab to update the package.
pub fn generate_update_command(info: &UpdateInfo) -> (String, Vec<String>) {
    let pkg_sys = detect_package_system();

    let script = match pkg_sys {
        PackageSystem::Deb if info.deb_url.is_some() => {
            let url = info.deb_url.as_ref().unwrap();
            format!(
                r#"echo "============================================================"
echo " Ocean Terminal Auto-Updater: Upgrading to {ver}"
echo " Release: {tag} ({url})"
echo "============================================================"
echo ""
TMP_DEB="/tmp/ocean_{ver}_amd64.deb"
echo "-> Downloading Debian package..."
curl -# -fL -o "$TMP_DEB" "{url}" || {{ echo "Download failed!"; read -p "Press Enter to exit..." _; exit 1; }}

echo "-> Installing with sudo..."
if command -v apt-get >/dev/null 2>&1; then
    sudo apt-get install -y --reinstall "$TMP_DEB" || sudo dpkg -i "$TMP_DEB"
else
    sudo dpkg -i "$TMP_DEB"
fi

if [ $? -eq 0 ]; then
    echo ""
    echo "============================================================"
    echo " [SUCCESS] Ocean Terminal successfully upgraded to {ver}!"
    echo " You can restart Ocean anytime to enjoy the latest release."
    echo "============================================================"
else
    echo ""
    echo "[ERROR] Installation encountered an error."
fi
rm -f "$TMP_DEB"
echo ""
read -p "Press Enter to close this update tab..." _
"#,
                ver = info.version,
                tag = info.tag_name,
                url = url
            )
        }
        PackageSystem::Rpm if info.rpm_url.is_some() => {
            let url = info.rpm_url.as_ref().unwrap();
            format!(
                r#"echo "============================================================"
echo " Ocean Terminal Auto-Updater: Upgrading to {ver}"
echo " Release: {tag} ({url})"
echo "============================================================"
echo ""
TMP_RPM="/tmp/ocean_{ver}.rpm"
echo "-> Downloading RPM package..."
curl -# -fL -o "$TMP_RPM" "{url}" || {{ echo "Download failed!"; read -p "Press Enter to exit..." _; exit 1; }}

echo "-> Installing with sudo..."
if command -v dnf >/dev/null 2>&1; then
    sudo dnf install -y "$TMP_RPM" || sudo rpm -Uvh "$TMP_RPM"
else
    sudo rpm -Uvh "$TMP_RPM"
fi

if [ $? -eq 0 ]; then
    echo ""
    echo "============================================================"
    echo " [SUCCESS] Ocean Terminal successfully upgraded to {ver}!"
    echo " You can restart Ocean anytime to enjoy the latest release."
    echo "============================================================"
else
    echo ""
    echo "[ERROR] Installation encountered an error."
fi
rm -f "$TMP_RPM"
echo ""
read -p "Press Enter to close this update tab..." _
"#,
                ver = info.version,
                tag = info.tag_name,
                url = url
            )
        }
        _ => {
            let url = info
                .tar_url
                .as_ref()
                .or(info.deb_url.as_ref())
                .cloned()
                .unwrap_or_default();
            format!(
                r#"echo "============================================================"
echo " Ocean Terminal Auto-Updater: Portable Install ({ver})"
echo "============================================================"
echo ""
TMP_DIR="/tmp/ocean_up_{ver}"
mkdir -p "$TMP_DIR"
echo "-> Downloading archive..."
curl -# -fL -o "$TMP_DIR/ocean.tar.gz" "{url}" || {{ echo "Download failed!"; read -p "Press Enter to exit..." _; exit 1; }}
tar -xzf "$TMP_DIR/ocean.tar.gz" -C "$TMP_DIR"
TARGET_DIR="$HOME/.local/bin"
mkdir -p "$TARGET_DIR"
if [ -f "$TMP_DIR/ocean" ]; then
    cp "$TMP_DIR/ocean" "$TARGET_DIR/ocean"
    chmod +x "$TARGET_DIR/ocean"
    echo ""
    echo "============================================================"
    echo " [SUCCESS] Ocean updated at $TARGET_DIR/ocean ({ver})!"
    echo "============================================================"
else
    echo "Extracted files located in $TMP_DIR"
fi
echo ""
read -p "Press Enter to close this update tab..." _
"#,
                ver = info.version,
                url = url
            )
        }
    };

    ("sh".to_string(), vec!["-c".to_string(), script])
}

/// Initiates an asynchronous update check according to the configured mode.
/// If cached info is recent (within 24 hours), emits it immediately without network traffic.
/// Otherwise, queries the GitHub Releases API in a background thread without blocking.
pub fn start_background_update_check(
    mode: UpdateCheckMode,
    current_version: String,
    proxy: winit::event_loop::EventLoopProxy<crate::terminal::AppEvent>,
) {
    if mode == UpdateCheckMode::Off {
        return;
    }

    // 1. Check local cache first (avoids hitting GitHub rate limits on frequent launches)
    if mode == UpdateCheckMode::Daily {
        if let Some((ts, cached)) = get_cached_update(&current_version) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let age_secs = now.saturating_sub(ts);
            // Cache valid for 24 hours (86,400 seconds)
            if age_secs < 86400 {
                if let Some(info) = cached {
                    let _ = proxy.send_event(crate::terminal::AppEvent::UpdateAvailable(info));
                }
                return;
            }
        }
    }

    // 2. Spawn lightweight background thread to query GitHub Releases API asynchronously
    std::thread::spawn(move || {
        match check_for_update_online(&current_version) {
            Ok(Some(info)) => {
                save_cached_update(Some(&info));
                let _ = proxy.send_event(crate::terminal::AppEvent::UpdateAvailable(info));
            }
            Ok(None) => {
                save_cached_update(None);
            }
            Err(_) => {
                // Silently ignore offline or rate-limit errors during background check
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_accurately_detects_newer_tags() {
        assert!(is_newer_version("0.1.0", "v1.0.0"));
        assert!(is_newer_version("0.1.0", "1.0.0"));
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.1.0", "0.1.1"));
        assert!(is_newer_version("1.0.0", "1.0.1"));
        assert!(is_newer_version("1.0.0", "1.1.0"));
        assert!(is_newer_version("1.0.0", "2.0.0"));

        // Same or older versions
        assert!(!is_newer_version("1.0.0", "v1.0.0"));
        assert!(!is_newer_version("v1.0.0", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "1.0.0"));
        assert!(!is_newer_version("1.0.0-1", "v1.0.0"));
        assert!(!is_newer_version("1.0.0-1", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "1.0.0-1"));
        assert!(!is_newer_version("1.0.0+1", "1.0.0"));
        assert!(!is_newer_version("1.0.0~beta1", "1.0.0"));
        assert!(!is_newer_version("1.0.1", "1.0.0"));
        assert!(!is_newer_version("2.0.0", "1.9.9"));
    }

    #[test]
    fn running_version_resolves_cleanly() {
        let ver = running_version();
        assert!(!ver.is_empty());
        let nums: Vec<u64> = ver.split('.').filter_map(|s| s.parse().ok()).collect();
        assert!(!nums.is_empty());
    }

    #[test]
    fn parses_github_release_payload_and_finds_deb_and_rpm() {
        let sample_json = r#"{
            "tag_name": "v1.0.0",
            "name": "Ocean Terminal v1.0.0",
            "html_url": "https://github.com/moekyawsoe/ocean/releases/tag/v1.0.0",
            "published_at": "2026-11-04T04:39:08Z",
            "body": "Release notes here",
            "assets": [
                {
                    "name": "ocean_amd64.deb",
                    "browser_download_url": "https://github.com/moekyawsoe/ocean/releases/download/v1.0.0/ocean_amd64.deb"
                },
                {
                    "name": "ocean.x86_64.rpm",
                    "browser_download_url": "https://github.com/moekyawsoe/ocean/releases/download/v1.0.0/ocean.x86_64.rpm"
                },
                {
                    "name": "ocean-linux-x86_64.tar.gz",
                    "browser_download_url": "https://github.com/moekyawsoe/ocean/releases/download/v1.0.0/ocean-linux-x86_64.tar.gz"
                },
                {
                    "name": "SHA256SUMS",
                    "browser_download_url": "https://github.com/moekyawsoe/ocean/releases/download/v1.0.0/SHA256SUMS"
                }
            ]
        }"#;

        let res = parse_release_json(sample_json, "0.1.0").expect("parse success");
        let info = res.expect("newer version found");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.tag_name, "v1.0.0");
        assert!(info.deb_url.as_deref().unwrap().ends_with(".deb"));
        assert!(info.rpm_url.as_deref().unwrap().ends_with(".rpm"));
        assert!(info.tar_url.as_deref().unwrap().ends_with(".tar.gz"));
        assert_eq!(
            info.sha256_url.as_deref().unwrap(),
            "https://github.com/moekyawsoe/ocean/releases/download/v1.0.0/SHA256SUMS"
        );

        // When running version is already 1.0.0, returns None
        let same = parse_release_json(sample_json, "1.0.0").expect("parse success");
        assert!(same.is_none());
    }

    #[test]
    fn generates_appropriate_update_command_for_distro() {
        let info = UpdateInfo {
            version: "1.0.0".to_string(),
            tag_name: "v1.0.0".to_string(),
            release_name: "Ocean Terminal v1.0.0".to_string(),
            html_url: "https://github.com/moekyawsoe/ocean".to_string(),
            deb_url: Some("https://example.com/ocean.deb".to_string()),
            rpm_url: Some("https://example.com/ocean.rpm".to_string()),
            tar_url: Some("https://example.com/ocean.tar.gz".to_string()),
            sha256_url: None,
            published_at: "2026-11-04".to_string(),
            body: "".to_string(),
        };

        let (prog, args) = generate_update_command(&info);
        assert_eq!(prog, "sh");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "-c");
        assert!(args[1].contains("Auto-Updater"));
        assert!(args[1].contains("1.0.0"));
    }
}
