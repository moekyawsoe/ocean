use std::path::Path;

use crate::editor::syntax::Language;
use crate::renderer::QuadInstance;
use crate::renderer::atlas::GlyphAtlas;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconId {
    Explorer,
    Search,
    Problems,
    Plugins,
    ChevronRight,
    ChevronDown,
    Close,
    Plus,
    Dirty,
    File,
    Folder,
    FolderOpen,
    Save,
    Refresh,
    Settings,
    Error,
    Warning,
    Info,
    Git,
    Outline,
    Terminal,
    Split,
    Check,
    FileRust,
    FileTypescript,
    FileJavascript,
    FilePython,
    FileGo,
    FileC,
    FileCpp,
    FileHtml,
    FileCss,
    FileJson,
    FileToml,
    FileMarkdown,
    FileShell,
    FileDocker,
    FilePlain,
}

pub const ICON_TABLE: &[(IconId, &[u8], &[u8])] = &[
    (
        IconId::Explorer,
        include_bytes!("../../assets/icons/ui/explorer-16.png"),
        include_bytes!("../../assets/icons/ui/explorer-32.png"),
    ),
    (
        IconId::Search,
        include_bytes!("../../assets/icons/ui/search-16.png"),
        include_bytes!("../../assets/icons/ui/search-32.png"),
    ),
    (
        IconId::Problems,
        include_bytes!("../../assets/icons/ui/problems-16.png"),
        include_bytes!("../../assets/icons/ui/problems-32.png"),
    ),
    (
        IconId::Plugins,
        include_bytes!("../../assets/icons/ui/plugins-16.png"),
        include_bytes!("../../assets/icons/ui/plugins-32.png"),
    ),
    (
        IconId::ChevronRight,
        include_bytes!("../../assets/icons/ui/chevron-right-16.png"),
        include_bytes!("../../assets/icons/ui/chevron-right-32.png"),
    ),
    (
        IconId::ChevronDown,
        include_bytes!("../../assets/icons/ui/chevron-down-16.png"),
        include_bytes!("../../assets/icons/ui/chevron-down-32.png"),
    ),
    (
        IconId::Close,
        include_bytes!("../../assets/icons/ui/close-16.png"),
        include_bytes!("../../assets/icons/ui/close-32.png"),
    ),
    (
        IconId::Plus,
        include_bytes!("../../assets/icons/ui/plus-16.png"),
        include_bytes!("../../assets/icons/ui/plus-32.png"),
    ),
    (
        IconId::Dirty,
        include_bytes!("../../assets/icons/ui/dirty-16.png"),
        include_bytes!("../../assets/icons/ui/dirty-32.png"),
    ),
    (
        IconId::File,
        include_bytes!("../../assets/icons/ui/file-16.png"),
        include_bytes!("../../assets/icons/ui/file-32.png"),
    ),
    (
        IconId::Folder,
        include_bytes!("../../assets/icons/ui/folder-16.png"),
        include_bytes!("../../assets/icons/ui/folder-32.png"),
    ),
    (
        IconId::FolderOpen,
        include_bytes!("../../assets/icons/ui/folder-open-16.png"),
        include_bytes!("../../assets/icons/ui/folder-open-32.png"),
    ),
    (
        IconId::Save,
        include_bytes!("../../assets/icons/ui/save-16.png"),
        include_bytes!("../../assets/icons/ui/save-32.png"),
    ),
    (
        IconId::Refresh,
        include_bytes!("../../assets/icons/ui/refresh-16.png"),
        include_bytes!("../../assets/icons/ui/refresh-32.png"),
    ),
    (
        IconId::Settings,
        include_bytes!("../../assets/icons/ui/settings-16.png"),
        include_bytes!("../../assets/icons/ui/settings-32.png"),
    ),
    (
        IconId::Error,
        include_bytes!("../../assets/icons/ui/error-16.png"),
        include_bytes!("../../assets/icons/ui/error-32.png"),
    ),
    (
        IconId::Warning,
        include_bytes!("../../assets/icons/ui/warning-16.png"),
        include_bytes!("../../assets/icons/ui/warning-32.png"),
    ),
    (
        IconId::Info,
        include_bytes!("../../assets/icons/ui/info-16.png"),
        include_bytes!("../../assets/icons/ui/info-32.png"),
    ),
    (
        IconId::Git,
        include_bytes!("../../assets/icons/ui/git-16.png"),
        include_bytes!("../../assets/icons/ui/git-32.png"),
    ),
    (
        IconId::Outline,
        include_bytes!("../../assets/icons/ui/outline-16.png"),
        include_bytes!("../../assets/icons/ui/outline-32.png"),
    ),
    (
        IconId::Terminal,
        include_bytes!("../../assets/icons/ui/terminal-16.png"),
        include_bytes!("../../assets/icons/ui/terminal-32.png"),
    ),
    (
        IconId::Split,
        include_bytes!("../../assets/icons/ui/split-16.png"),
        include_bytes!("../../assets/icons/ui/split-32.png"),
    ),
    (
        IconId::Check,
        include_bytes!("../../assets/icons/ui/check-16.png"),
        include_bytes!("../../assets/icons/ui/check-32.png"),
    ),
    (
        IconId::FileRust,
        include_bytes!("../../assets/icons/ui/file-rust-16.png"),
        include_bytes!("../../assets/icons/ui/file-rust-32.png"),
    ),
    (
        IconId::FileTypescript,
        include_bytes!("../../assets/icons/ui/file-typescript-16.png"),
        include_bytes!("../../assets/icons/ui/file-typescript-32.png"),
    ),
    (
        IconId::FileJavascript,
        include_bytes!("../../assets/icons/ui/file-javascript-16.png"),
        include_bytes!("../../assets/icons/ui/file-javascript-32.png"),
    ),
    (
        IconId::FilePython,
        include_bytes!("../../assets/icons/ui/file-python-16.png"),
        include_bytes!("../../assets/icons/ui/file-python-32.png"),
    ),
    (
        IconId::FileGo,
        include_bytes!("../../assets/icons/ui/file-go-16.png"),
        include_bytes!("../../assets/icons/ui/file-go-32.png"),
    ),
    (
        IconId::FileC,
        include_bytes!("../../assets/icons/ui/file-c-16.png"),
        include_bytes!("../../assets/icons/ui/file-c-32.png"),
    ),
    (
        IconId::FileCpp,
        include_bytes!("../../assets/icons/ui/file-cpp-16.png"),
        include_bytes!("../../assets/icons/ui/file-cpp-32.png"),
    ),
    (
        IconId::FileHtml,
        include_bytes!("../../assets/icons/ui/file-html-16.png"),
        include_bytes!("../../assets/icons/ui/file-html-32.png"),
    ),
    (
        IconId::FileCss,
        include_bytes!("../../assets/icons/ui/file-css-16.png"),
        include_bytes!("../../assets/icons/ui/file-css-32.png"),
    ),
    (
        IconId::FileJson,
        include_bytes!("../../assets/icons/ui/file-json-16.png"),
        include_bytes!("../../assets/icons/ui/file-json-32.png"),
    ),
    (
        IconId::FileToml,
        include_bytes!("../../assets/icons/ui/file-toml-16.png"),
        include_bytes!("../../assets/icons/ui/file-toml-32.png"),
    ),
    (
        IconId::FileMarkdown,
        include_bytes!("../../assets/icons/ui/file-markdown-16.png"),
        include_bytes!("../../assets/icons/ui/file-markdown-32.png"),
    ),
    (
        IconId::FileShell,
        include_bytes!("../../assets/icons/ui/file-shell-16.png"),
        include_bytes!("../../assets/icons/ui/file-shell-32.png"),
    ),
    (
        IconId::FileDocker,
        include_bytes!("../../assets/icons/ui/file-docker-16.png"),
        include_bytes!("../../assets/icons/ui/file-docker-32.png"),
    ),
    (
        IconId::FilePlain,
        include_bytes!("../../assets/icons/ui/file-plain-16.png"),
        include_bytes!("../../assets/icons/ui/file-plain-32.png"),
    ),
];

