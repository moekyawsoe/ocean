use alacritty_terminal::term::TermMode;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Encode a `winit` keyboard press into VT/xterm PTY bytes.
pub fn encode_key_event(
    event: &KeyEvent,
    mods: ModifiersState,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let ctrl = mods.control_key();
    let alt = mods.alt_key();
    let shift = mods.shift_key();
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    // Xterm modifier parameter: 1 + (shift ? 1 : 0) + (alt ? 2 : 0) + (ctrl ? 4 : 0)
    let mod_param = 1 + u8::from(shift) + u8::from(alt) * 2 + u8::from(ctrl) * 4;
    let has_mod = mod_param > 1;

    if let Key::Named(named) = &event.logical_key {
        let seq: Option<Vec<u8>> = match named {
            NamedKey::Enter => {
                if alt {
                    Some(b"\x1b\r".to_vec())
                } else {
                    Some(b"\r".to_vec())
                }
            }
            NamedKey::Backspace => {
                let b = if ctrl { 0x08 } else { 0x7f };
                if alt {
                    Some(vec![0x1b, b])
                } else {
                    Some(vec![b])
                }
            }
            NamedKey::Tab => {
                if shift {
                    Some(b"\x1b[Z".to_vec())
                } else if alt {
                    Some(b"\x1b\t".to_vec())
                } else {
                    Some(b"\t".to_vec())
                }
            }
            NamedKey::Escape => Some(b"\x1b".to_vec()),
            NamedKey::Space => {
                if ctrl {
                    Some(vec![0x00])
                } else if alt {
                    Some(b"\x1b ".to_vec())
                } else {
                    Some(b" ".to_vec())
                }
            }
            NamedKey::ArrowUp => Some(cursor_key(b'A', app_cursor, mod_param)),
            NamedKey::ArrowDown => Some(cursor_key(b'B', app_cursor, mod_param)),
            NamedKey::ArrowRight => Some(cursor_key(b'C', app_cursor, mod_param)),
            NamedKey::ArrowLeft => Some(cursor_key(b'D', app_cursor, mod_param)),
            NamedKey::Home => Some(if has_mod {
                format!("\x1b[1;{mod_param}H").into_bytes()
            } else if app_cursor {
                b"\x1bOH".to_vec()
            } else {
                b"\x1b[H".to_vec()
            }),
            NamedKey::End => Some(if has_mod {
                format!("\x1b[1;{mod_param}F").into_bytes()
            } else if app_cursor {
                b"\x1bOF".to_vec()
            } else {
                b"\x1b[F".to_vec()
            }),
            NamedKey::Insert => Some(tilde_key(2, mod_param)),
            NamedKey::Delete => Some(tilde_key(3, mod_param)),
            NamedKey::PageUp => Some(tilde_key(5, mod_param)),
            NamedKey::PageDown => Some(tilde_key(6, mod_param)),
            NamedKey::F1 => Some(fn_ss3_key(b'P', mod_param)),
            NamedKey::F2 => Some(fn_ss3_key(b'Q', mod_param)),
            NamedKey::F3 => Some(fn_ss3_key(b'R', mod_param)),
            NamedKey::F4 => Some(fn_ss3_key(b'S', mod_param)),
            NamedKey::F5 => Some(tilde_key(15, mod_param)),
            NamedKey::F6 => Some(tilde_key(17, mod_param)),
            NamedKey::F7 => Some(tilde_key(18, mod_param)),
            NamedKey::F8 => Some(tilde_key(19, mod_param)),
            NamedKey::F9 => Some(tilde_key(20, mod_param)),
            NamedKey::F10 => Some(tilde_key(21, mod_param)),
            NamedKey::F11 => Some(tilde_key(23, mod_param)),
            NamedKey::F12 => Some(tilde_key(24, mod_param)),
            _ => None,
        };
        if seq.is_some() {
            return seq;
        }
    }

    // Ctrl + key control codes
    if ctrl && !mods.super_key() {
        if let Key::Character(s) = &event.logical_key
            && let Some(ch) = s.chars().next()
        {
            let lower = ch.to_ascii_lowercase();
            let ctl_byte: Option<u8> = match lower {
                'a'..='z' => Some((lower as u8) - b'a' + 1),
                '@' | '2' => Some(0x00),
                '[' | '3' => Some(0x1b),
                '\\' | '4' => Some(0x1c),
                ']' | '5' => Some(0x1d),
                '^' | '6' => Some(0x1e),
                '_' | '/' | '7' => Some(0x1f),
                '8' => Some(0x7f),
                _ => None,
            };
            if let Some(b) = ctl_byte {
                return if alt {
                    Some(vec![0x1b, b])
                } else {
                    Some(vec![b])
                };
            }
        }
    }

    // Standard UTF-8 character input (covers ASCII, XKB Burmese `mm` layout, Unicode compose!)
    if !ctrl && !mods.super_key() {
        if let Some(txt) = &event.text
            && !txt.is_empty()
        {
            let first = txt.chars().next().unwrap_or('\0');
            if !first.is_control() {
                if alt {
                    let mut out = Vec::with_capacity(1 + txt.len());
                    out.push(0x1b);
                    out.extend_from_slice(txt.as_bytes());
                    return Some(out);
                }
                return Some(txt.as_bytes().to_vec());
            }
        }
        if alt
            && let Key::Character(s) = &event.logical_key
            && !s.is_empty()
        {
            let mut out = Vec::with_capacity(1 + s.len());
            out.push(0x1b);
            out.extend_from_slice(s.as_bytes());
            return Some(out);
        }
    }

    None
}

