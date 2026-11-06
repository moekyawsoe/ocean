#!/usr/bin/env python3
"""Generate Ocean editor UI icons (16px and 32px PNGs) with no dependencies."""

import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "icons", "ui")
SIZES = (16, 32)
SS = 4  # supersampling factor


def png_bytes(width, height, rgba):
    raw = bytearray()
    stride = width * 4
    for y in range(height):
        raw.append(0)
        raw.extend(rgba[y * stride : (y + 1) * stride])

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


class Canvas:
    def __init__(self, size):
        self.size = size
        self.n = size * SS
        self.s = self.n / 16.0
        self.px = [[0.0, 0.0, 0.0, 0.0] for _ in range(self.n * self.n)]

    def blend(self, x, y, r, g, b, a):
        if a <= 0 or x < 0 or y < 0 or x >= self.n or y >= self.n:
            return
        p = self.px[y * self.n + x]
        sr, sg, sb = r * a, g * a, b * a
        da = p[3]
        oa = a + da * (1.0 - a)
        if oa <= 0:
            return
        p[0] = (sr + p[0] * (1.0 - a)) / oa
        p[1] = (sg + p[1] * (1.0 - a)) / oa
        p[2] = (sb + p[2] * (1.0 - a)) / oa
        p[3] = oa

    def mask(self, x, y, a):
        self.blend(x, y, 1.0, 1.0, 1.0, a)

    def rect(self, x, y, w, h, color=(1, 1, 1), alpha=1.0, mask=False):
        x0, y0 = int(round(x * self.s)), int(round(y * self.s))
        x1, y1 = int(round((x + w) * self.s)), int(round((y + h) * self.s))
        for py in range(max(0, y0), min(self.n, y1)):
            for px in range(max(0, x0), min(self.n, x1)):
                if mask:
                    self.mask(px, py, alpha)
                else:
                    self.blend(px, py, *color, alpha)

    def line(self, x0, y0, x1, y1, width=1.0, color=(1, 1, 1), alpha=1.0, mask=False):
        ax, ay = x0 * self.s, y0 * self.s
        bx, by = x1 * self.s, y1 * self.s
        r = width * self.s * 0.5
        minx = max(0, int(min(ax, bx) - r - 1))
        maxx = min(self.n - 1, int(max(ax, bx) + r + 1))
        miny = max(0, int(min(ay, by) - r - 1))
        maxy = min(self.n - 1, int(max(ay, by) + r + 1))
        dx, dy = bx - ax, by - ay
        length2 = dx * dx + dy * dy
        for py in range(miny, maxy + 1):
            for px in range(minx, maxx + 1):
                cx, cy = px + 0.5, py + 0.5
                if length2 == 0:
                    dist = ((cx - ax) ** 2 + (cy - ay) ** 2) ** 0.5
                else:
                    t = max(0.0, min(1.0, ((cx - ax) * dx + (cy - ay) * dy) / length2))
                    dist = ((cx - (ax + t * dx)) ** 2 + (cy - (ay + t * dy)) ** 2) ** 0.5
                cov = max(0.0, min(1.0, r - dist + 0.5))
                if cov > 0:
                    if mask:
                        self.mask(px, py, cov * alpha)
                    else:
                        self.blend(px, py, *color, cov * alpha)

    def polyline(self, pts, width=1.0, color=(1, 1, 1), alpha=1.0, mask=False, closed=False):
        seq = list(pts) + ([pts[0]] if closed else [])
        for a, b in zip(seq, seq[1:]):
            self.line(a[0], a[1], b[0], b[1], width, color, alpha, mask)

    def circle(self, cx, cy, r, width=None, color=(1, 1, 1), alpha=1.0, mask=False):
        ccx, ccy, rr = cx * self.s, cy * self.s, r * self.s
        wr = (width * self.s * 0.5) if width else None
        minx = max(0, int(ccx - rr - 1))
        maxx = min(self.n - 1, int(ccx + rr + 1))
        miny = max(0, int(ccy - rr - 1))
        maxy = min(self.n - 1, int(ccy + rr + 1))
        for py in range(miny, maxy + 1):
            for px in range(minx, maxx + 1):
                d = ((px + 0.5 - ccx) ** 2 + (py + 0.5 - ccy) ** 2) ** 0.5
                if wr is None:
                    cov = max(0.0, min(1.0, rr - d + 0.5))
                else:
                    cov = max(0.0, min(1.0, wr - abs(d - rr) + 0.5))
                if cov > 0:
                    if mask:
                        self.mask(px, py, cov * alpha)
                    else:
                        self.blend(px, py, *color, cov * alpha)

    def triangle(self, pts, width=1.0, color=(1, 1, 1), alpha=1.0, mask=False):
        self.polyline(pts, width, color, alpha, mask, closed=True)

    def rounded_rect(self, x, y, w, h, radius, color=(1, 1, 1), alpha=1.0, mask=False):
        self.rect(x + radius, y, w - 2 * radius, h, color, alpha, mask)
        self.rect(x, y + radius, w, h - 2 * radius, color, alpha, mask)
        for cx, cy in (
            (x + radius, y + radius),
            (x + w - radius, y + radius),
            (x + radius, y + h - radius),
            (x + w - radius, y + h - radius),
        ):
            self.circle(cx, cy, radius, None, color, alpha, mask)

    def to_png(self):
        out = bytearray(self.size * self.size * 4)
        scale = SS
        for y in range(self.size):
            for x in range(self.size):
                r = g = b = a = 0.0
                for oy in range(scale):
                    for ox in range(scale):
                        p = self.px[(y * scale + oy) * self.n + (x * scale + ox)]
                        pa = p[3]
                        r += p[0] * pa
                        g += p[1] * pa
                        b += p[2] * pa
                        a += pa
                count = scale * scale
                a /= count
                if a > 0:
                    r /= a * count
                    g /= a * count
                    b /= a * count
                idx = (y * self.size + x) * 4
                out[idx] = max(0, min(255, int(round(r * 255))))
                out[idx + 1] = max(0, min(255, int(round(g * 255))))
                out[idx + 2] = max(0, min(255, int(round(b * 255))))
                out[idx + 3] = max(0, min(255, int(round(a * 255))))
        return bytes(out)


