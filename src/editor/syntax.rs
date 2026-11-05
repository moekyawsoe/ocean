use std::ops::Range;
use std::path::Path;

use crate::config::theme::{Rgb, ThemePalette};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Plain,
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    C,
    Cpp,
    Json,
    Toml,
    Markdown,
    Shell,
    Html,
    Css,
}

impl Language {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let p = path.as_ref();
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "rs" => Self::Rust,
            "py" => Self::Python,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "tsx" => Self::TypeScript,
            "go" => Self::Go,
            "c" | "h" => Self::C,
            "cpp" | "hpp" | "cc" | "cxx" => Self::Cpp,
            "json" => Self::Json,
            "toml" => Self::Toml,
            "md" | "markdown" => Self::Markdown,
            "sh" | "bash" | "zsh" => Self::Shell,
            "html" | "htm" => Self::Html,
            "css" | "scss" => Self::Css,
            _ => {
                let filename = p
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if filename == "cargo.toml" || filename == "config" {
                    Self::Toml
                } else if filename == "dockerfile" {
                    Self::Shell
                } else {
                    Self::Plain
                }
            }
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Plain => "Plain Text",
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Go => "Go",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Json => "JSON",
            Self::Toml => "TOML",
            Self::Markdown => "Markdown",
            Self::Shell => "Shell",
            Self::Html => "HTML",
            Self::Css => "CSS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxTokenKind {
    Plain,
    Keyword,
    Function,
    Type,
    String,
    Number,
    Comment,
    Operator,
    Punctuation,
    Constant,
    Tag,
    Attribute,
    Preproc,
}

impl SyntaxTokenKind {
    pub fn color(&self, palette: &ThemePalette) -> Rgb {
        match self {
            Self::Plain => palette.foreground,
            Self::Keyword => palette.ansi[5],   // Magenta
            Self::Function => palette.ansi[4],  // Blue
            Self::Type => palette.ansi[3],      // Yellow
            Self::String => palette.ansi[2],    // Green
            Self::Number => palette.ansi[6],    // Cyan
            Self::Comment => palette.ansi[8],   // Gray
            Self::Operator => palette.ansi[14], // Bright Cyan
            Self::Punctuation => palette.foreground,
            Self::Constant => palette.ansi[9],   // Bright Red
            Self::Tag => palette.ansi[1],        // Red
            Self::Attribute => palette.ansi[11], // Bright Yellow
            Self::Preproc => palette.ansi[13],   // Bright Magenta
        }
    }
}

#[derive(Debug, Clone)]
pub struct HighlightSpan {
    pub range: Range<usize>,
    pub kind: SyntaxTokenKind,
}

pub struct SyntaxHighlighter {
    pub language: Language,
}

impl SyntaxHighlighter {
    pub fn new(language: Language) -> Self {
        Self { language }
    }

    pub fn highlight_line(&self, line: &str) -> Vec<HighlightSpan> {
        match self.language {
            Language::Rust => highlight_rust(line),
            Language::Python => highlight_python(line),
            Language::JavaScript | Language::TypeScript => highlight_javascript(line),
            Language::Go => highlight_go(line),
            Language::C | Language::Cpp => highlight_c(line),
            Language::Json => highlight_json(line),
            Language::Toml => highlight_toml(line),
            Language::Markdown => highlight_markdown(line),
            Language::Shell => highlight_shell(line),
            Language::Html => highlight_html(line),
            Language::Css => highlight_css(line),
            _ => vec![HighlightSpan {
                range: 0..line.chars().count(),
                kind: SyntaxTokenKind::Plain,
            }],
        }
    }
}

fn highlight_rust(line: &str) -> Vec<HighlightSpan> {
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ];
    const TYPES: &[&str] = &[
        "bool", "char", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64",
        "u128", "usize", "f32", "f64", "str", "String", "Option", "Some", "None", "Result", "Ok",
        "Err", "Vec", "Box", "Arc", "Rc",
    ];

    tokenize_lexical(line, KEYWORDS, TYPES, "//", "/*", "*/", true)
}