impl IconId {
    pub fn is_color(self) -> bool {
        matches!(
            self,
            Self::FileRust
                | Self::FileTypescript
                | Self::FileJavascript
                | Self::FilePython
                | Self::FileGo
                | Self::FileC
                | Self::FileCpp
                | Self::FileHtml
                | Self::FileCss
                | Self::FileJson
                | Self::FileToml
                | Self::FileMarkdown
                | Self::FileShell
                | Self::FileDocker
                | Self::FilePlain
        )
    }

    pub fn bytes(self, px: u32) -> Option<&'static [u8]> {
        ICON_TABLE
            .iter()
            .find(|(id, _, _)| *id == self)
            .map(|(_, one, two)| if px >= 32 { *two } else { *one })
    }
}

pub fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for px in bytes.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        _ => return None,
    };
    Some((info.width, info.height, rgba))
}

pub fn file_icon_for(path: &Path, is_dir: bool, expanded: bool) -> IconId {
    if is_dir {
        return if expanded {
            IconId::FolderOpen
        } else {
            IconId::Folder
        };
    }

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name == "dockerfile" {
        return IconId::FileDocker;
    }

    let language = Language::from_path(path);
    match language {
        Language::Rust => IconId::FileRust,
        Language::TypeScript => IconId::FileTypescript,
        Language::JavaScript => IconId::FileJavascript,
        Language::Python => IconId::FilePython,
        Language::Go => IconId::FileGo,
        Language::C => IconId::FileC,
        Language::Cpp => IconId::FileCpp,
        Language::Html => IconId::FileHtml,
        Language::Css => IconId::FileCss,
        Language::Json => IconId::FileJson,
        Language::Toml => IconId::FileToml,
        Language::Markdown => IconId::FileMarkdown,
        Language::Shell => IconId::FileShell,
        Language::Plain => IconId::FilePlain,
    }
}