FONT = {
    "R": ["110", "101", "110", "101", "101"],
    "T": ["111", "010", "010", "010", "010"],
    "S": ["011", "100", "010", "001", "110"],
    "J": ["001", "001", "001", "101", "010"],
    "P": ["110", "101", "110", "100", "100"],
    "Y": ["101", "101", "010", "010", "010"],
    "O": ["010", "101", "101", "101", "010"],
    "C": ["011", "100", "100", "100", "011"],
    "G": ["011", "100", "101", "101", "011"],
    "H": ["101", "101", "111", "101", "101"],
    "M": ["101", "111", "111", "101", "101"],
    "D": ["110", "101", "101", "101", "110"],
    "A": ["010", "101", "111", "101", "101"],
    "+": ["000", "010", "111", "010", "000"],
    "<": ["001", "010", "100", "010", "001"],
    ">": ["100", "010", "001", "010", "100"],
    "#": ["101", "111", "101", "111", "101"],
    "{": ["001", "010", "010", "010", "001"],
    "}": ["100", "010", "010", "010", "100"],
    "$": ["011", "110", "010", "011", "110"],
    "_": ["000", "000", "000", "000", "111"],
}


def draw_text(c, text, x, y, color, cell=1.25):
    for ch in text:
        glyph = FONT.get(ch)
        if not glyph:
            continue
        for gy, row in enumerate(glyph):
            for gx, bit in enumerate(row):
                if bit == "1":
                    c.rect(x + gx * cell, y + gy * cell, cell, cell, color, 1.0)
        x += 4 * cell


def mono_icon(name, draw, size):
    c = Canvas(size)
    draw(c)
    with open(os.path.join(OUT, f"{name}-{size}.png"), "wb") as f:
        f.write(png_bytes(size, size, c.to_png()))


def file_icon(name, letters, color, label_color, size):
    c = Canvas(size)
    c.rounded_rect(1.0, 1.0, 14.0, 14.0, 3.0, color, 1.0)
    if letters:
        cell = 1.15 if len(letters) <= 1 else 0.95
        width = len(letters) * 4 * cell - cell
        draw_text(c, letters, (16 - width) / 2, (16 - 5 * cell) / 2, label_color, cell)
    with open(os.path.join(OUT, f"{name}-{size}.png"), "wb") as f:
        f.write(png_bytes(size, size, c.to_png()))