fn highlight_python(line: &str) -> Vec<HighlightSpan> {
    const KEYWORDS: &[&str] = &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import",
        "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True",
        "try", "while", "with", "yield",
    ];
    const TYPES: &[&str] = &[
        "int", "float", "str", "bool", "list", "dict", "set", "tuple", "bytes", "object",
    ];

    tokenize_lexical(line, KEYWORDS, TYPES, "#", "\"\"\"", "\"\"\"", false)
}

fn highlight_javascript(line: &str) -> Vec<HighlightSpan> {
    const KEYWORDS: &[&str] = &[
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "let",
        "new",
        "null",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "undefined",
        "var",
        "void",
        "while",
        "with",
        "yield",
    ];
    const TYPES: &[&str] = &[
        "string", "number", "boolean", "any", "void", "Promise", "Array", "Object", "Map", "Set",
    ];

    tokenize_lexical(line, KEYWORDS, TYPES, "//", "/*", "*/", true)
}

fn highlight_go(line: &str) -> Vec<HighlightSpan> {
    const KEYWORDS: &[&str] = &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "type",
        "var",
        "nil",
        "true",
        "false",
    ];
    const TYPES: &[&str] = &[
        "bool",
        "byte",
        "complex64",
        "complex128",
        "error",
        "float32",
        "float64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "rune",
        "string",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "uintptr",
    ];

    tokenize_lexical(line, KEYWORDS, TYPES, "//", "/*", "*/", true)
}

fn highlight_c(line: &str) -> Vec<HighlightSpan> {
    const KEYWORDS: &[&str] = &[
        "auto",
        "break",
        "case",
        "const",
        "continue",
        "default",
        "do",
        "else",
        "enum",
        "extern",
        "for",
        "goto",
        "if",
        "inline",
        "register",
        "restrict",
        "return",
        "sizeof",
        "static",
        "struct",
        "switch",
        "typedef",
        "union",
        "volatile",
        "while",
        "class",
        "public",
        "private",
        "protected",
        "virtual",
        "namespace",
        "template",
        "typename",
        "new",
        "delete",
    ];
    const TYPES: &[&str] = &[
        "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void", "size_t",
        "uint8_t", "uint16_t", "uint32_t", "uint64_t", "int8_t", "int16_t", "int32_t", "int64_t",
        "bool", "true", "false", "NULL",
    ];

    tokenize_lexical(line, KEYWORDS, TYPES, "//", "/*", "*/", true)
}

fn highlight_json(line: &str) -> Vec<HighlightSpan> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < chars.len() {
                i += 1;
            }
            let is_key = chars[i..]
                .iter()
                .find(|&&ch| !ch.is_whitespace())
                .is_some_and(|&ch| ch == ':');
            spans.push(HighlightSpan {
                range: start..i,
                kind: if is_key {
                    SyntaxTokenKind::Keyword
                } else {
                    SyntaxTokenKind::String
                },
            });
        } else if c.is_ascii_digit()
            || (c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
        {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit() || matches!(chars[i], '.' | 'e' | 'E' | '+' | '-'))
            {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::Number,
            });
        } else if line[i..].starts_with("true") || line[i..].starts_with("null") {
            spans.push(HighlightSpan {
                range: i..i + 4,
                kind: SyntaxTokenKind::Constant,
            });
            i += 4;
        } else if line[i..].starts_with("false") {
            spans.push(HighlightSpan {
                range: i..i + 5,
                kind: SyntaxTokenKind::Constant,
            });
            i += 5;
        } else if matches!(c, '{' | '}' | '[' | ']' | ':' | ',') {
            spans.push(HighlightSpan {
                range: i..i + 1,
                kind: SyntaxTokenKind::Punctuation,
            });
            i += 1;
        } else {
            let start = i;
            while i < chars.len()
                && !matches!(chars[i], '"' | '{' | '}' | '[' | ']' | ':' | ',')
                && !chars[i].is_ascii_digit()
            {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::Plain,
            });
        }
    }
    spans
}

