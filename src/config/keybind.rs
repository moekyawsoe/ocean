use winit::keyboard::{Key, ModifiersState, NamedKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ModMask {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl ModMask {
    pub fn from_winit(m: ModifiersState) -> Self {
        Self {
            ctrl: m.control_key(),
            alt: m.alt_key(),
            shift: m.shift_key(),
            super_key: m.super_key(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyTrigger {
    Char(char),
    Named(NamedKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Left,
    Right,
    Up,
    Down,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitFocusTarget {
    Left,
    Right,
    Up,
    Down,
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    CopyToClipboard,
    PasteFromClipboard,
    IncreaseFontSize(f32),
    DecreaseFontSize(f32),
    ResetFontSize,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    ScrollPageLines(i32),
    ClearScreen,
    ReloadConfig,
    NewTab,
    NewSplit(SplitDirection),
    GotoSplit(SplitFocusTarget),
    ResizeSplit(SplitDirection, u16),
    EqualizeSplits,
    CloseSurface,
    CloseTab,
    NextTab,
    PreviousTab,
    GotoTab(usize),
    NewWindow,
    ToggleFullscreen,
    ToggleWindowDecorations,
    ToggleStatusBar,
    ToggleContextMenu,
    ToggleMyanmarZawgyi,
    SetTheme(String),
    NextTheme,
    ShowHelp,
    EditConfig,
    TriggerUpdate,
    ToggleEditor,
    ToggleSplitMode,
    Quit,
    SendText(Vec<u8>),
    Ignore,
    Unbind,
}

#[derive(Debug, Clone)]
pub struct Keybind {
    pub mods: ModMask,
    pub key: KeyTrigger,
    pub action: Action,
}

pub fn default_keybinds() -> Vec<Keybind> {
    let entries = [
        "ctrl+insert=copy_to_clipboard",
        "ctrl+shift+c=copy_to_clipboard",
        "shift+insert=paste_from_clipboard",
        "ctrl+shift+v=paste_from_clipboard",
        "ctrl+plus=increase_font_size:1",
        "ctrl+shift+equal=increase_font_size:1",
        "ctrl+shift+plus=increase_font_size:1",
        "ctrl+equal=increase_font_size:1",
        "ctrl+minus=decrease_font_size:1",
        "ctrl+0=reset_font_size",
        "shift+page_up=scroll_page_up",
        "shift+page_down=scroll_page_down",
        "shift+home=scroll_to_top",
        "shift+end=scroll_to_bottom",
        "ctrl+shift+up=scroll_page_lines:-3",
        "ctrl+shift+down=scroll_page_lines:3",
        "ctrl+shift+k=clear_screen",
        "ctrl+shift+comma=reload_config",
        "ctrl+shift+r=reload_config",
        "ctrl+shift+t=new_tab",
        "ctrl+shift+d=new_split:right",
        "ctrl+shift+e=new_split:down",
        "alt+shift+left=goto_split:left",
        "alt+shift+right=goto_split:right",
        "alt+shift+up=goto_split:up",
        "alt+shift+down=goto_split:down",
        "alt+shift+h=goto_split:left",
        "alt+shift+l=goto_split:right",
        "alt+shift+k=goto_split:up",
        "alt+shift+j=goto_split:down",
        "ctrl+alt+left=resize_split:left",
        "ctrl+alt+right=resize_split:right",
        "ctrl+alt+up=resize_split:up",
        "ctrl+alt+down=resize_split:down",
        "ctrl+alt+h=resize_split:left",
        "ctrl+alt+l=resize_split:right",
        "ctrl+alt+k=resize_split:up",
        "ctrl+alt+j=resize_split:down",
        "alt+shift+plus=equalize_splits",
        "alt+shift+equal=equalize_splits",
        "ctrl+shift+w=close_surface",
        "ctrl+page_down=next_tab",
        "ctrl+tab=next_tab",
        "ctrl+page_up=previous_tab",
        "ctrl+shift+tab=previous_tab",
        "ctrl+shift+n=new_window",
        "f11=toggle_fullscreen",
        "ctrl+shift+b=toggle_status_bar",
        "context_menu=toggle_context_menu",
        "shift+f10=toggle_context_menu",
        "ctrl+shift+p=toggle_context_menu",
        "ctrl+shift+m=toggle_myanmar_zawgyi",
        "ctrl+shift+slash=show_help",
        "ctrl+shift+question=show_help",
        "ctrl+backslash=toggle_split_mode",
        "ctrl+shift+i=toggle_editor",
        "ctrl+shift+q=quit",
    ];
    entries.iter().filter_map(|s| parse_keybind(s)).collect()
}

/// Parse Ghostty-style `mods+key=action[:arg]`, e.g. `ctrl+shift+c=copy_to_clipboard`
pub fn parse_keybind(spec: &str) -> Option<Keybind> {
    let s = spec.trim();
    // Strip optional Ghostty prefix like `global:` or `performable:`
    let s = s
        .strip_prefix("performable:")
        .or_else(|| s.strip_prefix("global:"))
        .or_else(|| s.strip_prefix("all:"))
        .unwrap_or(s);

    // Note: key itself could be `equal` or `=`, so split on the last `=` before action name,
    // or first `=` where rhs is a valid action.
    let (lhs, rhs) = split_trigger_action(s)?;
    let (mods, key) = parse_trigger(lhs)?;
    let action = parse_action(rhs)?;
    Some(Keybind { mods, key, action })
}

fn split_trigger_action(s: &str) -> Option<(&str, &str)> {
    // Handle e.g. `ctrl+==increase_font_size:1` as well as `ctrl+equal=increase_font_size:1`
    for (idx, ch) in s.char_indices() {
        if ch == '=' && idx > 0 {
            let lhs = &s[..idx];
            let rhs = &s[idx + 1..];
            if !lhs.ends_with('+') && !rhs.is_empty() {
                return Some((lhs.trim(), rhs.trim()));
            }
        }
    }
    None
}

fn parse_trigger(lhs: &str) -> Option<(ModMask, KeyTrigger)> {
    let mut mods = ModMask::default();
    let mut key_part: Option<&str> = None;

    for tok in lhs.split('+') {
        let t = tok.trim();
        let lower = t.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "opt" | "option" | "meta" => mods.alt = true,
            "shift" => mods.shift = true,
            "super" | "cmd" | "command" | "win" | "logo" => mods.super_key = true,
            _ => key_part = Some(t),
        }
    }

    let kp = key_part?;
    let lower = kp.to_ascii_lowercase();
    let trigger = match lower.as_str() {
        "enter" | "return" => KeyTrigger::Named(NamedKey::Enter),
        "tab" => KeyTrigger::Named(NamedKey::Tab),
        "space" => KeyTrigger::Named(NamedKey::Space),
        "backspace" | "bs" => KeyTrigger::Named(NamedKey::Backspace),
        "escape" | "esc" => KeyTrigger::Named(NamedKey::Escape),
        "insert" | "ins" => KeyTrigger::Named(NamedKey::Insert),
        "delete" | "del" => KeyTrigger::Named(NamedKey::Delete),
        "home" => KeyTrigger::Named(NamedKey::Home),
        "end" => KeyTrigger::Named(NamedKey::End),
        "page_up" | "pageup" | "pgup" => KeyTrigger::Named(NamedKey::PageUp),
        "page_down" | "pagedown" | "pgdn" => KeyTrigger::Named(NamedKey::PageDown),
        "up" | "arrow_up" => KeyTrigger::Named(NamedKey::ArrowUp),
        "down" | "arrow_down" => KeyTrigger::Named(NamedKey::ArrowDown),
        "left" | "arrow_left" => KeyTrigger::Named(NamedKey::ArrowLeft),
        "right" | "arrow_right" => KeyTrigger::Named(NamedKey::ArrowRight),
        "context_menu" | "contextmenu" | "menu" | "apps" => {
            KeyTrigger::Named(NamedKey::ContextMenu)
        }
        "f1" => KeyTrigger::Named(NamedKey::F1),
        "f2" => KeyTrigger::Named(NamedKey::F2),
        "f3" => KeyTrigger::Named(NamedKey::F3),
        "f4" => KeyTrigger::Named(NamedKey::F4),
        "f5" => KeyTrigger::Named(NamedKey::F5),
        "f6" => KeyTrigger::Named(NamedKey::F6),
        "f7" => KeyTrigger::Named(NamedKey::F7),
        "f8" => KeyTrigger::Named(NamedKey::F8),
        "f9" => KeyTrigger::Named(NamedKey::F9),
        "f10" => KeyTrigger::Named(NamedKey::F10),
        "f11" => KeyTrigger::Named(NamedKey::F11),
        "f12" => KeyTrigger::Named(NamedKey::F12),
        "plus" => KeyTrigger::Char('+'),
        "minus" | "hyphen" => KeyTrigger::Char('-'),
        "equal" | "equals" => KeyTrigger::Char('='),
        "comma" => KeyTrigger::Char(','),
        "period" | "dot" => KeyTrigger::Char('.'),
        "slash" => KeyTrigger::Char('/'),
        "question" => KeyTrigger::Char('?'),
        "backslash" => KeyTrigger::Char('\\'),
        "grave" | "backtick" => KeyTrigger::Char('`'),
        "zero" => KeyTrigger::Char('0'),
        "one" => KeyTrigger::Char('1'),
        "two" => KeyTrigger::Char('2'),
        "three" => KeyTrigger::Char('3'),
        "four" => KeyTrigger::Char('4'),
        "five" => KeyTrigger::Char('5'),
        "six" => KeyTrigger::Char('6'),
        "seven" => KeyTrigger::Char('7'),
        "eight" => KeyTrigger::Char('8'),
        "nine" => KeyTrigger::Char('9'),
        _ => {
            let mut chars = lower.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyTrigger::Char(c)
        }
    };

    Some((mods, trigger))
}

fn parse_action(rhs: &str) -> Option<Action> {
    let (name, arg) = match rhs.split_once(':') {
        Some((n, a)) => (n.trim().to_ascii_lowercase(), Some(a.trim())),
        None => (rhs.trim().to_ascii_lowercase(), None),
    };
    match name.as_str() {
        "copy_to_clipboard" | "copy" => Some(Action::CopyToClipboard),
        "paste_from_clipboard" | "paste" => Some(Action::PasteFromClipboard),
        "increase_font_size" => {
            let step = arg.and_then(|a| a.parse::<f32>().ok()).unwrap_or(1.0);
            Some(Action::IncreaseFontSize(step))
        }
        "decrease_font_size" => {
            let step = arg.and_then(|a| a.parse::<f32>().ok()).unwrap_or(1.0);
            Some(Action::DecreaseFontSize(step))
        }
        "reset_font_size" => Some(Action::ResetFontSize),
        "scroll_page_up" => Some(Action::ScrollPageUp),
        "scroll_page_down" => Some(Action::ScrollPageDown),
        "scroll_to_top" => Some(Action::ScrollToTop),
        "scroll_to_bottom" => Some(Action::ScrollToBottom),
        "scroll_page_lines" => {
            let n = arg?.parse::<i32>().ok()?;
            Some(Action::ScrollPageLines(n))
        }
        "clear_screen" => Some(Action::ClearScreen),
        "reload_config" => Some(Action::ReloadConfig),
        "new_tab" => Some(Action::NewTab),
        "new_split" | "split" => {
            let dir = match arg.map(|a| a.to_ascii_lowercase()).as_deref() {
                None | Some("right") | Some("vertical") | Some("v") => SplitDirection::Right,
                Some("down") | Some("bottom") | Some("horizontal") | Some("h") => {
                    SplitDirection::Down
                }
                Some("left") => SplitDirection::Left,
                Some("up") | Some("top") => SplitDirection::Up,
                Some("auto") => SplitDirection::Auto,
                _ => return None,
            };
            Some(Action::NewSplit(dir))
        }
        "split_right" => Some(Action::NewSplit(SplitDirection::Right)),
        "split_down" => Some(Action::NewSplit(SplitDirection::Down)),
        "split_left" => Some(Action::NewSplit(SplitDirection::Left)),
        "split_up" => Some(Action::NewSplit(SplitDirection::Up)),
        "goto_split" | "focus_split" => {
            let target = match arg.map(|a| a.to_ascii_lowercase()).as_deref() {
                Some("left") => SplitFocusTarget::Left,
                Some("right") => SplitFocusTarget::Right,
                Some("up") | Some("top") => SplitFocusTarget::Up,
                Some("down") | Some("bottom") => SplitFocusTarget::Down,
                Some("next") => SplitFocusTarget::Next,
                Some("previous") | Some("prev") => SplitFocusTarget::Previous,
                _ => return None,
            };
            Some(Action::GotoSplit(target))
        }
        "resize_split" => {
            let raw = arg?;
            let (dir_str, amt_str) = match raw.split_once(',').or_else(|| raw.split_once(':')) {
                Some((d, a)) => (d.trim().to_ascii_lowercase(), Some(a.trim())),
                None => (raw.trim().to_ascii_lowercase(), None),
            };
            let dir = match dir_str.as_str() {
                "left" => SplitDirection::Left,
                "right" => SplitDirection::Right,
                "up" | "top" => SplitDirection::Up,
                "down" | "bottom" => SplitDirection::Down,
                _ => return None,
            };
            let px = amt_str
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(36)
                .clamp(4, 500);
            Some(Action::ResizeSplit(dir, px))
        }
        "equalize_splits" => Some(Action::EqualizeSplits),
        "close_surface" | "close_window" => Some(Action::CloseSurface),
        "close_tab" => Some(Action::CloseTab),
        "next_tab" => Some(Action::NextTab),
        "previous_tab" | "prev_tab" => Some(Action::PreviousTab),
        "goto_tab" => {
            let idx = arg?.parse::<usize>().ok()?;
            Some(Action::GotoTab(idx.saturating_sub(1)))
        }
        "new_window" => Some(Action::NewWindow),
        "toggle_fullscreen" => Some(Action::ToggleFullscreen),
        "toggle_window_decorations" | "toggle_titlebar" | "toggle_title_bar" => {
            Some(Action::ToggleWindowDecorations)
        }
        "toggle_status_bar" | "toggle_shortcut_bar" | "toggle_statusbar" => {
            Some(Action::ToggleStatusBar)
        }
        "toggle_context_menu"
        | "show_context_menu"
        | "context_menu"
        | "open_menu"
        | "command_palette"
        | "menu" => Some(Action::ToggleContextMenu),
        "toggle_myanmar_zawgyi" | "toggle_zawgyi" => Some(Action::ToggleMyanmarZawgyi),
        "set_theme" | "switch_theme" | "theme" => {
            let name = arg?.trim();
            if name.is_empty() {
                None
            } else {
                Some(Action::SetTheme(name.to_string()))
            }
        }
        "next_theme" | "cycle_theme" => Some(Action::NextTheme),
        "show_help" | "toggle_help" | "help" => Some(Action::ShowHelp),
        "edit_config" | "edit-config" | "open_config" | "open-config" => Some(Action::EditConfig),
        "update" | "check_update" | "check-update" | "trigger_update" => {
            Some(Action::TriggerUpdate)
        }
        "toggle_editor" | "editor" | "code_editor" => Some(Action::ToggleEditor),
        "toggle_split_mode" | "split_mode" => Some(Action::ToggleSplitMode),
        "quit" => Some(Action::Quit),
        "text" => Some(Action::SendText(unescape_bytes(arg.unwrap_or("")))),
        "csi" => {
            let mut v = b"\x1b[".to_vec();
            v.extend_from_slice(arg.unwrap_or("").as_bytes());
            Some(Action::SendText(v))
        }
        "esc" => {
            let mut v = b"\x1b".to_vec();
            v.extend_from_slice(arg.unwrap_or("").as_bytes());
            Some(Action::SendText(v))
        }
        "ignore" => Some(Action::Ignore),
        "unbind" => Some(Action::Unbind),
        _ => None,
    }
}

fn unescape_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => {
                    out.push(b'\n');
                    i += 2;
                }
                b'r' => {
                    out.push(b'\r');
                    i += 2;
                }
                b't' => {
                    out.push(b'\t');
                    i += 2;
                }
                b'e' => {
                    out.push(0x1b);
                    i += 2;
                }
                b'\\' => {
                    out.push(b'\\');
                    i += 2;
                }
                b'x' if i + 3 < bytes.len() => {
                    if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 2..i + 4])
                        && let Ok(v) = u8::from_str_radix(hex_str, 16)
                    {
                        out.push(v);
                        i += 4;
                        continue;
                    }
                    out.push(bytes[i]);
                    i += 1;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

pub fn apply_keybind_list(list: &mut Vec<Keybind>, kb: Keybind) {
    // Remove existing identical trigger so later config lines override earlier ones
    list.retain(|existing| !(existing.mods == kb.mods && existing.key == kb.key));
    if kb.action != Action::Unbind {
        list.push(kb);
    }
}

pub fn match_keybind<'a>(
    binds: &'a [Keybind],
    logical_key: &Key,
    mods: ModifiersState,
) -> Option<&'a Action> {
    let cur_mods = ModMask::from_winit(mods);
    for b in binds.iter().rev() {
        match &b.key {
            KeyTrigger::Named(named) => {
                if let Key::Named(k) = logical_key
                    && k == named
                    && b.mods == cur_mods
                {
                    return Some(&b.action);
                }
            }
            KeyTrigger::Char(ch) => {
                if let Key::Character(s) = logical_key {
                    let mut chars = s.chars();
                    if let Some(pressed) = chars.next()
                        && chars.next().is_none()
                    {
                        let pressed_lower = pressed.to_ascii_lowercase();
                        // Note: On Shift+Equal -> logical_key may be `+` with shift=true!
                        // Support both exact mod match and shifted symbol match (e.g. `ctrl+plus` when user presses Ctrl+Shift+=)
                        if pressed_lower == *ch && b.mods == cur_mods {
                            return Some(&b.action);
                        }
                        if !pressed.is_ascii_alphabetic()
                            && pressed_lower == *ch
                            && b.mods.ctrl == cur_mods.ctrl
                            && b.mods.alt == cur_mods.alt
                            && b.mods.super_key == cur_mods.super_key
                            && !b.mods.shift
                            && cur_mods.shift
                        {
                            return Some(&b.action);
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn format_keybind(kb: &Keybind) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(5);
    if kb.mods.ctrl {
        parts.push("Ctrl".to_string());
    }
    if kb.mods.alt {
        parts.push("Alt".to_string());
    }
    if kb.mods.shift {
        parts.push("Shift".to_string());
    }
    if kb.mods.super_key {
        parts.push("Super".to_string());
    }
    let key_str = match &kb.key {
        KeyTrigger::Char(' ') => "Space".to_string(),
        KeyTrigger::Char(ch) => ch.to_ascii_uppercase().to_string(),
        KeyTrigger::Named(named) => match named {
            NamedKey::Enter => "Enter",
            NamedKey::Tab => "Tab",
            NamedKey::Space => "Space",
            NamedKey::Backspace => "Bksp",
            NamedKey::Escape => "Esc",
            NamedKey::Insert => "Ins",
            NamedKey::Delete => "Del",
            NamedKey::Home => "Home",
            NamedKey::End => "End",
            NamedKey::PageUp => "PgUp",
            NamedKey::PageDown => "PgDn",
            NamedKey::ArrowUp => "Up",
            NamedKey::ArrowDown => "Down",
            NamedKey::ArrowLeft => "Left",
            NamedKey::ArrowRight => "Right",
            NamedKey::ContextMenu => "Menu",
            NamedKey::F1 => "F1",
            NamedKey::F2 => "F2",
            NamedKey::F3 => "F3",
            NamedKey::F4 => "F4",
            NamedKey::F5 => "F5",
            NamedKey::F6 => "F6",
            NamedKey::F7 => "F7",
            NamedKey::F8 => "F8",
            NamedKey::F9 => "F9",
            NamedKey::F10 => "F10",
            NamedKey::F11 => "F11",
            NamedKey::F12 => "F12",
            _ => "Key",
        }
        .to_string(),
    };
    parts.push(key_str);
    parts.join("+")
}

pub fn shortcut_for_action(binds: &[Keybind], target: &Action) -> Option<String> {
    binds
        .iter()
        .rev()
        .find(|b| &b.action == target)
        .map(format_keybind)
}
