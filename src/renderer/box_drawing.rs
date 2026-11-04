use crate::renderer::QuadInstance;

#[inline]
pub fn is_procedural_box_char(c: char) -> bool {
    matches!(c as u32, 0x2500..=0x259F)
}

/// Emit crisp cell-aligned solid quads for `U+2500..=U+259F` Box Drawing & Block Elements.
/// Returns `true` if handled procedurally; `false` to fall back to font glyph.
pub fn emit_box_char(
    c: char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    out: &mut Vec<QuadInstance>,
) -> bool {
    let thin = (w.min(h) * 0.11).round().max(1.0);
    let heavy = (thin * 2.0).max(2.0);
    let cx = (x + (w - thin) * 0.5).round();
    let cy = (y + (h - thin) * 0.5).round();
    let hcx = (x + (w - heavy) * 0.5).round();
    let hcy = (y + (h - heavy) * 0.5).round();
    let mid_x = cx + thin * 0.5;
    let mid_y = cy + thin * 0.5;

    let mut rect = |rx: f32, ry: f32, rw: f32, rh: f32, col: [f32; 4]| {
        if rw > 0.0 && rh > 0.0 {
            out.push(QuadInstance::solid(rx, ry, rw, rh, col));
        }
    };

    // Helper for 4-arm box characters: (left, right, up, down) weights: 0=none, 1=light, 2=heavy
    let arms: Option<(u8, u8, u8, u8)> = match c {
        '─' | '╌' | '┄' | '┈' | '╭' | '╮' | '╯' | '╰'
            if matches!(c, '─' | '╌' | '┄' | '┈') =>
        {
            Some((1, 1, 0, 0))
        }
        '━' | '╍' | '┅' | '┉' => Some((2, 2, 0, 0)),
        '│' | '╎' | '┆' | '┊' => Some((0, 0, 1, 1)),
        '┃' | '╏' | '┇' | '┋' => Some((0, 0, 2, 2)),
        '┌' | '╭' => Some((0, 1, 0, 1)),
        '┍' => Some((0, 2, 0, 1)),
        '┎' => Some((0, 1, 0, 2)),
        '┏' => Some((0, 2, 0, 2)),
        '┐' | '╮' => Some((1, 0, 0, 1)),
        '┑' => Some((2, 0, 0, 1)),
        '┒' => Some((1, 0, 0, 2)),
        '┓' => Some((2, 0, 0, 2)),
        '└' | '╰' => Some((0, 1, 1, 0)),
        '┕' => Some((0, 2, 1, 0)),
        '┖' => Some((0, 1, 2, 0)),
        '┗' => Some((0, 2, 2, 0)),
        '┘' | '╯' => Some((1, 0, 1, 0)),
        '┙' => Some((2, 0, 1, 0)),
        '┚' => Some((1, 0, 2, 0)),
        '┛' => Some((2, 0, 2, 0)),
        '├' => Some((0, 1, 1, 1)),
        '┝' => Some((0, 2, 1, 1)),
        '┠' => Some((0, 1, 2, 2)),
        '┣' => Some((0, 2, 2, 2)),
        '┤' => Some((1, 0, 1, 1)),
        '┥' => Some((2, 0, 1, 1)),
        '┨' => Some((1, 0, 2, 2)),
        '┫' => Some((2, 0, 2, 2)),
        '┬' => Some((1, 1, 0, 1)),
        '┳' => Some((2, 2, 0, 2)),
        '┴' => Some((1, 1, 1, 0)),
        '┻' => Some((2, 2, 2, 0)),
        '┼' => Some((1, 1, 1, 1)),
        '╋' => Some((2, 2, 2, 2)),
        '╴' => Some((1, 0, 0, 0)),
        '╵' => Some((0, 0, 1, 0)),
        '╶' => Some((0, 1, 0, 0)),
        '╷' => Some((0, 0, 0, 1)),
        '╸' => Some((2, 0, 0, 0)),
        '╹' => Some((0, 0, 2, 0)),
        '╺' => Some((0, 2, 0, 0)),
        '╻' => Some((0, 0, 0, 2)),
        '╼' => Some((1, 2, 0, 0)),
        '╽' => Some((0, 0, 1, 2)),
        '╾' => Some((2, 1, 0, 0)),
        '╿' => Some((0, 0, 2, 1)),
        _ => None,
    };

    if let Some((l, r, u, d)) = arms {
        if l > 0 {
            let t = if l == 2 { heavy } else { thin };
            let py = if l == 2 { hcy } else { cy };
            rect(x, py, (mid_x - x + t * 0.5).ceil(), t, color);
        }
        if r > 0 {
            let t = if r == 2 { heavy } else { thin };
            let py = if r == 2 { hcy } else { cy };
            let sx = (mid_x - t * 0.5).floor();
            rect(sx, py, x + w - sx, t, color);
        }
        if u > 0 {
            let t = if u == 2 { heavy } else { thin };
            let px = if u == 2 { hcx } else { cx };
            rect(px, y, t, (mid_y - y + t * 0.5).ceil(), color);
        }
        if d > 0 {
            let t = if d == 2 { heavy } else { thin };
            let px = if d == 2 { hcx } else { cx };
            let sy = (mid_y - t * 0.5).floor();
            rect(px, sy, t, y + h - sy, color);
        }
        return true;
    }

    // Double-line box drawing (`═ ║ ╔ ╗ ╚ ╝ ╠ ╣ ╦ ╩ ╬`)
    let gap = thin.max(1.0);
    match c {
        '═' => {
            rect(x, cy - gap, w, thin, color);
            rect(x, cy + gap, w, thin, color);
            return true;
        }
        '║' => {
            rect(cx - gap, y, thin, h, color);
            rect(cx + gap, y, thin, h, color);
            return true;
        }
        '╔' => {
            rect(cx - gap, cy - gap, x + w - (cx - gap), thin, color);
            rect(cx + gap, cy + gap, x + w - (cx + gap), thin, color);
            rect(cx - gap, cy - gap, thin, y + h - (cy - gap), color);
            rect(cx + gap, cy + gap, thin, y + h - (cy + gap), color);
            return true;
        }
        '╗' => {
            rect(x, cy - gap, cx + gap + thin - x, thin, color);
            rect(x, cy + gap, cx - gap + thin - x, thin, color);
            rect(cx + gap, cy - gap, thin, y + h - (cy - gap), color);
            rect(cx - gap, cy + gap, thin, y + h - (cy + gap), color);
            return true;
        }
        '╚' => {
            rect(cx - gap, y, thin, cy + gap + thin - y, color);
            rect(cx + gap, y, thin, cy - gap + thin - y, color);
            rect(cx - gap, cy + gap, x + w - (cx - gap), thin, color);
            rect(cx + gap, cy - gap, x + w - (cx + gap), thin, color);
            return true;
        }
        '╝' => {
            rect(cx + gap, y, thin, cy + gap + thin - y, color);
            rect(cx - gap, y, thin, cy - gap + thin - y, color);
            rect(x, cy + gap, cx + gap + thin - x, thin, color);
            rect(x, cy - gap, cx - gap + thin - x, thin, color);
            return true;
        }
        '╠' | '╣' | '╦' | '╩' | '╬' => {
            if matches!(c, '╠' | '╣' | '╬') {
                rect(cx - gap, y, thin, h, color);
                rect(cx + gap, y, thin, h, color);
            }
            if matches!(c, '╦' | '╩' | '╬') {
                rect(x, cy - gap, w, thin, color);
                rect(x, cy + gap, w, thin, color);
            }
            if c == '╠' {
                rect(cx + gap, cy - gap, x + w - (cx + gap), thin, color);
                rect(cx + gap, cy + gap, x + w - (cx + gap), thin, color);
            } else if c == '╣' {
                rect(x, cy - gap, cx - gap + thin - x, thin, color);
                rect(x, cy + gap, cx - gap + thin - x, thin, color);
            } else if c == '╦' {
                rect(cx - gap, cy + gap, thin, y + h - (cy + gap), color);
                rect(cx + gap, cy + gap, thin, y + h - (cy + gap), color);
            } else if c == '╩' {
                rect(cx - gap, y, thin, cy - gap + thin - y, color);
                rect(cx + gap, y, thin, cy - gap + thin - y, color);
            }
            return true;
        }
        _ => {}
    }

    // Block Elements (`U+2580..=U+259F`)
    let code = c as u32;
    match code {
        0x2580 => {
            // ▀ Upper half block
            rect(x, y, w, (h * 0.5).round(), color);
            true
        }
        0x2581..=0x2588 => {
            // ▁..█ Lower 1/8..8/8 blocks
            let frac = (code - 0x2580) as f32 / 8.0;
            let bh = (h * frac).round();
            rect(x, y + h - bh, w, bh, color);
            true
        }
        0x2589..=0x258F => {
            // ▉..▏ Left 7/8..1/8 blocks
            let frac = (0x2590 - code) as f32 / 8.0;
            let bw = (w * frac).round();
            rect(x, y, bw, h, color);
            true
        }
        0x2590 => {
            // ▐ Right half block
            let bw = (w * 0.5).round();
            rect(x + w - bw, y, bw, h, color);
            true
        }
        0x2591..=0x2593 => {
            // ░ ▒ ▓ Light / Medium / Dark shade
            let alpha_mul = match code {
                0x2591 => 0.25,
                0x2592 => 0.50,
                _ => 0.75,
            };
            let mut c2 = color;
            c2[3] *= alpha_mul;
            rect(x, y, w, h, c2);
            true
        }
        0x2594 => {
            // ▔ Upper 1/8 block
            rect(x, y, w, (h / 8.0).round().max(1.0), color);
            true
        }
        0x2595 => {
            // ▕ Right 1/8 block
            let bw = (w / 8.0).round().max(1.0);
            rect(x + w - bw, y, bw, h, color);
            true
        }
        0x2596..=0x259F => {
            // Quadrant blocks (▖..▟): bitmask (UL=1, UR=2, LL=4, LR=8)
            let mask: u8 = match code {
                0x2596 => 0b0100, // LL
                0x2597 => 0b1000, // LR
                0x2598 => 0b0001, // UL
                0x2599 => 0b1101, // UL+LL+LR
                0x259A => 0b1001, // UL+LR
                0x259B => 0b0111, // UL+UR+LL
                0x259C => 0b1011, // UL+UR+LR
                0x259D => 0b0010, // UR
                0x259E => 0b0110, // UR+LL
                _ => 0b1110,      // UR+LL+LR
            };
            let hw = (w * 0.5).round();
            let hh = (h * 0.5).round();
            if mask & 1 != 0 {
                rect(x, y, hw, hh, color);
            }
            if mask & 2 != 0 {
                rect(x + hw, y, w - hw, hh, color);
            }
            if mask & 4 != 0 {
                rect(x, y + hh, hw, h - hh, color);
            }
            if mask & 8 != 0 {
                rect(x + hw, y + hh, w - hw, h - hh, color);
            }
            true
        }
        _ => {
            // Remaining mixed light/heavy box chars fall back to cross or font
            rect(x, cy, w, thin, color);
            rect(cx, y, thin, h, color);
            true
        }
    }
}
