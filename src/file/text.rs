use std::fs;
use std::io;
use std::path::Path;

use super::pager::Pager;

pub fn view_file(path: &Path) -> io::Result<()> {
    let bytes = fs::read(path)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let is_binary = bytes.iter().take(512).any(|&b| b == 0);

    let (badge, lines) = if is_binary {
        ("HEX", render_hex_dump(&bytes))
    } else {
        ("TEXT", render_text_with_line_numbers(&bytes))
    };

    let mut pager = Pager::new(file_name, format!("[{badge} VIEW]"), lines);
    pager.run()
}

fn render_text_with_line_numbers(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let raw_lines: Vec<&str> = text.lines().collect();
    let total_digits = raw_lines.len().to_string().len().max(3);

    raw_lines
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let line_num = idx + 1;
            format!(
                "\x1b[38;5;240m{:width$} │ \x1b[0m{}",
                line_num,
                line,
                width = total_digits
            )
        })
        .collect()
}

fn render_hex_dump(bytes: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let chunk_size = 16;

    for (offset, chunk) in bytes.chunks(chunk_size).enumerate() {
        let abs_offset = offset * chunk_size;
        let mut hex_part = String::with_capacity(50);
        let mut ascii_part = String::with_capacity(18);

        for (i, &b) in chunk.iter().enumerate() {
            if i == 8 {
                hex_part.push(' ');
            }
            hex_part.push_str(&format!("{b:02x} "));
            if b.is_ascii_graphic() || b == b' ' {
                ascii_part.push(b as char);
            } else {
                ascii_part.push('.');
            }
        }

        // Padding if last chunk < 16
        if chunk.len() < 16 {
            let missing = 16 - chunk.len();
            for i in 0..missing {
                if chunk.len() + i == 8 {
                    hex_part.push(' ');
                }
                hex_part.push_str("   ");
            }
        }

        lines.push(format!(
            "\x1b[38;5;244m{:08x}:\x1b[0m \x1b[38;5;220m{}\x1b[0m \x1b[38;5;242m|{}\x1b[0m",
            abs_offset, hex_part, ascii_part
        ));
    }

    if lines.is_empty() {
        lines.push("\x1b[38;5;244m(empty binary file)\x1b[0m".to_string());
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_dump() {
        let data = b"Hello, World!\x00\x01\x02";
        let lines = render_hex_dump(data);
        assert!(!lines.is_empty());
        assert!(lines[0].contains("Hello, World!"));
    }

    #[test]
    fn test_text_render() {
        let text = b"line 1\nline 2";
        let lines = render_text_with_line_numbers(text);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("line 1"));
    }
}