fn cursor_key(letter: u8, app_cursor: bool, mod_param: u8) -> Vec<u8> {
    if mod_param > 1 {
        format!("\x1b[1;{mod_param}{}", letter as char).into_bytes()
    } else if app_cursor {
        vec![0x1b, b'O', letter]
    } else {
        vec![0x1b, b'[', letter]
    }
}

fn tilde_key(code: u8, mod_param: u8) -> Vec<u8> {
    if mod_param > 1 {
        format!("\x1b[{code};{mod_param}~").into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn fn_ss3_key(letter: u8, mod_param: u8) -> Vec<u8> {
    if mod_param > 1 {
        format!("\x1b[1;{mod_param}{}", letter as char).into_bytes()
    } else {
        vec![0x1b, b'O', letter]
    }
}

pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let safe = text.replace("\x1b[201~", "").replace("\x1b[200~", "");
        let mut out = Vec::with_capacity(safe.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        out.extend_from_slice(safe.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.as_bytes().to_vec()
    }
}

pub fn encode_mouse_button(
    button: MouseButton,
    pressed: bool,
    motion: bool,
    col: usize,
    row: usize,
    mods: ModifiersState,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !mode.intersects(TermMode::MOUSE_MODE) {
        return None;
    }
    if motion && !mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
        return None;
    }

    let mut code: u8 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        _ => 3,
    };
    if motion {
        code += 32;
    }
    if mods.shift_key() {
        code += 4;
    }
    if mods.alt_key() {
        code += 8;
    }
    if mods.control_key() {
        code += 16;
    }

    let cx = col + 1;
    let cy = row + 1;

    if mode.contains(TermMode::SGR_MOUSE) {
        let suffix = if pressed || motion { 'M' } else { 'm' };
        Some(format!("\x1b[<{code};{cx};{cy}{suffix}").into_bytes())
    } else if cx <= 223 && cy <= 223 {
        let btn = if !pressed && !motion { 3 + (code & 28) } else { code };
        Some(vec![
            0x1b,
            b'[',
            b'M',
            32 + btn,
            (32 + cx) as u8,
            (32 + cy) as u8,
        ])
    } else {
        None
    }
}

pub fn encode_mouse_wheel(
    up: bool,
    col: usize,
    row: usize,
    mods: ModifiersState,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if mode.intersects(TermMode::MOUSE_MODE) {
        let mut code: u8 = if up { 64 } else { 65 };
        if mods.shift_key() {
            code += 4;
        }
        if mods.alt_key() {
            code += 8;
        }
        if mods.control_key() {
            code += 16;
        }
        let cx = col + 1;
        let cy = row + 1;
        if mode.contains(TermMode::SGR_MOUSE) {
            return Some(format!("\x1b[<{code};{cx};{cy}M").into_bytes());
        } else if cx <= 223 && cy <= 223 {
            return Some(vec![
                0x1b,
                b'[',
                b'M',
                32 + code,
                (32 + cx) as u8,
                (32 + cy) as u8,
            ]);
        }
    } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
        || mode.contains(TermMode::ALT_SCREEN)
    {
        // In alternate screen pagers (`less`, `man`, `git log`), translate wheel to Up/Down arrows!
        let arrow = if up { b'A' } else { b'B' };
        return Some(cursor_key(arrow, mode.contains(TermMode::APP_CURSOR), 1));
    }
    None
}
