use std::fs;
use std::io;
use std::path::Path;

use super::pager::Pager;

pub fn view_markdown(path: &Path) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document.md");

    let formatted_lines = render_markdown_to_ansi(&content);
    let mut pager = Pager::new(
        file_name.to_string(),
        "📖 [VIEW MODE]".to_string(),
        formatted_lines,
    );
    pager.run()
}

pub fn render_markdown_to_ansi(content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut _code_lang = String::new();

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_end();

        // 1. Fenced Code Blocks
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if in_code_block {
                in_code_block = false;
                lines.push("\x1b[38;5;240m└────────────────────────────────────────────────────────\x1b[0m".to_string());
            } else {
                in_code_block = true;
                let lang = trimmed.trim_start_matches(['`', '~']).trim();
                _code_lang = lang.to_string();
                let badge = if lang.is_empty() {
                    "code".to_string()
                } else {
                    format!("[{lang}]")
                };
                lines.push(format!(
                    "\x1b[38;5;240m┌── \x1b[1;38;5;220m{badge}\x1b[0m \x1b[38;5;240m──────────────────────────────────────────────\x1b[0m"
                ));
            }
            continue;
        }

        if in_code_block {
            lines.push(format!("  \x1b[38;5;252m{raw_line}\x1b[0m"));
            continue;
        }

        // 2. Horizontal Rules
        let is_hr = trimmed.len() >= 3
            && (trimmed.chars().all(|c| c == '-')
                || trimmed.chars().all(|c| c == '*')
                || trimmed.chars().all(|c| c == '_'));
        if is_hr {
            lines.push("\x1b[38;5;240m────────────────────────────────────────────────────────────────\x1b[0m".to_string());
            continue;
        }

        // 3. Headings
        if let Some(h) = parse_heading(trimmed) {
            lines.extend(h);
            continue;
        }

        // 4. Blockquotes
        if trimmed.starts_with('>') {
            let quote_text = trimmed.trim_start_matches('>').trim_start();
            let styled_text = style_inline(quote_text);
            lines.push(format!(
                "  \x1b[38;5;39m│\x1b[0m \x1b[3;38;5;248m{styled_text}\x1b[0m"
            ));
            continue;
        }

        // 5. Task list items
        if trimmed.starts_with("- [ ] ") || trimmed.starts_with("* [ ] ") {
            let rest = &trimmed[6..];
            lines.push(format!("  \x1b[38;5;244m☐\x1b[0m {}", style_inline(rest)));
            continue;
        }
        if trimmed.starts_with("- [x] ")
            || trimmed.starts_with("- [X] ")
            || trimmed.starts_with("* [x] ")
        {
            let rest = &trimmed[6..];
            lines.push(format!("  \x1b[1;38;5;82m☑\x1b[0m {}", style_inline(rest)));
            continue;
        }

        // 6. Unordered lists
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            let rest = &trimmed[2..];
            lines.push(format!("  \x1b[38;5;39m•\x1b[0m {}", style_inline(rest)));
            continue;
        }

        // 7. Ordered lists (e.g. 1. )
        if let Some((num, rest)) = parse_ordered_list(trimmed) {
            lines.push(format!(
                "  \x1b[1;38;5;214m{num}.\x1b[0m {}",
                style_inline(rest)
            ));
            continue;
        }

        // 8. Tables
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            if trimmed
                .chars()
                .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
            {
                lines.push(format!("\x1b[38;5;240m{trimmed}\x1b[0m"));
            } else {
                lines.push(format!("\x1b[38;5;248m{}\x1b[0m", style_inline(trimmed)));
            }
            continue;
        }

        // 9. Normal paragraph text
        lines.push(style_inline(raw_line));
    }

    lines
}

