mod app;
mod config;
mod font;
mod help;
mod menu;
mod renderer;
mod split;
mod statusbar;
mod terminal;

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

const STARTER_CONFIG: &str = include_str!("../config.example");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut config_path: Option<PathBuf> = None;
    let mut working_dir: Option<PathBuf> = None;
    let mut exec_cmd: Option<(String, Vec<String>)> = None;
    let mut screenshot_out: Option<PathBuf> = None;
    let mut plus_action: Option<String> = None;
    let mut force_flag = false;
    let mut cli_overrides: Vec<(String, String)> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" | "+help" => {
                print_help();
                return;
            }
            "-v" | "--version" | "+version" => {
                println!("ocean {}", env!("CARGO_PKG_VERSION"));
                return;
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
            "--force" => {
                force_flag = true;
            }
            s if s.starts_with('+') => {
                plus_action = Some(s.to_string());
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
                    } else {
                        cli_overrides.push((k.to_string(), v.to_string()));
                    }
                } else if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    cli_overrides.push((kv.to_string(), args[i + 1].clone()));
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut config = Config::load(config_path.as_deref());
    for (k, v) in &cli_overrides {
        config.apply_cli_kv(k, v);
    }

    if let Some(act) = plus_action {
        run_plus_action(&act, &config, force_flag);
        return;
    }

    for warn in &config.warnings {
        eprintln!("[ocean config warning] {warn}");
    }

    if let Some(out_png) = screenshot_out {
        if let Err(err) = run_screenshot(&out_png, &config, exec_cmd.as_ref()) {
            eprintln!("ocean screenshot failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    let event_loop = EventLoop::<terminal::AppEvent>::with_user_event()
        .build()
        .expect("Failed building Wayland/X11 event loop");
    let proxy = event_loop.create_proxy();

    let launch_opts = LaunchOptions {
        config_path,
        command: exec_cmd,
        working_directory: working_dir,
    };

    let mut app = OceanApp::new(config, launch_opts, proxy);
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("ocean runtime error: {err}");
        std::process::exit(1);
    }
}

fn run_plus_action(act: &str, config: &Config, force: bool) {
    match act {
        "+list-themes" => {
            println!(
                "Built-in themes — 5 Dark + 5 Light (use `theme = <name>` in config or Right-Click -> Switch Theme):"
            );
            for info in ThemePalette::curated_themes() {
                let marker = if info.id == config.theme_name { "*" } else { " " };
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
) -> Result<(), String> {
    let mut font_sys = FontSystem::new(config, 1.0);
    let cols = config.window_width as usize;
    let rows = config.window_height as usize;

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
        true,
        true,
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
ocean {} — Clean, lightweight GPU terminal with full Myanmar/Burmese support

USAGE:
    ocean [OPTIONS] [-e <COMMAND> [ARGS...]]
    ocean +<ACTION>

OPTIONS:
    -c, --config-file <PATH>       Load Ghostty-style config from PATH
        --working-directory <DIR>  Set initial shell working directory
        --<KEY>=<VALUE>            Override any Ghostty config key (e.g. --theme=tokyo-night)
    -e, --command <CMD> [ARGS...]  Execute command instead of default login shell
        --screenshot <OUT.PNG>     Render terminal output to a PNG image and exit
    -h, --help                     Print help
    -v, --version                  Print version

ACTIONS (Ghostty-style):
    +init-config                   Create starter config at ~/.config/ocean/config
    +show-config                   Display active resolved configuration
    +validate-config               Check config file syntax for errors/warnings
    +list-fonts                    List discovered Monospace and Myanmar/Burmese fonts
    +list-themes                   List built-in color themes
    +myanmar-check                 Verify OpenType Myanmar shaping on system fonts",
        env!("CARGO_PKG_VERSION")
    );
}