def draw_explorer(c):
    c.polyline([(2, 5), (2, 3.5), (6.5, 3.5), (8, 5), (14, 5), (14, 13), (2, 13)], 1.2, mask=True, closed=True)


def draw_search(c):
    c.circle(7, 7, 4.0, 1.4, mask=True)
    c.line(10.0, 10.0, 13.5, 13.5, 1.8, mask=True)


def draw_problems(c):
    c.triangle([(8, 1.8), (14.5, 14.0), (1.5, 14.0)], 1.2, mask=True)
    c.line(8, 6.0, 8, 9.8, 1.4, mask=True)
    c.circle(8, 12.0, 0.8, None, mask=True)


def draw_plugins(c):
    c.rounded_rect(4.8, 6.0, 6.4, 6.2, 1.2, mask=True)
    c.line(6.5, 3.2, 6.5, 6.0, 1.4, mask=True)
    c.line(9.5, 3.2, 9.5, 6.0, 1.4, mask=True)
    c.line(8.0, 12.2, 8.0, 14.6, 1.4, mask=True)


def draw_chevron_right(c):
    c.polyline([(5.5, 3.0), (11.0, 8.0), (5.5, 13.0)], 1.9, mask=True)


def draw_chevron_down(c):
    c.polyline([(3.0, 5.5), (8.0, 11.0), (13.0, 5.5)], 1.9, mask=True)


def draw_close(c):
    c.line(4, 4, 12, 12, 1.6, mask=True)
    c.line(12, 4, 4, 12, 1.6, mask=True)


def draw_plus(c):
    c.line(8, 3, 8, 13, 1.7, mask=True)
    c.line(3, 8, 13, 8, 1.7, mask=True)


def draw_dirty(c):
    c.circle(8, 8, 3.4, None, mask=True)


def draw_file(c):
    c.polyline([(3, 1.5), (9.5, 1.5), (13, 5), (13, 14.5), (3, 14.5)], 1.1, mask=True, closed=True)
    c.polyline([(9.5, 1.5), (9.5, 5), (13, 5)], 1.1, mask=True)


def draw_folder(c):
    c.polyline([(2, 5), (2, 3.5), (6.5, 3.5), (8, 5), (14, 5), (14, 13), (2, 13)], 1.2, mask=True, closed=True)


def draw_folder_open(c):
    c.polyline([(2, 5), (2, 3.5), (6.5, 3.5), (8, 5), (13, 5)], 1.1, mask=True)
    c.polyline([(2, 5), (2, 13), (13, 13), (15, 7), (5, 7)], 1.1, mask=True, closed=True)


def draw_save(c):
    c.polyline([(2.5, 2.5), (11, 2.5), (13.5, 5), (13.5, 13.5), (2.5, 13.5)], 1.1, mask=True, closed=True)
    c.rect(5, 2.8, 6, 3.4, mask=True)
    c.rect(5.2, 8.5, 5.6, 4.7, mask=True)


def draw_refresh(c):
    pts = []
    import math

    for i in range(24):
        a = math.radians(-40 + i * (320 / 23))
        pts.append((8 + 4.6 * math.cos(a), 8 + 4.6 * math.sin(a)))
    c.polyline(pts, 1.4, mask=True)
    c.polyline([(11.8, 2.3), (13.2, 4.8), (10.4, 5.0)], 1.4, mask=True)


def draw_settings(c):
    import math

    c.circle(8, 8, 3.1, 1.3, mask=True)
    for i in range(8):
        a = math.radians(i * 45)
        c.line(8 + 4.0 * math.cos(a), 8 + 4.0 * math.sin(a), 8 + 6.0 * math.cos(a), 8 + 6.0 * math.sin(a), 1.3, mask=True)


def draw_error(c):
    c.circle(8, 8, 5.7, 1.3, mask=True)
    c.line(5.6, 5.6, 10.4, 10.4, 1.4, mask=True)
    c.line(10.4, 5.6, 5.6, 10.4, 1.4, mask=True)


def draw_warning(c):
    c.triangle([(8, 1.8), (14.5, 14.0), (1.5, 14.0)], 1.2, mask=True)
    c.line(8, 6.0, 8, 9.8, 1.4, mask=True)
    c.circle(8, 12.0, 0.8, None, mask=True)