pub fn file_icon_for_name(name: &str) -> IconId {
    file_icon_for(Path::new(name), false, false)
}

pub fn emit(
    atlas: &mut GlyphAtlas,
    target: &mut Vec<QuadInstance>,
    icon: IconId,
    x: f32,
    y: f32,
    size: f32,
    color: [f32; 4],
) -> bool {
    if size <= 0.0 {
        return false;
    }
    let bucket = if size >= 24.0 { 32 } else { 16 };
    let entry = atlas.get_or_insert_icon(icon, bucket);
    if entry.width == 0 || entry.height == 0 {
        return false;
    }
    target.push(QuadInstance {
        pos: [x.round(), y.round()],
        size: [size, size],
        uv_pos: [entry.uv_x, entry.uv_y],
        uv_size: [entry.uv_w, entry.uv_h],
        color: if icon.is_color() {
            [1.0, 1.0, 1.0, color[3]]
        } else {
            color
        },
        kind: if icon.is_color() { 2.0 } else { 1.0 },
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_icons() {
        let (w, h, rgba) = decode_png(IconId::Explorer.bytes(16).unwrap()).expect("decode");
        assert_eq!((w, h), (16, 16));
        assert_eq!(rgba.len(), 16 * 16 * 4);
    }

    #[test]
    fn maps_file_icons() {
        assert_eq!(
            file_icon_for(Path::new("main.rs"), false, false),
            IconId::FileRust
        );
        assert_eq!(
            file_icon_for(Path::new("app.tsx"), false, false),
            IconId::FileTypescript
        );
        assert_eq!(
            file_icon_for(Path::new("index.html"), false, false),
            IconId::FileHtml
        );
        assert_eq!(
            file_icon_for(Path::new("src"), true, true),
            IconId::FolderOpen
        );
    }
}
