use std::io::{self, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use super::pager::{Key, RawTerminal};

#[derive(Debug, Clone)]
pub struct PlaylistItem {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub enum PlayerMode {
    LocalAudio(String),
    LocalVideo(String),
    YouTubeSong(PlaylistItem),
    YouTubePlaylist {
        items: Vec<PlaylistItem>,
        current_idx: usize,
    },
}

pub struct MediaPlayer {
    pub mode: PlayerMode,
    pub is_playing: bool,
    pub is_paused: bool,
    pub status_message: String,
    pub active_child: Option<Child>,
}

impl MediaPlayer {
    pub fn new(mode: PlayerMode) -> Self {
        Self {
            mode,
            is_playing: false,
            is_paused: false,
            status_message: "Ready".to_string(),
            active_child: None,
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        let mut term = RawTerminal::enter()?;
        self.start_current_track();

        loop {
            // Check if process finished
            if let Some(child) = &mut self.active_child {
                if let Ok(Some(_)) = child.try_wait() {
                    self.active_child = None;
                    self.on_track_finished();
                }
            }

            let (cols, rows) = RawTerminal::terminal_size();
            self.render(cols, rows)?;

            // Non-blocking poll for key with small sleep to keep CPU at ~0%
            if let Some(key) = term.read_key() {
                match key {
                    Key::Char('q') | Key::Char('Q') | Key::Esc | Key::Ctrl('c') => {
                        self.stop();
                        break;
                    }
                    Key::Char(' ') => {
                        self.toggle_pause();
                    }
                    Key::Char('s') | Key::Char('S') => {
                        self.stop();
                        self.status_message = "Stopped".to_string();
                    }
                    Key::Char('n') => {
                        self.next_track();
                    }
                    Key::Char('p') => {
                        self.prev_track();
                    }
                    Key::Char(c @ '1'..='9') => {
                        let idx = (c as usize) - ('1' as usize);
                        self.jump_to_track(idx);
                    }
                    _ => {}
                }
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        Ok(())
    }

    fn start_current_track(&mut self) {
        self.stop();

        enum Action {
            LocalAudio(String, String),
            LocalVideo(String, String),
            YouTube(String, String),
        }

        let action = match &self.mode {
            PlayerMode::LocalAudio(path) => {
                let name = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path)
                    .to_string();
                Action::LocalAudio(path.clone(), format!("Playing: {name}"))
            }
            PlayerMode::LocalVideo(path) => {
                let name = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path)
                    .to_string();
                Action::LocalVideo(path.clone(), format!("Playing video: {name}"))
            }
            PlayerMode::YouTubeSong(item) => {
                Action::YouTube(item.id.clone(), format!("Streaming: {}", item.title))
            }
            PlayerMode::YouTubePlaylist { items, current_idx } => {
                if let Some(item) = items.get(*current_idx) {
                    Action::YouTube(
                        item.id.clone(),
                        format!(
                            "Streaming [{} of {}]: {}",
                            current_idx + 1,
                            items.len(),
                            item.title
                        ),
                    )
                } else {
                    return;
                }
            }
        };

        match action {
            Action::LocalAudio(path, msg) => {
                self.status_message = msg;
                self.spawn_ffplay(&[path.as_str()], true);
            }
            Action::LocalVideo(path, msg) => {
                self.status_message = msg;
                self.spawn_ffplay(&[path.as_str()], false);
            }
            Action::YouTube(id, msg) => {
                self.status_message = msg;
                self.spawn_youtube_stream(&id);
            }
        }
    }

    fn spawn_ffplay(&mut self, args: &[&str], audio_only: bool) {
        let mut cmd = Command::new("ffplay");
        if audio_only {
            cmd.arg("-nodisp");
        }
        cmd.arg("-autoexit")
            .arg("-loglevel")
            .arg("quiet")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match cmd.spawn() {
            Ok(child) => {
                self.active_child = Some(child);
                self.is_playing = true;
                self.is_paused = false;
            }
            Err(e) => {
                self.status_message = format!("ffplay error: {e}. Is ffmpeg installed?");
                self.is_playing = false;
            }
        }
    }

    fn spawn_youtube_stream(&mut self, video_id_or_url: &str) {
        let url =
            if video_id_or_url.starts_with("http://") || video_id_or_url.starts_with("https://") {
                video_id_or_url.to_string()
            } else {
                format!("https://www.youtube.com/watch?v={video_id_or_url}")
            };

        // Try getting direct streaming audio URL with yt-dlp -g
        let stream_url = Command::new("yt-dlp")
            .args(["-f", "ba/b", "-g", &url])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if let Some(direct_url) = stream_url {
            self.spawn_ffplay(&[&direct_url], true);
        } else {
            // Fallback: pipe yt-dlp directly
            let child_pipe = Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "yt-dlp -f 'ba/b' -o - '{}' | ffplay -nodisp -autoexit -loglevel quiet -",
                    url.replace('\'', "'\\''")
                ))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();

            match child_pipe {
                Ok(child) => {
                    self.active_child = Some(child);
                    self.is_playing = true;
                    self.is_paused = false;
                }
                Err(e) => {
                    self.status_message = format!("Error launching YouTube stream: {e}");
                    self.is_playing = false;
                }
            }
        }
    }

    fn toggle_pause(&mut self) {
        if let Some(child) = &self.active_child {
            let pid = child.id() as i32;
            #[cfg(unix)]
            unsafe {
                if self.is_paused {
                    libc::kill(pid, libc::SIGCONT);
                    self.is_paused = false;
                    self.status_message = "Resumed".to_string();
                } else {
                    libc::kill(pid, libc::SIGSTOP);
                    self.is_paused = true;
                    self.status_message = "Paused".to_string();
                }
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.active_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.is_playing = false;
        self.is_paused = false;
    }

    fn next_track(&mut self) {
        if let PlayerMode::YouTubePlaylist { items, current_idx } = &mut self.mode {
            if *current_idx + 1 < items.len() {
                *current_idx += 1;
                self.start_current_track();
            } else {
                self.status_message = "Reached end of playlist".to_string();
            }
        }
    }

    fn prev_track(&mut self) {
        if let PlayerMode::YouTubePlaylist { current_idx, .. } = &mut self.mode {
            if *current_idx > 0 {
                *current_idx -= 1;
                self.start_current_track();
            }
        }
    }

    fn jump_to_track(&mut self, idx: usize) {
        if let PlayerMode::YouTubePlaylist { items, current_idx } = &mut self.mode {
            if idx < items.len() {
                *current_idx = idx;
                self.start_current_track();
            }
        }
    }

    fn on_track_finished(&mut self) {
        if let PlayerMode::YouTubePlaylist { items, current_idx } = &mut self.mode {
            if *current_idx + 1 < items.len() {
                *current_idx += 1;
                self.start_current_track();
                return;
            }
        }
        self.is_playing = false;
        self.status_message = "Finished playback".to_string();
    }

    fn render(&self, cols: usize, rows: usize) -> io::Result<()> {
        let mut out = String::with_capacity(cols * rows * 4);
        out.push_str("\x1b[H\x1b[?25l");

        // Top Header
        let header = " 🎵 OCEAN LIGHTWEIGHT MEDIA PLAYER ";
        let pad = cols.saturating_sub(header.chars().count());
        out.push_str("\x1b[7m\x1b[1m");
        out.push_str(header);
        out.push_str(&" ".repeat(pad));
        out.push_str("\x1b[0m\r\n\r\n");

        // Now Playing Box
        let state_icon = if self.is_paused {
            "\x1b[1;38;5;214m⏸ PAUSED\x1b[0m"
        } else if self.is_playing {
            "\x1b[1;38;5;82m▶ PLAYING\x1b[0m"
        } else {
            "\x1b[38;5;244m⏹ STOPPED\x1b[0m"
        };

        out.push_str(&format!("  Status: {state_icon}\r\n"));
        out.push_str(&format!(
            "  Info:   \x1b[1;38;5;45m{}\x1b[0m\r\n\r\n",
            self.status_message
        ));

        // Playlist View if playlist mode
        if let PlayerMode::YouTubePlaylist { items, current_idx } = &self.mode {
            out.push_str("  \x1b[1;38;5;220mPlaylist Tracks:\x1b[0m\r\n");
            let max_display = rows.saturating_sub(11).max(3);
            let start = (*current_idx).saturating_sub(max_display / 2);
            let end = (start + max_display).min(items.len());

            for i in start..end {
                let item = &items[i];
                let is_curr = i == *current_idx;
                let cursor = if is_curr {
                    "\x1b[1;38;5;82m ▶ \x1b[0m"
                } else {
                    "   "
                };
                let style = if is_curr {
                    "\x1b[1;38;5;255m"
                } else {
                    "\x1b[38;5;246m"
                };
                out.push_str(&format!(
                    "  {cursor}{style}[{:2}] {}\x1b[0m\r\n",
                    i + 1,
                    item.title
                ));
            }
            out.push_str("\r\n");
        }

        // Controls box
        out.push_str(
            "  \x1b[38;5;242m┌────────────────────────────────────────────────────────┐\x1b[0m\r\n",
        );
        out.push_str("  \x1b[38;5;242m│\x1b[0m \x1b[1;38;5;39mControls:\x1b[0m                                             \x1b[38;5;242m│\x1b[0m\r\n");
        out.push_str("  \x1b[38;5;242m│\x1b[0m   \x1b[1m[Space]\x1b[0m  Pause / Resume        \x1b[1m[q / Esc]\x1b[0m Quit player  \x1b[38;5;242m│\x1b[0m\r\n");
        if matches!(self.mode, PlayerMode::YouTubePlaylist { .. }) {
            out.push_str("  \x1b[38;5;242m│\x1b[0m   \x1b[1m[n]\x1b[0m      Next track            \x1b[1m[p]\x1b[0m       Prev track    \x1b[38;5;242m│\x1b[0m\r\n");
            out.push_str("  \x1b[38;5;242m│\x1b[0m   \x1b[1m[1-9]\x1b[0m    Jump to track         \x1b[1m[s]\x1b[0m       Stop playback \x1b[38;5;242m│\x1b[0m\r\n");
        } else {
            out.push_str("  \x1b[38;5;242m│\x1b[0m   \x1b[1m[s]\x1b[0m      Stop playback                               \x1b[38;5;242m│\x1b[0m\r\n");
        }
        out.push_str(
            "  \x1b[38;5;242m└────────────────────────────────────────────────────────┘\x1b[0m\r\n",
        );

        let mut stdout = io::stdout();
        stdout.write_all(out.as_bytes())?;
        stdout.flush()?;
        Ok(())
    }
}

impl Drop for MediaPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// YouTube Parser
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum YouTubeInput {
    Video(String),
    Playlist(String),
}

pub fn parse_youtube_input(input: &str) -> Option<YouTubeInput> {
    let trimmed = input.trim();

    // Check for "yt:" or "youtube:" prefixes
    if let Some(rest) = trimmed
        .strip_prefix("yt:")
        .or_else(|| trimmed.strip_prefix("youtube:"))
    {
        if let Some(list_id) = rest
            .strip_prefix("playlist:")
            .or_else(|| rest.strip_prefix("list:"))
        {
            return Some(YouTubeInput::Playlist(list_id.to_string()));
        }
        if rest.starts_with("PL") || rest.starts_with("RD") || rest.starts_with("OLAK") {
            return Some(YouTubeInput::Playlist(rest.to_string()));
        }
        return Some(YouTubeInput::Video(rest.to_string()));
    }

    // Direct playlist ID
    if (trimmed.starts_with("PL") || trimmed.starts_with("RD") || trimmed.starts_with("OLAK"))
        && trimmed.len() >= 16
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Some(YouTubeInput::Playlist(trimmed.to_string()));
    }

    // URL containing list=
    if trimmed.contains("list=") {
        if let Some(list_id) = extract_url_param(trimmed, "list") {
            return Some(YouTubeInput::Playlist(list_id));
        }
    }

    // URL containing v=
    if trimmed.contains("watch?") || trimmed.contains("/watch") {
        if let Some(video_id) = extract_url_param(trimmed, "v") {
            return Some(YouTubeInput::Video(video_id));
        }
    }

    // Short URLs: youtu.be/<id>
    if trimmed.contains("youtu.be/") {
        if let Some((_, path)) = trimmed.split_once("youtu.be/") {
            let id = path.split(['?', '&', '/']).next().unwrap_or("");
            if !id.is_empty() {
                return Some(YouTubeInput::Video(id.to_string()));
            }
        }
    }

    // YouTube Shorts: /shorts/<id>
    if trimmed.contains("/shorts/") {
        if let Some((_, path)) = trimmed.split_once("/shorts/") {
            let id = path.split(['?', '&', '/']).next().unwrap_or("");
            if !id.is_empty() {
                return Some(YouTubeInput::Video(id.to_string()));
            }
        }
    }

    // YouTube Embed: /embed/<id>
    if trimmed.contains("/embed/") {
        if let Some((_, path)) = trimmed.split_once("/embed/") {
            let id = path.split(['?', '&', '/']).next().unwrap_or("");
            if !id.is_empty() {
                return Some(YouTubeInput::Video(id.to_string()));
            }
        }
    }

    // 11-char video ID (e.g. dQw4w9WgXcQ)
    if trimmed.len() == 11
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Some(YouTubeInput::Video(trimmed.to_string()));
    }

    None
}

fn extract_url_param(url: &str, param: &str) -> Option<String> {
    let key = format!("{param}=");
    let pos = url.find(&key)?;
    let rest = &url[pos + key.len()..];
    let val = rest
        .split('&')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("");
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

pub fn play_youtube(input: &str) -> io::Result<()> {
    let parsed =
        parse_youtube_input(input).unwrap_or_else(|| YouTubeInput::Video(input.to_string()));

    match parsed {
        YouTubeInput::Playlist(list_id) => {
            println!("\x1b[1;38;5;39m[ocean]\x1b[0m Parsing YouTube playlist {list_id}...");
            let url = format!("https://www.youtube.com/playlist?list={list_id}");
            let items = fetch_playlist_items(&url);
            if items.is_empty() {
                // If playlist fetching failed or single video
                let mut player = MediaPlayer::new(PlayerMode::YouTubeSong(PlaylistItem {
                    id: list_id,
                    title: format!("Playlist {url}"),
                }));
                player.run()
            } else {
                let mut player = MediaPlayer::new(PlayerMode::YouTubePlaylist {
                    items,
                    current_idx: 0,
                });
                player.run()
            }
        }
        YouTubeInput::Video(video_id) => {
            println!("\x1b[1;38;5;39m[ocean]\x1b[0m Fetching YouTube track {video_id}...");
            let title = fetch_video_title(&video_id)
                .unwrap_or_else(|| format!("YouTube Video: {video_id}"));
            let mut player = MediaPlayer::new(PlayerMode::YouTubeSong(PlaylistItem {
                id: video_id,
                title,
            }));
            player.run()
        }
    }
}

pub fn play_local_file(path: &Path, is_video: bool) -> io::Result<()> {
    let path_str = path.to_string_lossy().to_string();
    let mode = if is_video {
        PlayerMode::LocalVideo(path_str)
    } else {
        PlayerMode::LocalAudio(path_str)
    };
    let mut player = MediaPlayer::new(mode);
    player.run()
}

fn fetch_video_title(id: &str) -> Option<String> {
    let url = if id.starts_with("http") {
        id.to_string()
    } else {
        format!("https://www.youtube.com/watch?v={id}")
    };
    let output = Command::new("yt-dlp")
        .args(["--print", "%(title)s", &url])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

fn fetch_playlist_items(playlist_url: &str) -> Vec<PlaylistItem> {
    let output = Command::new("yt-dlp")
        .args([
            "--flat-playlist",
            "--print",
            "%(id)s\t%(title)s",
            playlist_url,
        ])
        .output();

    let mut items = Vec::new();
    if let Ok(out) = output {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines() {
                if let Some((id, title)) = line.split_once('\t') {
                    items.push(PlaylistItem {
                        id: id.trim().to_string(),
                        title: title.trim().to_string(),
                    });
                }
            }
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_youtube_parser_video_url() {
        let parsed = parse_youtube_input("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(parsed, Some(YouTubeInput::Video("dQw4w9WgXcQ".to_string())));
    }

    #[test]
    fn test_youtube_parser_short_url() {
        let parsed = parse_youtube_input("https://youtu.be/dQw4w9WgXcQ?si=abc");
        assert_eq!(parsed, Some(YouTubeInput::Video("dQw4w9WgXcQ".to_string())));
    }

    #[test]
    fn test_youtube_parser_playlist_url() {
        let parsed =
            parse_youtube_input("https://www.youtube.com/playlist?list=PL1234567890abcdef");
        assert_eq!(
            parsed,
            Some(YouTubeInput::Playlist("PL1234567890abcdef".to_string()))
        );
    }

    #[test]
    fn test_youtube_parser_video_id() {
        let parsed = parse_youtube_input("dQw4w9WgXcQ");
        assert_eq!(parsed, Some(YouTubeInput::Video("dQw4w9WgXcQ".to_string())));
    }

    #[test]
    fn test_youtube_parser_prefix() {
        let parsed = parse_youtube_input("yt:dQw4w9WgXcQ");
        assert_eq!(parsed, Some(YouTubeInput::Video("dQw4w9WgXcQ".to_string())));

        let parsed_pl = parse_youtube_input("yt:playlist:PL1234567890abcdef");
        assert_eq!(
            parsed_pl,
            Some(YouTubeInput::Playlist("PL1234567890abcdef".to_string()))
        );
    }
}