fn parse_heading(line: &str) -> Option<Vec<String>> {
    if let Some(rest) = line.strip_prefix("# ") {
        let text = style_inline(rest);
        Some(vec![
            format!("\x1b[1;38;5;45m# {text}\x1b[0m"),
            "\x1b[38;5;39m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m"
                .to_string(),
        ])
    } else if let Some(rest) = line.strip_prefix("## ") {
        let text = style_inline(rest);
        Some(vec![
            format!("\x1b[1;38;5;214m## {text}\x1b[0m"),
            "\x1b[38;5;208m────────────────────────────────────────────\x1b[0m".to_string(),
        ])
    } else if let Some(rest) = line.strip_prefix("### ") {
        let text = style_inline(rest);
        Some(vec![format!("\x1b[1;38;5;120m### {text}\x1b[0m")])
    } else if let Some(rest) = line.strip_prefix("#### ") {
        let text = style_inline(rest);
        Some(vec![format!("\x1b[1;38;5;183m#### {text}\x1b[0m")])
    } else if let Some(rest) = line.strip_prefix("##### ") {
        let text = style_inline(rest);
        Some(vec![format!("\x1b[1;38;5;246m##### {text}\x1b[0m")])
    } else if let Some(rest) = line.strip_prefix("###### ") {
        let text = style_inline(rest);
        Some(vec![format!("\x1b[3;38;5;242m###### {text}\x1b[0m")])
    } else {
        None
    }
}

fn parse_ordered_list(line: &str) -> Option<(&str, &str)> {
    let (num_part, rest) = line.split_once(". ")?;
    if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
        Some((num_part, rest))
    } else {
        None
    }
}

pub fn style_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '`' {
            // Inline code
            let mut code = String::new();
            let mut closed = false;
            for next_c in chars.by_ref() {
                if next_c == '`' {
                    closed = true;
                    break;
                }
                code.push(next_c);
            }
            if closed {
                out.push_str("\x1b[38;5;222;48;5;236m ");
                out.push_str(&code);
                out.push_str(" \x1b[0m");
            } else {
                out.push('`');
                out.push_str(&code);
            }
        } else if c == '*' && chars.peek() == Some(&'*') {
            // Bold **text**
            chars.next();
            let mut bold_text = String::new();
            let mut closed = false;
            while let Some(b_c) = chars.next() {
                if b_c == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    closed = true;
                    break;
                }
                bold_text.push(b_c);
            }
            if closed {
                out.push_str("\x1b[1;38;5;255m");
                out.push_str(&bold_text);
                out.push_str("\x1b[0m");
            } else {
                out.push_str("**");
                out.push_str(&bold_text);
            }
        } else if c == '*' && chars.peek() != Some(&' ') {
            // Italic *text*
            let mut it_text = String::new();
            let mut closed = false;
            for it_c in chars.by_ref() {
                if it_c == '*' {
                    closed = true;
                    break;
                }
                it_text.push(it_c);
            }
            if closed {
                out.push_str("\x1b[3;38;5;250m");
                out.push_str(&it_text);
                out.push_str("\x1b[0m");
            } else {
                out.push('*');
                out.push_str(&it_text);
            }
        } else if c == '[' {
            // Link [text](url)
            let mut link_text = String::new();
            let mut closed_bracket = false;
            for l_c in chars.by_ref() {
                if l_c == ']' {
                    closed_bracket = true;
                    break;
                }
                link_text.push(l_c);
            }
            if closed_bracket && chars.peek() == Some(&'(') {
                chars.next();
                let mut url = String::new();
                let mut closed_paren = false;
                for u_c in chars.by_ref() {
                    if u_c == ')' {
                        closed_paren = true;
                        break;
                    }
                    url.push(u_c);
                }
                if closed_paren {
                    out.push_str("\x1b[4;38;5;39m");
                    out.push_str(&link_text);
                    out.push_str("\x1b[0m \x1b[38;5;244m(");
                    out.push_str(&url);
                    out.push_str(")\x1b[0m");
                } else {
                    out.push('[');
                    out.push_str(&link_text);
                    out.push_str("](");
                    out.push_str(&url);
                }
            } else {
                out.push('[');
                out.push_str(&link_text);
                if closed_bracket {
                    out.push(']');
                }
            }
        } else {
            out.push(c);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_heading() {
        let lines = render_markdown_to_ansi("# Hello World\nSome text");
        assert!(lines.len() >= 2);
        assert!(lines[0].contains("Hello World"));
    }

    #[test]
    fn test_markdown_code_block() {
        let lines = render_markdown_to_ansi("```rust\nlet x = 42;\n```");
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("[rust]"));
        assert!(lines[1].contains("let x = 42;"));
    }

    #[test]
    fn test_inline_styling() {
        let styled = style_inline("This is **bold** and `code`");
        assert!(styled.contains("\x1b[1;38;5;255mbold\x1b[0m"));
        assert!(styled.contains("\x1b[38;5;222;48;5;236m code \x1b[0m"));
    }
}
