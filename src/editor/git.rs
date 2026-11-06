use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl GitChangeKind {
    pub fn letter(self) -> char {
        match self {
            Self::Modified => 'M',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Untracked => '?',
            Self::Conflict => '!',
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitChange {
    pub path: PathBuf,
    pub relative: String,
    pub kind: GitChangeKind,
    pub staged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitLineChange {
    pub line: usize,
    pub kind: GitChangeKind,
}

#[derive(Debug, Clone, Default)]
pub struct GitBlameLine {
    pub author: String,
    pub timestamp: i64,
    pub summary: String,
    pub commit: String,
}

#[derive(Debug, Default)]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub changes: Vec<GitChange>,
    pub error: Option<String>,
    pub last_refresh: Option<std::time::Instant>,
    file_changes: HashMap<PathBuf, Vec<GitLineChange>>,
    blame: HashMap<PathBuf, Vec<GitBlameLine>>,
}

impl GitStatus {
    pub fn refresh(&mut self, root: &Path) {
        self.last_refresh = Some(std::time::Instant::now());
        self.is_repo = false;
        self.branch = None;
        self.changes.clear();
        self.error = None;
        self.file_changes.clear();
        self.blame.clear();

        if root.as_os_str().is_empty() || !root.is_dir() {
            return;
        }

        match git_output(root, &["rev-parse", "--is-inside-work-tree"]) {
            Ok(text) if text.trim() == "true" => {}
            Ok(_) => return,
            Err(err) => {
                if !err.is_empty() && !err.to_ascii_lowercase().contains("not a git repository") {
                    self.error = Some(err);
                }
                return;
            }
        }
        self.is_repo = true;

        if let Ok(text) = git_output(root, &["rev-parse", "--abbrev-ref", "HEAD"]) {
            let branch = text.trim();
            if !branch.is_empty() {
                self.branch = Some(if branch == "HEAD" {
                    "detached".to_string()
                } else {
                    branch.to_string()
                });
            }
        }

        if let Ok(text) = git_output(root, &["status", "--porcelain=v1"]) {
            self.changes = parse_status(root, &text);
        }
    }

    pub fn refresh_file(&mut self, root: &Path, path: &Path) {
        if !self.is_repo || root.as_os_str().is_empty() {
            return;
        }
        let Some(relative) = relative_path(root, path) else {
            return;
        };

        let untracked = self
            .changes
            .iter()
            .find(|change| change.path == path)
            .is_some_and(|change| change.kind == GitChangeKind::Untracked);
        if untracked {
            let count = std::fs::read_to_string(path)
                .map(|text| text.lines().count())
                .unwrap_or(0);
            self.file_changes.insert(
                path.to_path_buf(),
                (0..count)
                    .map(|line| GitLineChange {
                        line,
                        kind: GitChangeKind::Added,
                    })
                    .collect(),
            );
            return;
        }

        match git_output(
            root,
            &["diff", "HEAD", "--unified=0", "--no-color", "--", &relative],
        ) {
            Ok(diff) => {
                self.file_changes
                    .insert(path.to_path_buf(), parse_diff(&diff));
            }
            Err(err) => {
                self.file_changes.remove(path);
                if self.error.is_none() {
                    self.error = Some(err);
                }
            }
        }
    }

    pub fn refresh_blame(&mut self, root: &Path, path: &Path) {
        if !self.is_repo || root.as_os_str().is_empty() {
            return;
        }
        let Some(relative) = relative_path(root, path) else {
            return;
        };
        match git_output(root, &["blame", "--line-porcelain", "--", &relative]) {
            Ok(text) => {
                self.blame.insert(path.to_path_buf(), parse_blame(&text));
            }
            Err(_) => {
                self.blame.remove(path);
            }
        }
    }

    pub fn line_changes(&self, path: &Path) -> &[GitLineChange] {
        self.file_changes
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn blame_for(&self, path: &Path) -> Option<&[GitBlameLine]> {
        self.blame.get(path).map(Vec::as_slice)
    }

    pub fn diff_text(&self, root: &Path, path: &Path) -> Option<String> {
        let relative = relative_path(root, path)?;
        let untracked = self
            .changes
            .iter()
            .find(|change| change.path == path)
            .is_some_and(|change| change.kind == GitChangeKind::Untracked);
        if untracked {
            let text = std::fs::read_to_string(path).ok()?;
            return Some(
                text.lines()
                    .map(|line| format!("+{line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        git_output(root, &["diff", "HEAD", "--no-color", "--", &relative])
            .ok()
            .filter(|text| !text.trim().is_empty())
    }
}

pub fn format_blame(line: &GitBlameLine) -> String {
    let author = if line.author.is_empty() {
        "unknown"
    } else {
        line.author.as_str()
    };
    let summary = line.summary.trim();
    let summary = if summary.is_empty() {
        String::new()
    } else {
        format!(" · {summary}")
    };
    format!(
        "{author}, {} ago · {}{summary}",
        relative_time(line.timestamp),
        line.commit
    )
}

fn relative_time(timestamp: i64) -> String {
    if timestamp <= 0 {
        return "unknown".to_string();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(timestamp);
    let delta = now.saturating_sub(timestamp).max(0);
    match delta {
        0..=59 => "now".to_string(),
        60..=3599 => format!("{}m", delta / 60),
        3600..=86399 => format!("{}h", delta / 3600),
        86400..=2_591_999 => format!("{}d", delta / 86400),
        2_592_000..=31_535_999 => format!("{}mo", delta / 2_592_000),
        _ => format!("{}y", delta / 31_536_000),
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .map_err(|err| format!("git unavailable: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "git command failed".to_string()
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

fn parse_status(root: &Path, text: &str) -> Vec<GitChange> {
    let mut changes = Vec::new();
    for line in text.lines() {
        if line.len() < 4 {
            continue;
        }
        let bytes = line.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let mut relative = line[3..].trim().to_string();
        if let Some((_, target)) = relative.rsplit_once(" -> ") {
            relative = target.to_string();
        }
        relative = relative.trim_matches('"').to_string();
        if relative.is_empty() {
            continue;
        }

        let kind = if x == '?' || y == '?' {
            GitChangeKind::Untracked
        } else if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
            GitChangeKind::Conflict
        } else if x == 'R' || y == 'R' {
            GitChangeKind::Renamed
        } else if x == 'A' || y == 'A' {
            GitChangeKind::Added
        } else if x == 'D' || y == 'D' {
            GitChangeKind::Deleted
        } else {
            GitChangeKind::Modified
        };
        changes.push(GitChange {
            path: root.join(&relative),
            relative,
            kind,
            staged: x != ' ' && x != '?',
        });
    }
    changes
}

fn parse_diff(diff: &str) -> Vec<GitLineChange> {
    let mut changes = Vec::new();
    let mut new_line = 0usize;
    let mut old_left = 0usize;
    let mut new_left = 0usize;
    let mut in_hunk = false;

    for line in diff.lines() {
        if line.starts_with("@@") {
            let Some((old_count, new_start, new_count)) = parse_hunk_header(line) else {
                in_hunk = false;
                continue;
            };
            in_hunk = true;
            old_left = old_count;
            new_left = new_count;
            new_line = new_start;
            if new_count == 0 {
                changes.push(GitLineChange {
                    line: new_start.saturating_sub(1),
                    kind: GitChangeKind::Deleted,
                });
            }
            continue;
        }
        if !in_hunk {
            continue;
        }
        if line.starts_with('+') && !line.starts_with("+++") {
            changes.push(GitLineChange {
                line: new_line.saturating_sub(1),
                kind: if old_left > 0 {
                    GitChangeKind::Modified
                } else {
                    GitChangeKind::Added
                },
            });
            new_line += 1;
            new_left = new_left.saturating_sub(1);
        } else if line.starts_with('-') && !line.starts_with("---") {
            old_left = old_left.saturating_sub(1);
        } else if line.starts_with(' ') {
            new_line += 1;
            new_left = new_left.saturating_sub(1);
            old_left = old_left.saturating_sub(1);
        }
        if old_left == 0 && new_left == 0 {
            in_hunk = false;
        }
    }

    changes.sort_by_key(|change| change.line);
    changes.dedup_by_key(|change| change.line);
    changes
}

fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize)> {
    let mut parts = line.split_whitespace();
    parts.next()?; // @@
    let old = parts.next()?.trim_start_matches('-');
    let new = parts.next()?.trim_start_matches('+');
    let old_count = old
        .split_once(',')
        .map(|(_, count)| count.parse().unwrap_or(1))
        .unwrap_or(1);
    let (new_start, new_count) = match new.split_once(',') {
        Some((start, count)) => (start.parse().ok()?, count.parse().unwrap_or(1)),
        None => (new.parse().ok()?, 1),
    };
    Some((old_count, new_start, new_count))
}

fn parse_blame(text: &str) -> Vec<GitBlameLine> {
    let mut lines = Vec::new();
    let mut current = GitBlameLine::default();
    for line in text.lines() {
        if line.len() >= 40
            && line.as_bytes()[..40]
                .iter()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            if !current.commit.is_empty() {
                lines.push(current);
                current = GitBlameLine::default();
            }
            current.commit = line[..7].to_string();
        } else if let Some(rest) = line.strip_prefix("author ") {
            current.author = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            current.timestamp = rest.parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("summary ") {
            current.summary = rest.to_string();
        } else if line.starts_with('\t') {
            lines.push(current);
            current = GitBlameLine::default();
        }
    }
    if !current.commit.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_marks_staged_changes() {
        let root = Path::new("/repo");
        let changes = parse_status(
            root,
            " M src/main.rs\nM  README.md\n?? new file.txt\nR  old.rs -> src/new.rs\n",
        );
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0].relative, "src/main.rs");
        assert_eq!(changes[0].kind, GitChangeKind::Modified);
        assert!(!changes[0].staged);
        assert!(changes[1].staged);
        assert_eq!(changes[2].kind, GitChangeKind::Untracked);
        assert_eq!(changes[3].relative, "src/new.rs");
        assert_eq!(changes[3].kind, GitChangeKind::Renamed);
    }

    #[test]
    fn parses_zero_context_diff_hunks() {
        let diff = "\
@@ -3,2 +3,3 @@ fn main() {
-old
+new
+extra
 context
@@ -10 +11,0 @@
-removed
";
        let changes = parse_diff(diff);
        assert!(
            changes
                .iter()
                .any(|change| change.line == 2 && change.kind == GitChangeKind::Modified)
        );
        assert!(
            changes
                .iter()
                .any(|change| change.line == 3 && change.kind == GitChangeKind::Modified)
        );
        assert!(
            changes
                .iter()
                .any(|change| change.line == 10 && change.kind == GitChangeKind::Deleted)
        );
    }

    #[test]
    fn refreshes_status_diff_and_blame_in_a_real_repository() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("ocean_git_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "ocean@example.com"]);
        run(&["config", "user.name", "Ocean Test"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        run(&["add", "."]);
        run(&["-c", "commit.gpgsign=false", "commit", "-qm", "init"]);
        std::fs::write(root.join("a.txt"), "one\nchanged\n").unwrap();

        let path = root.join("a.txt");
        let mut git = GitStatus::default();
        git.refresh(&root);
        assert!(git.is_repo);
        assert_eq!(git.changes.len(), 1);
        assert_eq!(git.changes[0].kind, GitChangeKind::Modified);

        git.refresh_file(&root, &path);
        assert!(
            git.line_changes(&path)
                .iter()
                .any(|change| change.line == 1)
        );

        git.refresh_blame(&root, &path);
        let blame = git.blame_for(&path).expect("blame");
        assert_eq!(blame.len(), 2);
        assert_eq!(blame[0].author, "Ocean Test");
        assert_eq!(blame[1].author, "Not Committed Yet");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parses_line_porcelain_blame() {
        let text = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1 1
author Alice
author-time 1700000000
summary Fix parser
\tfn main() {}
";
        let blame = parse_blame(text);
        assert_eq!(blame.len(), 1);
        assert_eq!(blame[0].author, "Alice");
        assert_eq!(blame[0].commit, "aaaaaaa");
        assert_eq!(blame[0].summary, "Fix parser");
    }
}