fn highlight_toml(line: &str) -> Vec<HighlightSpan> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return vec![HighlightSpan {
            range: 0..line.chars().count(),
            kind: SyntaxTokenKind::Comment,
        }];
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return vec![HighlightSpan {
            range: 0..line.chars().count(),
            kind: SyntaxTokenKind::Type,
        }];
    }
    tokenize_lexical(line, &["true", "false"], &[], "#", "", "", false)
}

fn highlight_markdown(line: &str) -> Vec<HighlightSpan> {
    let chars: Vec<char> = line.chars().collect();
    if line.trim_start().starts_with('#') {
        return vec![HighlightSpan {
            range: 0..chars.len(),
            kind: SyntaxTokenKind::Function,
        }];
    }
    if line.trim_start().starts_with("```") {
        return vec![HighlightSpan {
            range: 0..chars.len(),
            kind: SyntaxTokenKind::Preproc,
        }];
    }
    vec![HighlightSpan {
        range: 0..chars.len(),
        kind: SyntaxTokenKind::Plain,
    }]
}

fn highlight_shell(line: &str) -> Vec<HighlightSpan> {
    const KEYWORDS: &[&str] = &[
        "if", "then", "else", "elif", "fi", "case", "esac", "for", "while", "until", "do", "done",
        "in", "function", "select", "echo", "export", "local", "return", "exit",
    ];
    tokenize_lexical(line, KEYWORDS, &[], "#", "", "", false)
}

fn tokenize_lexical(
    line: &str,
    keywords: &[&str],
    types: &[&str],
    line_comment: &str,
    _block_comment_start: &str,
    _block_comment_end: &str,
    support_preproc: bool,
) -> Vec<HighlightSpan> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let remaining_str: String = chars[i..].iter().collect();

        // Line comment check
        if !line_comment.is_empty() && remaining_str.starts_with(line_comment) {
            spans.push(HighlightSpan {
                range: i..chars.len(),
                kind: SyntaxTokenKind::Comment,
            });
            break;
        }

        // Preprocessor directive (e.g. #include, #![allow])
        if support_preproc && chars[i] == '#' {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::Preproc,
            });
            continue;
        }

        let c = chars[i];

        // String literals
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < chars.len() {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::String,
            });
            continue;
        }

        // Numeric literals
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::Number,
            });
            continue;
        }

        // Identifiers / Keywords / Types
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let kind = if keywords.contains(&word.as_str()) {
                SyntaxTokenKind::Keyword
            } else if types.contains(&word.as_str())
                || (word.starts_with(|ch: char| ch.is_uppercase())
                    && !word.chars().all(|ch| ch.is_uppercase() || ch == '_'))
            {
                SyntaxTokenKind::Type
            } else if i < chars.len() && chars[i] == '(' {
                SyntaxTokenKind::Function
            } else {
                SyntaxTokenKind::Plain
            };
            spans.push(HighlightSpan {
                range: start..i,
                kind,
            });
            continue;
        }

        // Operators & punctuation
        let kind = match c {
            '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!' | '&' | '|' | '^' | '~' | '?' => {
                SyntaxTokenKind::Operator
            }
            '.' | ',' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}' => {
                SyntaxTokenKind::Punctuation
            }
            _ => SyntaxTokenKind::Plain,
        };

        spans.push(HighlightSpan {
            range: i..i + 1,
            kind,
        });
        i += 1;
    }

    spans
}

