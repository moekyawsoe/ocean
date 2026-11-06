mod app;
mod config;
mod confirm;
pub mod editor;
pub mod file;
mod font;
mod help;
mod ipc;
mod menu;
mod menubar;
pub mod metrics;
mod plugins;
mod renderer;
mod session;
mod split;
mod statusbar;
mod terminal;
mod update;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use winit::event_loop::EventLoop;

use app::{LaunchOptions, OceanApp};
use config::Config;
use config::theme::ThemePalette;
use font::shaper::{CellInput, TextShaper};
use font::{FontStyle, FontSystem};
use renderer::{RenderEngine, save_scene_png};
use terminal::snapshot_from_bytes;

use crate::config::STARTER_CONFIG;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCli {
    pub config_path: Option<PathBuf>,
    pub working_dir: Option<PathBuf>,
    pub exec_cmd: Option<(String, Vec<String>)>,
    pub screenshot_out: Option<PathBuf>,
    pub screenshot_preset: Option<String>,
    pub plus_action: Option<String>,
    pub plus_args: Vec<String>,
    pub force_flag: bool,
    pub backend: Option<String>,
    pub cli_overrides: Vec<(String, String)>,
    pub editor_file: Option<PathBuf>,
    pub initial_editor_mode: bool,
    pub new_editor_window: bool,
    pub print_help: bool,
    pub print_version: bool,
}