def draw_info(c):
    c.circle(8, 8, 5.7, 1.3, mask=True)
    c.circle(8, 4.8, 0.9, None, mask=True)
    c.line(8, 7.0, 8, 11.6, 1.4, mask=True)


def draw_git(c):
    c.circle(5, 4, 1.8, 1.1, mask=True)
    c.circle(5, 12, 1.8, 1.1, mask=True)
    c.circle(11.5, 8, 1.8, 1.1, mask=True)
    c.line(5, 5.8, 5, 10.2, 1.2, mask=True)
    c.line(5, 8, 9.7, 8, 1.2, mask=True)


def draw_outline(c):
    for i, y in enumerate((3.5, 8, 12.5)):
        c.circle(3.2, y, 0.9, None, mask=True)
        c.line(6.0, y, 13.5, y, 1.2, mask=True)


def draw_terminal(c):
    c.polyline([(3, 4.5), (6.5, 8), (3, 11.5)], 1.5, mask=True)
    c.line(8, 11.5, 13, 11.5, 1.5, mask=True)


def draw_split(c):
    c.polyline([(2, 2), (6.5, 2), (6.5, 14), (2, 14)], 1.1, mask=True, closed=True)
    c.polyline([(9.5, 2), (14, 2), (14, 14), (9.5, 14)], 1.1, mask=True, closed=True)


def draw_check(c):
    c.polyline([(3, 8.2), (6.5, 11.7), (13, 4.2)], 1.8, mask=True)


def draw_plain_file(c):
    draw_file(c)


MONO = {
    "explorer": draw_explorer,
    "search": draw_search,
    "problems": draw_problems,
    "plugins": draw_plugins,
    "chevron-right": draw_chevron_right,
    "chevron-down": draw_chevron_down,
    "close": draw_close,
    "plus": draw_plus,
    "dirty": draw_dirty,
    "file": draw_file,
    "folder": draw_folder,
    "folder-open": draw_folder_open,
    "save": draw_save,
    "refresh": draw_refresh,
    "settings": draw_settings,
    "error": draw_error,
    "warning": draw_warning,
    "info": draw_info,
    "git": draw_git,
    "outline": draw_outline,
    "terminal": draw_terminal,
    "split": draw_split,
    "check": draw_check,
    "plain-file": draw_plain_file,
}

FILE_ICONS = {
    "file-rust": ("R", (0.78, 0.27, 0.18), (1, 1, 1)),
    "file-typescript": ("TS", (0.19, 0.47, 0.78), (1, 1, 1)),
    "file-javascript": ("JS", (0.95, 0.85, 0.12), (0.08, 0.08, 0.08)),
    "file-python": ("PY", (0.22, 0.46, 0.67), (1, 1, 1)),
    "file-go": ("GO", (0.0, 0.68, 0.85), (1, 1, 1)),
    "file-c": ("C", (0.16, 0.28, 0.6), (1, 1, 1)),
    "file-cpp": ("C+", (0.0, 0.35, 0.6), (1, 1, 1)),
    "file-html": ("<>", (0.89, 0.30, 0.15), (1, 1, 1)),
    "file-css": ("C", (0.15, 0.30, 0.89), (1, 1, 1)),
    "file-json": ("{}", (0.22, 0.22, 0.24), (1, 1, 1)),
    "file-toml": ("T", (0.56, 0.36, 0.20), (1, 1, 1)),
    "file-markdown": ("M", (0.13, 0.35, 0.63), (1, 1, 1)),
    "file-shell": ("$_", (0.31, 0.67, 0.15), (1, 1, 1)),
    "file-docker": ("D", (0.15, 0.55, 0.82), (1, 1, 1)),
    "file-plain": ("", (0.48, 0.51, 0.56), (1, 1, 1)),
}


def main():
    os.makedirs(OUT, exist_ok=True)
    for size in SIZES:
        for name, draw in MONO.items():
            mono_icon(name, draw, size)
        for name, (letters, color, label_color) in FILE_ICONS.items():
            file_icon(name, letters, color, label_color, size)
    print(f"Generated {len(MONO) + len(FILE_ICONS)} icons x {len(SIZES)} sizes in {OUT}")


if __name__ == "__main__":
    main()