fn highlight_html(line: &str) -> Vec<HighlightSpan> {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut spans = Vec::new();
    let mut i = 0;
    let mut in_tag = false;
    let mut tag_name_done = false;

    while i < len {
        if !in_tag
            && chars[i] == '<'
            && chars.get(i + 1) == Some(&'!')
            && chars.get(i + 2) == Some(&'-')
            && chars.get(i + 3) == Some(&'-')
        {
            spans.push(HighlightSpan {
                range: i..len,
                kind: SyntaxTokenKind::Comment,
            });
            break;
        }

        let c = chars[i];
        if c == '<' {
            let start = i;
            let doctype = i + 9 <= len
                && chars[i..i + 9]
                    .iter()
                    .collect::<String>()
                    .eq_ignore_ascii_case("<!doctype");
            i += 1;
            if i < len && chars[i] == '/' {
                i += 1;
            }
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '!') {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: if doctype {
                    SyntaxTokenKind::Preproc
                } else {
                    SyntaxTokenKind::Tag
                },
            });
            in_tag = true;
            tag_name_done = true;
            continue;
        }

        if in_tag && c == '>' {
            spans.push(HighlightSpan {
                range: i..i + 1,
                kind: SyntaxTokenKind::Punctuation,
            });
            in_tag = false;
            tag_name_done = false;
            i += 1;
            continue;
        }

        if in_tag && (c == '"' || c == '\'') {
            let quote = c;
            let start = i;
            i += 1;
            while i < len && chars[i] != quote {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::String,
            });
            continue;
        }

        if in_tag && (c.is_alphabetic() || c == '_' || c == ':' || c == '@') {
            let start = i;
            while i < len
                && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '-' | ':' | '@' | '.'))
            {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: if tag_name_done {
                    SyntaxTokenKind::Attribute
                } else {
                    SyntaxTokenKind::Tag
                },
            });
            continue;
        }

        if !in_tag && c == '&' {
            let start = i;
            i += 1;
            while i < len && chars[i] != ';' && i - start < 12 {
                i += 1;
            }
            if i < len && chars[i] == ';' {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::Constant,
            });
            continue;
        }

        if in_tag && matches!(c, '=' | '/' | '?') {
            spans.push(HighlightSpan {
                range: i..i + 1,
                kind: SyntaxTokenKind::Operator,
            });
            i += 1;
            continue;
        }

        let start = i;
        while i < len
            && !(in_tag && matches!(chars[i], '<' | '>' | '"' | '\'' | '=' | '/'))
            && !(in_tag && chars[i].is_alphabetic())
            && !(!in_tag && (chars[i] == '<' || chars[i] == '&'))
        {
            i += 1;
        }
        if i == start {
            i += 1;
        }
        spans.push(HighlightSpan {
            range: start..i,
            kind: SyntaxTokenKind::Plain,
        });
    }

    spans
}

