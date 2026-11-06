pub mod markdown;
pub mod pager;
pub mod pdf;
pub mod player;
pub mod text;

use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum FileType {
    Pdf,
    Markdown,
    Audio,
    Video,
    YouTube(String),
    Directory,
    Other,
}

#[derive(Debug, Default)]
pub struct FileCliOptions {
    pub target: Option<String>,
    pub here: bool,
    pub split: bool,
    pub split_direction: Option<String>,
    pub print_help: bool,
}

pub fn parse_file_args(args: &[String]) -> FileCliOptions {
    let mut opts = FileCliOptions::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" | "help" => {
                opts.print_help = true;
            }
            "--here" | "--in-place" | "--current-pane" => {
                opts.here = true;
            }
            "--split" | "-s" => {
                opts.split = true;
            }
            "--right" | "--horizontal" => {
                opts.split = true;
                opts.split_direction = Some("right".to_string());
            }
            "--down" | "--bottom" | "--vertical" => {
                opts.split = true;
                opts.split_direction = Some("down".to_string());
            }
            s if !s.starts_with('-') && opts.target.is_none() => {
                opts.target = Some(s.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    opts
}

pub fn print_help() {
    println!(
        "\x1b[1;38;5;39mOcean File Tool\x1b[0m - Fast, lightweight terminal file viewer & media player\n"
    );
    println!("\x1b[1mUSAGE:\x1b[0m");
    println!("    file <path-or-url> [OPTIONS]");
    println!("    ocean file <path-or-url> [OPTIONS]\n");
    println!("\x1b[1mSUPPORTED FORMATS:\x1b[0m");
    println!(
        "    \x1b[38;5;220mPDF Documents\x1b[0m       .pdf (Auto-splits pane in Ocean for multitasking)"
    );
    println!(
        "    \x1b[38;5;45mMarkdown\x1b[0m            .md, .markdown (Dedicated read/view mode with ANSI styling)"
    );
    println!(
        "    \x1b[38;5;82mAudio & Video\x1b[0m       .mp3, .wav, .flac, .ogg, .m4a, .mp4, .mkv, .webm"
    );
    println!(
        "    \x1b[38;5;196mYouTube\x1b[0m             URLs, IDs, playlists (e.g. yt:ID, playlist URLs, etc.)"
    );
    println!(
        "    \x1b[38;5;250mSource / Text / Hex\x1b[0m Code, text files, and binary hex inspection\n"
    );
    println!("\x1b[1mOPTIONS:\x1b[0m");
    println!("    --split, -s         Open viewer in an Ocean split pane");
    println!("    --right             Split right (horizontal split)");
    println!("    --down              Split down (vertical split)");
    println!("    --here              Force viewing in the current terminal pane");
    println!("    -h, --help          Show this help message\n");
    println!("\x1b[1mEXAMPLES:\x1b[0m");
    println!("    file report.pdf                      # View PDF (splits pane in Ocean)");
    println!("    file README.md                       # View Markdown in read-only mode");
    println!("    file song.mp3                        # Play local audio with terminal controls");
    println!("    file dQw4w9WgXcQ                     # Stream YouTube song by ID");
    println!("    file \"https://youtube.com/watch?v=\"  # Stream YouTube song by URL");
    println!(
        "    file \"https://youtube.com/playlist?list=\" # Interactive YouTube playlist player"
    );
    println!("    file src/main.rs                     # View source code with line numbers");
}

pub fn detect_file_type(input: &str) -> FileType {
    // 1. Check YouTube input
    if player::parse_youtube_input(input).is_some() {
        return FileType::YouTube(input.to_string());
    }

    let path = Path::new(input);
    if path.is_dir() {
        return FileType::Directory;
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "pdf" => FileType::Pdf,
        "md" | "markdown" => FileType::Markdown,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" | "wma" => FileType::Audio,
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" => FileType::Video,
        _ => {
            // Check PDF magic bytes
            if let Ok(mut file) = std::fs::File::open(path) {
                use std::io::Read;
                let mut magic = [0u8; 5];
                if file.read_exact(&mut magic).is_ok() && &magic == b"%PDF-" {
                    return FileType::Pdf;
                }
            }
            FileType::Other
        }
    }
}

pub fn run_file_cli(args: &[String]) {
    let opts = parse_file_args(args);

    if opts.print_help || opts.target.is_none() {
        print_help();
        return;
    }

    let target = opts.target.unwrap();
    let file_type = detect_file_type(&target);

    // Multitasking Split in Ocean:
    // When viewing a PDF file, or when --split was requested:
    // If running inside Ocean shell (OCEAN_SOCKET present) and --here was not passed:
    // Split an Ocean pane and run the viewer in that split pane!
    let should_split = (opts.split || matches!(file_type, FileType::Pdf)) && !opts.here;

    if should_split {
        let abs_path = if !matches!(file_type, FileType::YouTube(_)) {
            let p = Path::new(&target);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|d| d.join(p))
                    .unwrap_or_else(|_| p.to_path_buf())
            }
        } else {
            PathBuf::from(&target)
        };

        let current_exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "ocean".to_string());

        let cmd_args = vec![
            "file".to_string(),
            "--here".to_string(),
            abs_path.to_string_lossy().to_string(),
        ];

        let direction = opts.split_direction.as_deref().unwrap_or("right");

        if crate::ipc::try_send_split_request(
            &current_exe,
            &cmd_args,
            Some(direction),
            std::env::current_dir().ok().as_deref(),
        ) {
            println!(
                "\x1b[1;38;5;39m[ocean]\x1b[0m Opened {} in split pane.",
                abs_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&target)
            );
            return;
        }
    }

    // Direct terminal execution
    let path = Path::new(&target);
    match file_type {
        FileType::Pdf => {
            if let Err(err) = pdf::view_pdf(path) {
                eprintln!("\x1b[1;38;5;196mError opening PDF:\x1b[0m {err}");
            }
        }
        FileType::Markdown => {
            if let Err(err) = markdown::view_markdown(path) {
                eprintln!("\x1b[1;38;5;196mError opening Markdown:\x1b[0m {err}");
            }
        }
        FileType::Audio => {
            if let Err(err) = player::play_local_file(path, false) {
                eprintln!("\x1b[1;38;5;196mError playing audio:\x1b[0m {err}");
            }
        }
        FileType::Video => {
            if let Err(err) = player::play_local_file(path, true) {
                eprintln!("\x1b[1;38;5;196mError playing video:\x1b[0m {err}");
            }
        }
        FileType::YouTube(yt_input) => {
            if let Err(err) = player::play_youtube(&yt_input) {
                eprintln!("\x1b[1;38;5;196mError playing YouTube:\x1b[0m {err}");
            }
        }
        FileType::Directory => {
            println!("\x1b[1;38;5;39mDirectory contents of:\x1b[0m {target}");
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let is_d = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    if is_d {
                        println!("  \x1b[1;38;5;39m📁 {}/\x1b[0m", name.to_string_lossy());
                    } else {
                        println!("  📄 {}", name.to_string_lossy());
                    }
                }
            }
        }
        FileType::Other => {
            if let Err(err) = text::view_file(path) {
                eprintln!("\x1b[1;38;5;196mError opening file:\x1b[0m {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_file_types() {
        assert_eq!(detect_file_type("document.pdf"), FileType::Pdf);
        assert_eq!(detect_file_type("README.md"), FileType::Markdown);
        assert_eq!(detect_file_type("audio.mp3"), FileType::Audio);
        assert_eq!(detect_file_type("video.mp4"), FileType::Video);
        assert!(matches!(
            detect_file_type("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            FileType::YouTube(_)
        ));
    }

    #[test]
    fn test_parse_file_cli_flags() {
        let args = vec!["--split".to_string(), "sample.pdf".to_string()];
        let opts = parse_file_args(&args);
        assert!(opts.split);
        assert_eq!(opts.target.as_deref(), Some("sample.pdf"));

        let args_here = vec!["--here".to_string(), "document.pdf".to_string()];
        let opts_here = parse_file_args(&args_here);
        assert!(opts_here.here);
        assert!(!opts_here.split);
        assert_eq!(opts_here.target.as_deref(), Some("document.pdf"));

        let args_right = vec!["--right".to_string(), "notes.md".to_string()];
        let opts_right = parse_file_args(&args_right);
        assert!(opts_right.split);
        assert_eq!(opts_right.split_direction.as_deref(), Some("right"));

        let args_down = vec!["--down".to_string(), "video.mp4".to_string()];
        let opts_down = parse_file_args(&args_down);
        assert!(opts_down.split);
        assert_eq!(opts_down.split_direction.as_deref(), Some("down"));
    }

    #[test]
    fn test_file_type_classification() {
        assert_eq!(detect_file_type("song.flac"), FileType::Audio);
        assert_eq!(detect_file_type("movie.mkv"), FileType::Video);
        assert_eq!(detect_file_type("src"), FileType::Directory);
        assert_eq!(detect_file_type("Cargo.toml"), FileType::Other);
        assert_eq!(
            detect_file_type("https://youtu.be/dQw4w9WgXcQ"),
            FileType::YouTube("https://youtu.be/dQw4w9WgXcQ".to_string())
        );
    }
}