pub fn parse_cli_args(prog_name: &str, args: &[String]) -> ParsedCli {
    let mut config_path: Option<PathBuf> = None;
    let mut working_dir: Option<PathBuf> = None;
    let mut exec_cmd: Option<(String, Vec<String>)> = None;
    let mut screenshot_out: Option<PathBuf> = None;
    let mut screenshot_preset: Option<String> = None;
    let mut plus_action: Option<String> = None;
    let mut plus_args: Vec<String> = Vec::new();
    let mut force_flag = false;
    let mut backend: Option<String> = None;
    let mut cli_overrides: Vec<(String, String)> = Vec::new();

    let is_edit_binary = prog_name.ends_with("/edit") || prog_name == "edit";
    let mut editor_file: Option<PathBuf> = None;
    let mut initial_editor_mode = is_edit_binary;
    let mut window_choice: Option<bool> = None;
    let mut print_help = false;
    let mut print_version = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "edit" => {
                initial_editor_mode = true;
                if window_choice.is_none() {
                    window_choice = Some(true);
                }
                if i + 1 < args.len()
                    && !args[i + 1].starts_with('-')
                    && !args[i + 1].starts_with('+')
                {
                    editor_file = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                    continue;
                }
            }
            "-w" | "--window" | "--editor-window" => {
                initial_editor_mode = true;
                window_choice = Some(true);
            }
            "--here" | "--current-window" | "--in-place" => {
                initial_editor_mode = true;
                window_choice = Some(false);
            }
            "-h" | "--help" | "+help" => {
                print_help = true;
                break;
            }
            "-v" | "--version" | "+version" => {
                print_version = true;
                break;
            }
            "-e" | "--command" => {
                if i + 1 < args.len() {
                    let prog = args[i + 1].clone();
                    let rest = args[i + 2..].to_vec();
                    exec_cmd = Some((prog, rest));
                }
                break;
            }
            "-c" | "--config-file" => {
                if i + 1 < args.len() {
                    config_path = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                    continue;
                }
            }
            "--working-directory" => {
                if i + 1 < args.len() {
                    working_dir = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                    continue;
                }
            }
            "--screenshot" => {
                if i + 1 < args.len() {
                    screenshot_out = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                    continue;
                }
            }
            "--screenshot-preset" => {
                if i + 1 < args.len() {
                    screenshot_preset = Some(args[i + 1].clone());
                    i += 2;
                    continue;
                }
            }
            "--force" => {
                force_flag = true;
            }
            "--x11" => {
                backend = Some("x11".to_string());
            }
            "--wayland" => {
                backend = Some("wayland".to_string());
            }
            "--backend" => {
                if i + 1 < args.len() {
                    backend = Some(args[i + 1].clone());
                    i += 2;
                    continue;
                }
            }
            s if s.starts_with('+') => {
                plus_action = Some(s.to_string());
                let mut next = i + 1;
                while next < args.len()
                    && !args[next].starts_with('+')
                    && !args[next].starts_with("--")
                {
                    plus_args.push(args[next].clone());
                    next += 1;
                }
                i = next;
                continue;
            }
            s if s.starts_with("--") => {
                let kv = &s[2..];
                if let Some((k, v)) = kv.split_once('=') {
                    if k == "config-file" {
                        config_path = Some(PathBuf::from(v));
                    } else if k == "working-directory" {
                        working_dir = Some(PathBuf::from(v));
                    } else if k == "screenshot" {
                        screenshot_out = Some(PathBuf::from(v));
                    } else if k == "screenshot-preset" {
                        screenshot_preset = Some(v.to_string());
                    } else if k == "backend" || k == "linux-display-server" {
                        backend = Some(v.to_string());
                    } else {
                        cli_overrides.push((k.to_string(), v.to_string()));
                    }
                } else if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    cli_overrides.push((kv.to_string(), args[i + 1].clone()));
                    i += 2;
                    continue;
                }
            }
            s if !s.starts_with('-')
                && !s.starts_with('+')
                && initial_editor_mode
                && editor_file.is_none() =>
            {
                editor_file = Some(PathBuf::from(s));
            }
            _ => {}
        }
        i += 1;
    }

    if initial_editor_mode && editor_file.is_none() {
        let cur_dir = working_dir
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        editor_file = Some(cur_dir);
    }

    ParsedCli {
        config_path,
        working_dir,
        exec_cmd,
        screenshot_out,
        screenshot_preset,
        plus_action,
        plus_args,
        force_flag,
        backend,
        cli_overrides,
        editor_file,
        initial_editor_mode,
        new_editor_window: window_choice.unwrap_or(is_edit_binary),
        print_help,
        print_version,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let prog_name = std::env::args().next().unwrap_or_default();

    let is_file_binary = prog_name.ends_with("/file")
        || prog_name == "file"
        || prog_name.ends_with("/ocean-file")
        || prog_name == "ocean-file"
        || prog_name.ends_with("/file-ocean")
        || prog_name == "file-ocean";

    if is_file_binary || args.first().map(|s| s.as_str()) == Some("file") {
        let file_args = if !is_file_binary && args.first().map(|s| s.as_str()) == Some("file") {
            &args[1..]
        } else {
            &args[..]
        };
        crate::file::run_file_cli(file_args);
        return;
    }

    let cli = parse_cli_args(&prog_name, &args);

    if cli.print_help {
        print_help();
        return;
    }
    if cli.print_version {
        println!("ocean {}", crate::update::running_version());
        return;
    }

    let mut config = Config::load(cli.config_path.as_deref());
    for (k, v) in &cli.cli_overrides {
        config.apply_cli_kv(k, v);
    }

    if let Some(act) = cli.plus_action {
        run_plus_action(&act, &config, cli.force_flag, &cli.plus_args);
        return;
    }

    // `edit` inside an Ocean shell should reuse the current window and switch
    // that pane's window into editor mode instead of spawning a new window.
    if cli.initial_editor_mode
        && !cli.new_editor_window
        && crate::ipc::try_send_edit_request(cli.editor_file.as_deref())
    {
        return;
    }

    for warn in &config.warnings {
        eprintln!("[ocean config warning] {warn}");
    }

    if let Some(out_png) = cli.screenshot_out {
        if let Err(err) = run_screenshot(
            &out_png,
            &config,
            cli.exec_cmd.as_ref(),
            cli.screenshot_preset.as_deref(),
        ) {
            eprintln!("ocean screenshot failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    let backend_choice = cli
        .backend
        .as_deref()
        .or(std::env::var("OCEAN_BACKEND").ok().as_deref())
        .and_then(|s| s.parse::<crate::config::BackendMode>().ok())
        .unwrap_or(config.backend);

    let mut event_loop_builder = EventLoop::<terminal::AppEvent>::with_user_event();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        use winit::platform::x11::EventLoopBuilderExtX11;

        match backend_choice {
            crate::config::BackendMode::X11 => {
                event_loop_builder.with_x11();
            }
            crate::config::BackendMode::Wayland => {
                event_loop_builder.with_wayland();
            }
            crate::config::BackendMode::Auto => {
                // Auto detection:
                // Under GNOME Wayland sessions (Mutter), a known Wayland protocol race condition
                // (error 0: invalid global wl_output) occurs in Mutter when the screen blanks, sleeps,
                // or suspends, which terminates native Wayland clients abruptly.
                // If running under GNOME Wayland and XWayland (DISPLAY) is available, use X11/XWayland
                // by default to prevent ocean from being killed during screen blank or system sleep.
                let is_gnome = std::env::var("XDG_CURRENT_DESKTOP")
                    .map(|v| v.to_ascii_uppercase().contains("GNOME"))
                    .unwrap_or(false);
                let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
                let has_x11 = std::env::var_os("DISPLAY").is_some();
                if is_gnome && has_wayland && has_x11 {
                    event_loop_builder.with_x11();
                }
            }
        }
    }

    let event_loop = event_loop_builder
        .build()
        .expect("Failed building Wayland/X11 event loop");
    let proxy = event_loop.create_proxy();

    let launch_opts = LaunchOptions {
        config_path: cli.config_path,
        command: cli.exec_cmd,
        working_directory: cli.working_dir,
        editor_file: cli.editor_file,
        initial_editor_mode: cli.initial_editor_mode,
    };

    let mut app = OceanApp::new(config, launch_opts, proxy);
    if let Err(err) = event_loop.run_app(&mut app) {
        let message = err.to_string();
        if !is_benign_shutdown_error(&message) {
            eprintln!("ocean runtime error: {message}");
            std::process::exit(1);
        }
    }
}

fn is_benign_shutdown_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("glxbadwindow")
        || lower.contains("glxbaddrawable")
        || lower.contains("badwindow (code:")
}

fn run_plus_action(act: &str, config: &Config, force: bool, args: &[String]) {
    match act {
        "+editor-window" | "+new-editor-window" => {
            let Ok(exe) = std::env::current_exe() else {
                eprintln!("Could not resolve the Ocean executable.");
                std::process::exit(1);
            };
            let mut cmd = Command::new(exe);
            if let Some(path) = &config.loaded_path {
                cmd.arg("--config-file").arg(path);
            }
            cmd.arg(format!("--theme={}", config.theme_name));
            cmd.arg("--editor-window");
            if let Some(path) = args.first() {
                cmd.arg(path);
            }
            if let Err(err) = cmd.spawn() {
                eprintln!("Failed opening a separate editor window: {err}");
                std::process::exit(1);
            }
        }
        "+list-themes" => {
            println!(
                "Built-in themes — 5 Dark + 5 Light (use `theme = <name>` in config or Right-Click -> Switch Theme):"
            );
            for info in ThemePalette::curated_themes() {
                let marker = if info.id == config.theme_name {
                    "*"
                } else {
                    " "
                };
                let mode = if info.is_dark { "dark " } else { "light" };
                println!("  {marker} {:<20} ({mode}) — {}", info.id, info.label);
            }
        }
        "+list-fonts" => {
            let font_sys = FontSystem::new(config, 1.0);
            let (mono, myanmar) = font_sys.list_available_families();
            println!(
                "Active primary monospace font : {}",
                font_sys.primary_font_name()
            );
            println!(
                "Active Myanmar/Burmese font   : {}",
                font_sys.myanmar_font_name()
            );
            println!("\nDiscovered Myanmar/Burmese fonts ({}):", myanmar.len());
            for f in &myanmar {
                println!("  - {f}");
            }
            println!("\nDiscovered Monospace fonts ({}):", mono.len());
            for f in &mono {
                println!("  - {f}");
            }
        }
        "+show-config" => {
            print!("{}", config.format_show_config());
        }
        "+validate-config" => {
            if config.warnings.is_empty() {
                match &config.loaded_path {
                    Some(p) => println!("OK: `{}` is valid.", p.display()),
                    None => println!("OK: no config file present; built-in defaults active."),
                }
            } else {
                for w in &config.warnings {
                    eprintln!("WARNING: {w}");
                }
                std::process::exit(1);
            }
        }
        "+init-config" => {
            let Some(path) = Config::default_config_path() else {
                eprintln!("Could not determine XDG config directory.");
                std::process::exit(1);
            };
            if path.exists() && !force {
                eprintln!(
                    "Config already exists at `{}` (pass `--force` to overwrite).",
                    path.display()
                );
                std::process::exit(1);
            }
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(err) = fs::write(&path, STARTER_CONFIG) {
                eprintln!("Failed writing {}: {err}", path.display());
                std::process::exit(1);
            }
            println!("Created Ghostty-style config at `{}`", path.display());
        }
        "+edit-config" => {
            let Some(path) = Config::default_config_path() else {
                eprintln!("Could not determine XDG config directory.");
                std::process::exit(1);
            };
            if !path.exists() {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Err(err) = fs::write(&path, STARTER_CONFIG) {
                    eprintln!("Failed writing {}: {err}", path.display());
                    std::process::exit(1);
                }
            }
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
            let status = std::process::Command::new(&editor).arg(&path).status();
            match status {
                Ok(s) => std::process::exit(s.code().unwrap_or(0)),
                Err(err) => {
                    eprintln!("Failed launching {editor}: {err}");
                    std::process::exit(1);
                }
            }
        }
        "+myanmar-check" => {
            let mut font_sys = FontSystem::new(config, 1.0);
            let mm_id = font_sys.resolve_font_for_char('က', FontStyle::Regular);
            let dotted_gid = font_sys.font(mm_id).glyph_id('\u{25CC}');
            println!("Ocean Myanmar/Burmese Shaping Diagnostic");
            println!("  Primary font : {}", font_sys.primary_font_name());
            println!("  Myanmar font : {}", font_sys.myanmar_font_name());
            println!(
                "  Cell metrics : {}x{} px (baseline={} px, mm_px={:.1})",
                font_sys.metrics.cell_width,
                font_sys.metrics.cell_height,
                font_sys.metrics.baseline,
                font_sys.metrics.myanmar_font_px
            );

            let mut shaper = TextShaper::new();
            let tests = [
                ("Greeting (Kinzi + Tall AA)", "မင်္ဂလာပါ"),
                ("Official Country Name", "ပြည်ထောင်စုသမ္မတမြန်မာနိုင်ငံတော်"),
                ("Pre-base E + Medial Ra+Wa", "ကြွေးမြီ"),
                ("Stacked Pali Consonants", "ဗုဒ္ဓဘာသာ နှင့် ဣန္ဒြေ"),
                ("Upper+Lower Marks", "ရန်ကုန်မြို့ နှင့် မန္တလေးမြို့"),
                ("Myanmar Digits", "၀၁၂၃၄၅၆၇၈၉"),
            ];
            for (label, sample) in tests {
                let snap = snapshot_from_bytes(sample.as_bytes(), config, 60, 2);
                let inputs: Vec<CellInput<'_>> = snap.cells[0..60]
                    .iter()
                    .map(|rc| CellInput {
                        c: rc.c,
                        zerowidth: &rc.zerowidth,
                        style: FontStyle::Regular,
                        is_wide_spacer: rc.wide_spacer,
                        is_box_drawing: false,
                    })
                    .collect();
                let mut out = Vec::new();
                shaper.shape_row(&mut font_sys, &inputs, config.myanmar_zawgyi, &mut out);
                let has_dotted = dotted_gid != 0 && out.iter().any(|g| g.glyph_id == dotted_gid);
                println!(
                    "  [{}] {:<28} -> {:2} shaped glyphs (dotted_circle={})",
                    if !has_dotted && !out.is_empty() {
                        "PASS"
                    } else {
                        "FAIL"
                    },
                    label,
                    out.len(),
                    has_dotted
                );
            }
        }
        "+check-update" => {
            println!(
                "Checking GitHub Releases for ocean updates (https://api.github.com/repos/moekyawsoe/ocean/releases/latest)..."
            );
            let cur = crate::update::running_version();
            match crate::update::check_for_update_online(&cur) {
                Ok(Some(info)) => {
                    crate::update::save_cached_update(Some(&info));
                    println!("\n  Update Available: {cur} -> {}", info.tag_name);
                    println!("  Release Name    : {}", info.release_name);
                    println!("  Published At    : {}", info.published_at);
                    println!("  Release Page    : {}", info.html_url);
                    if let Some(deb) = &info.deb_url {
                        println!("  Debian (.deb)   : {deb}");
                    }
                    if let Some(rpm) = &info.rpm_url {
                        println!("  Fedora (.rpm)   : {rpm}");
                    }
                    if let Some(tar) = &info.tar_url {
                        println!("  Tarball (.tar)  : {tar}");
                    }
                    println!("\nTo install this update directly, run:\n  ocean +update");
                }
                Ok(None) => {
                    crate::update::save_cached_update(None);
                    println!("\nOcean is up to date (current version: {cur}).");
                }
                Err(err) => {
                    eprintln!("\nFailed to check for updates: {err}");
                    std::process::exit(1);
                }
            }
        }
        "+update" => {
            println!("Checking GitHub Releases for latest ocean package...");
            let cur = crate::update::running_version();
            match crate::update::check_for_update_online(&cur) {
                Ok(Some(info)) => {
                    println!("Found newer release: {}", info.tag_name);
                    let (prog, args) = crate::update::generate_update_command(&info);
                    let status = std::process::Command::new(&prog).args(&args).status();
                    match status {
                        Ok(s) => std::process::exit(s.code().unwrap_or(0)),
                        Err(e) => {
                            eprintln!("Failed executing updater: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                Ok(None) => {
                    println!("Ocean is already at the latest version ({cur}). Nothing to update.");
                }
                Err(err) => {
                    eprintln!("Update check failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        "+plugins-dir" => match crate::plugins::plugins_dir() {
            Some(path) => println!("{}", path.display()),
            None => {
                eprintln!("Could not resolve the Ocean plugins directory.");
                std::process::exit(1);
            }
        },
        "+plugin-list" => {
            let plugins = crate::plugins::list();
            if plugins.is_empty() {
                println!("No plugins installed.");
            } else {
                for plugin in plugins {
                    let version = if plugin.version.is_empty() {
                        "0.0.0"
                    } else {
                        plugin.version.as_str()
                    };
                    println!(
                        "{}  v{}  [{}]  {}",
                        plugin.name,
                        version,
                        if plugin.enabled { "on" } else { "off" },
                        plugin.path.display()
                    );
                    if !plugin.description.is_empty() {
                        println!("    {}", plugin.description);
                    }
                }
            }
        }
        "+plugin-install" => {
            let Some(source) = args.first() else {
                eprintln!("Usage: ocean +plugin-install <file.wasm|https://...> [--force]");
                std::process::exit(1);
            };
            let result = if source.starts_with("http://") || source.starts_with("https://") {
                crate::plugins::install_url(source, force)
            } else {
                crate::plugins::install_local(Path::new(source), force, None)
            };
            match result {
                Ok(plugin) => println!(
                    "Installed {} v{} (sha256 {})",
                    plugin.name, plugin.version, plugin.sha256
                ),
                Err(err) => {
                    eprintln!("Plugin install failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        "+plugin-remove" => {
            let Some(id) = args.first() else {
                eprintln!("Usage: ocean +plugin-remove <id> [--force]");
                std::process::exit(1);
            };
            if let Err(err) = crate::plugins::remove(id, force) {
                eprintln!("Plugin remove failed: {err}");
                std::process::exit(1);
            }
            println!("Removed plugin `{id}`.");
        }
        "+plugin-enable" | "+plugin-disable" => {
            let Some(id) = args.first() else {
                eprintln!("Usage: ocean +plugin-enable|+plugin-disable <id>");
                std::process::exit(1);
            };
            let enabled = act == "+plugin-enable";
            if let Err(err) = crate::plugins::set_enabled(id, enabled) {
                eprintln!("Plugin update failed: {err}");
                std::process::exit(1);
            }
            println!(
                "Plugin `{id}` {}.",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        "+plugin-info" => {
            let Some(id) = args.first() else {
                eprintln!("Usage: ocean +plugin-info <id>");
                std::process::exit(1);
            };
            match crate::plugins::load_manifest(id) {
                Some(manifest) => {
                    println!("id          = {}", manifest.id);
                    println!("name        = {}", manifest.name);
                    println!("version     = {}", manifest.version);
                    println!("description = {}", manifest.description);
                    println!("entry       = {}", manifest.entry);
                    println!("sha256      = {}", manifest.sha256);
                    println!("source      = {}", manifest.source);
                }
                None => {
                    eprintln!("No manifest found for plugin `{id}`.");
                    std::process::exit(1);
                }
            }
        }
        "+plugin-verify" => {
            let Some(id) = args.first() else {
                eprintln!("Usage: ocean +plugin-verify <id>");
                std::process::exit(1);
            };
            match crate::plugins::verify(id) {
                Ok(true) => println!("Plugin `{id}` checksum OK."),
                Ok(false) => {
                    eprintln!("Plugin `{id}` checksum MISMATCH.");
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("Plugin verify failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        "+plugin-update" => {
            let Some(id) = args.first() else {
                eprintln!("Usage: ocean +plugin-update <id>");
                std::process::exit(1);
            };
            let Some(manifest) = crate::plugins::load_manifest(id) else {
                eprintln!("No manifest found for plugin `{id}`.");
                std::process::exit(1);
            };
            let result = if manifest.source.starts_with("http://")
                || manifest.source.starts_with("https://")
            {
                crate::plugins::install_url(&manifest.source, true)
            } else if !manifest.source.is_empty() {
                crate::plugins::install_local(Path::new(&manifest.source), true, None)
            } else {
                Err("Plugin has no recorded install source".to_string())
            };
            match result {
                Ok(plugin) => println!("Updated {} v{}.", plugin.name, plugin.version),
                Err(err) => {
                    eprintln!("Plugin update failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        other => {
            eprintln!("Unknown action `{other}`. Run `ocean --help` for usage.");
            std::process::exit(1);
        }
    }
}

fn run_screenshot(
    out_path: &Path,
    config: &Config,
    exec_cmd: Option<&(String, Vec<String>)>,
    preset: Option<&str>,
) -> Result<(), String> {
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = fs::create_dir_all(parent);
    }

    let mut font_sys = FontSystem::new(config, 1.0);
    let cols = config.window_width as usize;
    let rows = config.window_height as usize;

    if preset == Some("splits") {
        let tabs = [
            renderer::TabTitle {
                title: "ocean — ~/BDATA/ocean",
                active: true,
            },
            renderer::TabTitle {
                title: "nvim ~/.config/ocean/config",
                active: false,
            },
        ];
        let tab_h = RenderEngine::tab_bar_height(config, &font_sys, tabs.len());
        let status_h = RenderEngine::status_bar_height(config, &font_sys, 1.0);
        let win_w = (cols as f32 * font_sys.metrics.cell_width
            + (config.window_padding_x.0 + config.window_padding_x.1) as f32)
            .round() as u32;
        let win_h = (rows as f32 * font_sys.metrics.cell_height
            + (config.window_padding_y.0 + config.window_padding_y.1) as f32
            + tab_h
            + status_h)
            .round() as u32;

        let root = split::SplitNode::Split {
            axis: split::SplitAxis::Horizontal,
            ratio: 0.54,
            first: Box::new(split::SplitNode::Leaf(1)),
            second: Box::new(split::SplitNode::Leaf(2)),
        };
        let (layouts, dividers) = split::compute_split_layout(
            &root, config, &font_sys, win_w, win_h, 1.0, tab_h, status_h,
        );
        let l1 = layouts[0];
        let l2 = layouts[1];

        let left_ansi = concat!(
            "\x1b[1;32mocean@linux\x1b[0m:\x1b[1;34m~/BDATA/ocean\x1b[0m$ cargo build --release\r\n",
            "   \x1b[1;32mCompiling\x1b[0m ocean v1.0.0 (/home/mks/BDATA/ocean)\r\n",
            "    \x1b[1;32mFinished\x1b[0m `release` profile [optimized] in 2.41s\r\n\r\n",
            "\x1b[1;32mocean@linux\x1b[0m:\x1b[1;34m~/BDATA/ocean\x1b[0m$ ocean +myanmar-check\r\n",
            "\x1b[1;36mOcean Myanmar/Burmese Shaping Diagnostic\x1b[0m\r\n",
            "  Primary font : Cascadia Code NF\r\n",
            "  Myanmar font : Padauk\r\n",
            "  [\x1b[1;32mPASS\x1b[0m] Greeting (Kinzi + Tall AA) -> မင်္ဂလာပါ\r\n",
            "  [\x1b[1;32mPASS\x1b[0m] Stacked Pali Consonants    -> ဗုဒ္ဓဘာသာ နှင့် ဣန္ဒြေ\r\n",
            "  [\x1b[1;32mPASS\x1b[0m] Upper+Lower Marks          -> ရန်ကုန်မြို့ • မန္တလေးမြို့\r\n",
            "  [\x1b[1;32mPASS\x1b[0m] Myanmar Digits             -> ၀၁၂၃၄၅၆၇၈၉\r\n\r\n",
            "\x1b[1;32mocean@linux\x1b[0m:\x1b[1;34m~/BDATA/ocean\x1b[0m$ git tag v1.0.0\r\n",
            "\x1b[1;32mocean@linux\x1b[0m:\x1b[1;34m~/BDATA/ocean\x1b[0m$ "
        );
        let right_ansi = concat!(
            "\x1b[1;35m╭─ Active Split Pane (Alt+Shift+H/L) ───────╮\x1b[0m\r\n",
            "\x1b[1;35m│\x1b[0m  • Keyboard Splits : \x1b[36mCtrl+Shift+D / E\x1b[0m     \x1b[1;35m│\x1b[0m\r\n",
            "\x1b[1;35m│\x1b[0m  • Context Menu    : \x1b[36mCtrl+Shift+P\x1b[0m         \x1b[1;35m│\x1b[0m\r\n",
            "\x1b[1;35m│\x1b[0m  • Reload Config   : \x1b[36mCtrl+Shift+,\x1b[0m         \x1b[1;35m│\x1b[0m\r\n",
            "\x1b[1;35m╰───────────────────────────────────────────╯\x1b[0m\r\n\r\n",
            "\x1b[1;34m$\x1b[0m ocean +list-themes\r\n",
            "  * tokyo-night        (dark)  — Tokyo Night\r\n",
            "    ocean-dark         (dark)  — Ocean Dark\r\n",
            "    catppuccin-mocha   (dark)  — Catppuccin\r\n",
            "    gruvbox-dark       (dark)  — Gruvbox\r\n",
            "    nord               (dark)  — Nord\r\n",
            "    ocean-light        (light) — Ocean Light\r\n"
        );

        let snap1 = snapshot_from_bytes(
            left_ansi.as_bytes(),
            config,
            l1.cols as usize,
            l1.rows as usize,
        );
        let snap2 = snapshot_from_bytes(
            right_ansi.as_bytes(),
            config,
            l2.cols as usize,
            l2.rows as usize,
        );

        let panes = [
            renderer::PaneScene {
                snapshot: &snap1,
                layout: l1,
                is_active: true,
                ime_preedit: None,
            },
            renderer::PaneScene {
                snapshot: &snap2,
                layout: l2,
                is_active: false,
                ime_preedit: None,
            },
        ];

        let mut ctx_menu = menu::ContextMenu::build(
            (win_w as f32 * 0.34).round(),
            (win_h as f32 * 0.24).round(),
            false,
            2,
            config.window_show_status_bar,
            &config.keybinds,
            None,
        );
        if let Some(theme_idx) = ctx_menu
            .entries
            .iter()
            .position(|e| matches!(e, menu::MenuEntry::ThemeSubmenu { .. }))
        {
            ctx_menu.hovered = Some(theme_idx);
            ctx_menu.submenu_open = true;
            ctx_menu.sub_hovered = Some(2);
        }

        let mut engine = RenderEngine::new();
        let (atlas, instances) = engine.build_scene(
            config,
            &mut font_sys,
            &panes,
            &dividers,
            win_w,
            win_h,
            1.0,
            &tabs,
            Some(&ctx_menu),
            None,
            None,
            None,
            true,
            true,
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            0.0,
            None,
        );

        return save_scene_png(
            out_path,
            atlas,
            instances,
            win_w,
            win_h,
            config.colors.background,
        );
    }

    let payload: Vec<u8> = if let Some((prog, args)) = exec_cmd {
        let out = Command::new(prog)
            .args(args)
            .output()
            .map_err(|e| format!("Failed running `{prog}`: {e}"))?;
        let mut combined = Vec::new();
        for &b in out.stdout.iter().chain(out.stderr.iter()) {
            if b == b'\n' {
                combined.push(b'\r');
            }
            combined.push(b);
        }
        combined
    } else {
        default_showcase_ansi().into_bytes()
    };

    let snap = snapshot_from_bytes(&payload, config, cols, rows);
    let status_h = RenderEngine::status_bar_height(config, &font_sys, 1.0);
    let win_w = (cols as f32 * font_sys.metrics.cell_width
        + (config.window_padding_x.0 + config.window_padding_x.1) as f32)
        .round() as u32;
    let win_h = (rows as f32 * font_sys.metrics.cell_height
        + (config.window_padding_y.0 + config.window_padding_y.1) as f32
        + status_h)
        .round() as u32;

    let (ox, oy, _, _) =
        RenderEngine::compute_grid_geometry(config, &font_sys, win_w, win_h, 1.0, 0);
    let pane_scene = [renderer::PaneScene {
        snapshot: &snap,
        layout: split::PaneLayout {
            pane_id: 1,
            bounds_x: 0.0,
            bounds_y: 0.0,
            bounds_w: win_w as f32,
            bounds_h: (win_h as f32 - status_h).max(1.0),
            origin_x: ox,
            origin_y: oy,
            cols: cols as u16,
            rows: rows as u16,
        },
        is_active: true,
        ime_preedit: None,
    }];

    let mut engine = RenderEngine::new();
    let (atlas, instances) = engine.build_scene(
        config,
        &mut font_sys,
        &pane_scene,
        &[],
        win_w,
        win_h,
        1.0,
        &[],
        None,
        None,
        None,
        None,
        true,
        true,
        None,
        None,
        None,
        None,
        false,
        None,
        None,
        0.0,
        None,
    );

    save_scene_png(
        out_path,
        atlas,
        instances,
        win_w,
        win_h,
        config.colors.background,
    )
}

fn default_showcase_ansi() -> String {
    let pad_col = |s: &str, target_cols: usize| -> String {
        let used: usize = s
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        let pad = target_cols.saturating_sub(used);
        format!("{s}{}", " ".repeat(pad))
    };

    let h1 = pad_col("မြို့အမည် (City)", 20);
    let h2 = pad_col("တိုင်းဒေသကြီး / ပြည်နယ်", 28);
    let h3 = pad_col("အခြေအနေ", 14);

    let r1c1 = pad_col("ရန်ကုန်မြို့", 20);
    let r1c2 = pad_col("ရန်ကုန်တိုင်းဒေသကြီး", 28);
    let r1c3 = pad_col("● ကောင်းမွန်", 14);

    let r2c1 = pad_col("မန္တလေးမြို့", 20);
    let r2c2 = pad_col("မန္တလေးတိုင်းဒေသကြီး", 28);
    let r2c3 = pad_col("● ကောင်းမွန်", 14);

    let r3c1 = pad_col("တောင်ကြီးမြို့", 20);
    let r3c2 = pad_col("ရှမ်းပြည်နယ်", 28);
    let r3c3 = pad_col("● အေးချမ်း", 14);

    format!(
        concat!(
            "\x1b[1;36m╭──────────────────────────────────────────────────────────────────────────────╮\x1b[0m\r\n",
            "\x1b[1;36m│\x1b[0m  \x1b[1;97mOCEAN TERMINAL\x1b[0m • \x1b[32mFast Rust + GPU\x1b[0m • \x1b[33mFull Myanmar/Burmese\x1b[0m • \x1b[35mGhostty Config\x1b[0m    \x1b[1;36m│\x1b[0m\r\n",
            "\x1b[1;36m╰──────────────────────────────────────────────────────────────────────────────╯\x1b[0m\r\n\r\n",
            "  \x1b[1;34m$\x1b[0m cat greeting_myanmar.txt\r\n",
            "  \x1b[1;92mမင်္ဂလာပါ — မြန်မာဘာသာစကားနှင့် စာပေကို အပြည့်အဝ ပံ့ပိုးပေးထားပါသည်။\x1b[0m\r\n",
            "  ပြည်ထောင်စုသမ္မတမြန်မာနိုင်ငံတော် • ရန်ကုန်မြို့ • မန္တလေးမြို့ • ပုဂံရှေးဟောင်းယဉ်ကျေးမှုနယ်မြေ\r\n",
            "  သဒ္ဒါနှင့် စာလုံးဆင့်များ : \x1b[93mကြွေးမြီ  ဗုဒ္ဓဘာသာ  ဣန္ဒြေ  တက္ကသိုလ်  သမ္မတ  ဉာဏ်ပညာ  ဦးနှောက်  ၎င်း\x1b[0m\r\n",
            "  မြန်မာဂဏန်းများ       : \x1b[96m၀  ၁  ၂  ၃  ၄  ၅  ၆  ၇  ၈  ၉\x1b[0m   (၂၀၂၆ ခုနှစ်၊ နိုဝင်ဘာလ)\r\n\r\n",
            "  \x1b[1;34m$\x1b[0m ocean +show-config\r\n",
            "  \x1b[90m# ~/.config/ocean/config (Ghostty syntax)\x1b[0m\r\n",
            "  \x1b[36mfont-family\x1b[0m         = \x1b[32mCascadia Code NF\x1b[0m\r\n",
            "  \x1b[36mfont-family-myanmar\x1b[0m = \x1b[32mPadauk\x1b[0m\r\n",
            "  \x1b[36mtheme\x1b[0m               = \x1b[33mocean-dark\x1b[0m\r\n",
            "  \x1b[36mcursor-style\x1b[0m        = \x1b[35mblock\x1b[0m\r\n\r\n",
            "  \x1b[1;34m╭──────────────────────┬──────────────────────────────┬────────────────╮\x1b[0m\r\n",
            "  \x1b[1;34m│\x1b[0m \x1b[1m{h1}\x1b[0m \x1b[1;34m│\x1b[0m \x1b[1m{h2}\x1b[0m \x1b[1;34m│\x1b[0m \x1b[1m{h3}\x1b[0m \x1b[1;34m│\x1b[0m\r\n",
            "  \x1b[1;34m├──────────────────────┼──────────────────────────────┼────────────────┤\x1b[0m\r\n",
            "  \x1b[1;34m│\x1b[0m {r1c1} \x1b[1;34m│\x1b[0m {r1c2} \x1b[1;34m│\x1b[0m \x1b[32m{r1c3}\x1b[0m \x1b[1;34m│\x1b[0m\r\n",
            "  \x1b[1;34m│\x1b[0m {r2c1} \x1b[1;34m│\x1b[0m {r2c2} \x1b[1;34m│\x1b[0m \x1b[32m{r2c3}\x1b[0m \x1b[1;34m│\x1b[0m\r\n",
            "  \x1b[1;34m│\x1b[0m {r3c1} \x1b[1;34m│\x1b[0m {r3c2} \x1b[1;34m│\x1b[0m \x1b[36m{r3c3}\x1b[0m \x1b[1;34m│\x1b[0m\r\n",
            "  \x1b[1;34m╰──────────────────────┴──────────────────────────────┴────────────────╯\x1b[0m\r\n\r\n",
            "  Palette: \x1b[40m   \x1b[41m   \x1b[42m   \x1b[43m   \x1b[44m   \x1b[45m   \x1b[46m   \x1b[47m   \x1b[0m ",
            "\x1b[100m   \x1b[101m   \x1b[102m   \x1b[103m   \x1b[104m   \x1b[105m   \x1b[106m   \x1b[107m   \x1b[0m\r\n",
            "  \x1b[1;32mocean@linux\x1b[0m:\x1b[1;34m~/BDATA/ocean\x1b[0m$ "
        ),
        h1 = h1,
        h2 = h2,
        h3 = h3,
        r1c1 = r1c1,
        r1c2 = r1c2,
        r1c3 = r1c3,
        r2c1 = r2c1,
        r2c2 = r2c2,
        r2c3 = r2c3,
        r3c1 = r3c1,
        r3c2 = r3c2,
        r3c3 = r3c3,
    )
}

fn print_help() {
    println!(
        "\
ocean {} — Clean, lightweight GPU terminal and built-in code editor with full Myanmar/Burmese support

USAGE:
    ocean [OPTIONS] [-e <COMMAND> [ARGS...]]
    ocean edit [PATH]
    ocean edit --here [PATH]
    edit [PATH]
    ocean +<ACTION>

COMMANDS:
    edit [PATH]                    Open the built-in code editor in a separate window
    edit --here [PATH]             Open the editor in the current window (IPC when inside Ocean)
    edit --window [PATH]           Explicitly open the editor in a separate window

OPTIONS:
    -c, --config-file <PATH>       Load Ghostty-style config from PATH
        --working-directory <DIR>  Set initial shell working directory
        --backend <auto|wayland|x11> Select display server backend (default: auto)
        --x11                      Force X11 / XWayland backend
        --wayland                  Force native Wayland backend
        --<KEY>=<VALUE>            Override any Ghostty config key (e.g. --theme=tokyo-night)
    -e, --command <CMD> [ARGS...]  Execute command instead of default login shell
        --screenshot <OUT.PNG>     Render terminal output to a PNG image and exit
    -h, --help                     Print help
    -v, --version                  Print version

ACTIONS (Ghostty-style):
    +init-config                   Create starter config at ~/.config/ocean/config
    +edit-config                   Open ~/.config/ocean/config in nano to view/edit
    +show-config                   Display active resolved configuration
    +validate-config               Check config file syntax for errors/warnings
    +list-fonts                    List discovered Monospace and Myanmar/Burmese fonts
    +list-themes                   List built-in color themes
    +myanmar-check                 Verify OpenType Myanmar shaping on system fonts
    +editor-window [PATH]          Open the built-in editor in a separate window
    +plugins-dir                   Print the installed plugin directory
    +plugin-list                   List installed WASM plugins and enabled state
    +plugin-install <path|url>     Install a .wasm plugin from disk or HTTPS URL (--force)
    +plugin-remove <id>            Remove an installed plugin (--force if enabled)
    +plugin-enable <id>            Enable a plugin
    +plugin-disable <id>           Disable a plugin without removing it
    +plugin-info <id>              Show the plugin manifest
    +plugin-verify <id>            Verify the plugin checksum
    +plugin-update <id>            Reinstall from the recorded source
    +check-update                  Check online if a newer release of Ocean is available
    +update                        Download and upgrade Ocean to latest tagged release",
        crate::update::running_version()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_binary_no_args_opens_cwd() {
        let cli = parse_cli_args("edit", &[]);
        assert!(cli.initial_editor_mode);
        assert!(cli.new_editor_window);
        assert!(cli.editor_file.is_some());
        let cur = std::env::current_dir().unwrap();
        assert_eq!(cli.editor_file.unwrap(), cur);
    }

    #[test]
    fn test_edit_binary_with_folder_arg() {
        let cli = parse_cli_args("edit", &[String::from(".")]);
        assert!(cli.initial_editor_mode);
        assert_eq!(cli.editor_file, Some(PathBuf::from(".")));
    }

    #[test]
    fn test_edit_binary_with_file_arg() {
        let cli = parse_cli_args("/home/mks/.local/bin/edit", &[String::from("src/main.rs")]);
        assert!(cli.initial_editor_mode);
        assert_eq!(cli.editor_file, Some(PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_ocean_edit_subcommand_no_args() {
        let cli = parse_cli_args("ocean", &[String::from("edit")]);
        assert!(cli.initial_editor_mode);
        assert!(cli.new_editor_window);
        assert!(cli.editor_file.is_some());
        let cur = std::env::current_dir().unwrap();
        assert_eq!(cli.editor_file.unwrap(), cur);
    }

    #[test]
    fn test_ocean_edit_subcommand_with_path() {
        let cli = parse_cli_args("ocean", &[String::from("edit"), String::from("Cargo.toml")]);
        assert!(cli.initial_editor_mode);
        assert!(cli.new_editor_window);
        assert_eq!(cli.editor_file, Some(PathBuf::from("Cargo.toml")));
    }

    #[test]
    fn test_ocean_edit_here_flag() {
        let cli = parse_cli_args(
            "ocean",
            &[
                String::from("edit"),
                String::from("--here"),
                String::from("Cargo.toml"),
            ],
        );
        assert!(cli.initial_editor_mode);
        assert!(!cli.new_editor_window);
        assert_eq!(cli.editor_file, Some(PathBuf::from("Cargo.toml")));
    }

    #[test]
    fn test_ocean_edit_window_flag() {
        let cli = parse_cli_args(
            "ocean",
            &[
                String::from("edit"),
                String::from("--window"),
                String::from("src/main.rs"),
            ],
        );
        assert!(cli.initial_editor_mode);
        assert!(cli.new_editor_window);
        assert_eq!(cli.editor_file, Some(PathBuf::from("src/main.rs")));

        let cli = parse_cli_args(
            "ocean",
            &[String::from("--editor-window"), String::from("Cargo.toml")],
        );
        assert!(cli.initial_editor_mode);
        assert!(cli.new_editor_window);
        assert_eq!(cli.editor_file, Some(PathBuf::from("Cargo.toml")));
    }

    #[test]
    fn benign_x11_shutdown_errors_are_not_reported_as_fatal() {
        assert!(is_benign_shutdown_error(
            "Xlib error: X error: GLXBadWindow (code: 168, request code: 148, minor code: 32)"
        ));
        assert!(is_benign_shutdown_error(
            "X error: BadWindow (code: 3, request code: 12)"
        ));
        assert!(!is_benign_shutdown_error("Failed creating GL 3.3 context"));
    }

    #[test]
    fn test_backend_cli_flags() {
        let cli_x11 = parse_cli_args("ocean", &[String::from("--x11")]);
        assert_eq!(cli_x11.backend.as_deref(), Some("x11"));

        let cli_wl = parse_cli_args("ocean", &[String::from("--wayland")]);
        assert_eq!(cli_wl.backend.as_deref(), Some("wayland"));

        let cli_be = parse_cli_args("ocean", &[String::from("--backend=x11")]);
        assert_eq!(cli_be.backend.as_deref(), Some("x11"));

        let cli_be_sep =
            parse_cli_args("ocean", &[String::from("--backend"), String::from("auto")]);
        assert_eq!(cli_be_sep.backend.as_deref(), Some("auto"));
    }
}