fn highlight_css(line: &str) -> Vec<HighlightSpan> {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut spans = Vec::new();
    let mut i = 0;
    let mut declaration = false;

    while i < len {
        if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            spans.push(HighlightSpan {
                range: i..len,
                kind: SyntaxTokenKind::Comment,
            });
            break;
        }

        let c = chars[i];

        if c == '"' || c == '\'' {
            let quote = c;
            let start = i;
            i += 1;
            while i < len && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < len {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::String,
            });
            continue;
        }

        if c == '#' {
            let start = i;
            i += 1;
            while i < len && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            if i - start > 1 {
                spans.push(HighlightSpan {
                    range: start..i,
                    kind: SyntaxTokenKind::Constant,
                });
                continue;
            }
            i = start + 1;
        }

        if c.is_ascii_digit() {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '.' | '%')) {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::Number,
            });
            continue;
        }

        if c == '@' {
            let start = i;
            i += 1;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '-') {
                i += 1;
            }
            spans.push(HighlightSpan {
                range: start..i,
                kind: SyntaxTokenKind::Keyword,
            });
            continue;
        }

        if c.is_alphabetic() || c == '_' || c == '-' || c == '.' || c == '#' {
            let start = i;
            while i < len
                && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '-' | '.' | '#'))
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let mut lookahead = i;
            while lookahead < len && chars[lookahead].is_whitespace() {
                lookahead += 1;
            }
            let kind = if matches!(
                word.as_str(),
                "important"
                    | "none"
                    | "auto"
                    | "inherit"
                    | "initial"
                    | "unset"
                    | "block"
                    | "flex"
                    | "grid"
                    | "absolute"
                    | "relative"
                    | "fixed"
                    | "sticky"
                    | "transparent"
            ) {
                SyntaxTokenKind::Keyword
            } else if lookahead < len && chars[lookahead] == ':' && !declaration {
                declaration = true;
                SyntaxTokenKind::Attribute
            } else if declaration {
                SyntaxTokenKind::Plain
            } else {
                SyntaxTokenKind::Tag
            };
            spans.push(HighlightSpan {
                range: start..i,
                kind,
            });
            continue;
        }

        if matches!(c, '{' | '}' | ';') {
            if c == '{' {
                declaration = false;
            }
            spans.push(HighlightSpan {
                range: i..i + 1,
                kind: SyntaxTokenKind::Punctuation,
            });
            i += 1;
            continue;
        }

        if matches!(c, ':' | ',' | '(' | ')') {
            spans.push(HighlightSpan {
                range: i..i + 1,
                kind: SyntaxTokenKind::Punctuation,
            });
            i += 1;
            continue;
        }

        spans.push(HighlightSpan {
            range: i..i + 1,
            kind: SyntaxTokenKind::Plain,
        });
        i += 1;
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(
            Language::from_path(Path::new("src/main.rs")),
            Language::Rust
        );
        assert_eq!(
            Language::from_path(Path::new("script.py")),
            Language::Python
        );
        assert_eq!(
            Language::from_path(Path::new("app.ts")),
            Language::TypeScript
        );
        assert_eq!(
            Language::from_path(Path::new("index.js")),
            Language::JavaScript
        );
        assert_eq!(Language::from_path(Path::new("server.go")), Language::Go);
        assert_eq!(
            Language::from_path(Path::new("config.toml")),
            Language::Toml
        );
        assert_eq!(Language::from_path(Path::new("data.json")), Language::Json);
        assert_eq!(
            Language::from_path(Path::new("README.md")),
            Language::Markdown
        );
        assert_eq!(Language::from_path(Path::new("build.sh")), Language::Shell);
        assert_eq!(Language::from_path(Path::new("index.html")), Language::Html);
        assert_eq!(Language::from_path(Path::new("site.css")), Language::Css);
    }

    #[test]
    fn test_rust_highlighting() {
        let spans = highlight_rust("pub fn main() -> u32 { 42 }");
        // "pub" -> Keyword
        assert!(
            spans
                .iter()
                .any(|s| s.kind == SyntaxTokenKind::Keyword && s.range == (0..3))
        );
        // "fn" -> Keyword
        assert!(
            spans
                .iter()
                .any(|s| s.kind == SyntaxTokenKind::Keyword && s.range == (4..6))
        );
        // "main" -> Function
        assert!(
            spans
                .iter()
                .any(|s| s.kind == SyntaxTokenKind::Function && s.range == (7..11))
        );
        // "42" -> Number
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::Number));
    }

    #[test]
    fn test_json_highlighting() {
        let spans = highlight_json("  \"name\": \"Ocean\", \"version\": 1.1");
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::Keyword)); // "name":
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::String)); // "Ocean"
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::Number)); // 1.1
    }

    #[test]
    fn test_html_highlighting() {
        let spans = highlight_html("<div class=\"card\">မြန်မာ</div>");
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::Tag));
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::Attribute));
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::String));

        let comment = highlight_html("<!-- note -->");
        assert_eq!(comment[0].kind, SyntaxTokenKind::Comment);
    }

    #[test]
    fn test_css_highlighting() {
        let spans = highlight_css(".card { color: #58d1eb; margin: 4px; }");
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::Tag));
        assert!(spans.iter().any(|s| s.kind == SyntaxTokenKind::Attribute));
        assert!(
            spans
                .iter()
                .any(|s| matches!(s.kind, SyntaxTokenKind::Constant | SyntaxTokenKind::Number))
        );
    }

    #[test]
    fn test_myanmar_text_in_syntax() {
        let spans = highlight_rust("// မင်္ဂလာပါ မြန်မာ Unicode comment");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, SyntaxTokenKind::Comment);
    }
}
