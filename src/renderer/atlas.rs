use rustc_hash::FxHashMap;
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;

use crate::font::FontSystem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtlasKey {
    pub font_id: u32,
    pub glyph_id: u16,
    /// Font size in 1/16ths of a pixel
    pub size_q4: u16,
    pub thicken: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AtlasEntry {
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub is_color: bool,
    pub atlas_x: u32,
    pub atlas_y: u32,
}

pub struct GlyphAtlas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    entries: FxHashMap<AtlasKey, AtlasEntry>,
    scale_ctx: ScaleContext,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    pub dirty_rows: Option<(u32, u32)>,
    pub resized: bool,
}

impl GlyphAtlas {
    pub fn new(initial_size: u32) -> Self {
        let sz = initial_size.max(512);
        Self {
            width: sz,
            height: sz,
            pixels: vec![0u8; (sz * sz * 4) as usize],
            entries: FxHashMap::with_capacity_and_hasher(1024, Default::default()),
            scale_ctx: ScaleContext::new(),
            cursor_x: 1,
            cursor_y: 1,
            row_height: 0,
            dirty_rows: Some((0, sz)),
            resized: true,
        }
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
        self.entries.clear();
        self.cursor_x = 1;
        self.cursor_y = 1;
        self.row_height = 0;
        self.dirty_rows = Some((0, self.height));
    }

    pub fn get_or_rasterize(
        &mut self,
        font_sys: &FontSystem,
        font_id: usize,
        glyph_id: u16,
        font_px: f32,
        thicken: bool,
    ) -> AtlasEntry {
        let size_q4 = (font_px * 16.0).round().clamp(64.0, 4096.0) as u16;
        let key = AtlasKey {
            font_id: font_id as u32,
            glyph_id,
            size_q4,
            thicken,
        };
        if let Some(&entry) = self.entries.get(&key) {
            return entry;
        }

        let loaded = font_sys.font(font_id);
        let font_ref = loaded.swash_ref();
        let px = size_q4 as f32 / 16.0;

        let mut scaler = self.scale_ctx.builder(font_ref).size(px).hint(true).build();

        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .render(&mut scaler, glyph_id);

        let Some(img) = image else {
            let empty = AtlasEntry {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_w: 0.0,
                uv_h: 0.0,
                left: 0,
                top: 0,
                width: 0,
                height: 0,
                is_color: false,
                atlas_x: 0,
                atlas_y: 0,
            };
            self.entries.insert(key, empty);
            return empty;
        };

        let gw = img.placement.width;
        let gh = img.placement.height;
        if gw == 0 || gh == 0 {
            let empty = AtlasEntry {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_w: 0.0,
                uv_h: 0.0,
                left: img.placement.left,
                top: img.placement.top,
                width: 0,
                height: 0,
                is_color: false,
                atlas_x: 0,
                atlas_y: 0,
            };
            self.entries.insert(key, empty);
            return empty;
        }

        let (ax, ay) = self.alloc_rect(gw, gh);
        let is_color = matches!(img.content, Content::Color);

        for row in 0..gh {
            let dst_y = ay + row;
            let dst_row_start = ((dst_y * self.width + ax) * 4) as usize;
            match img.content {
                Content::Mask => {
                    let src_start = (row * gw) as usize;
                    for col in 0..gw as usize {
                        let mut a = img.data[src_start + col];
                        if thicken && a > 0 && a < 255 {
                            a = ((a as f32 / 255.0).powf(0.75) * 255.0).round() as u8;
                        }
                        let di = dst_row_start + col * 4;
                        self.pixels[di] = 255;
                        self.pixels[di + 1] = 255;
                        self.pixels[di + 2] = 255;
                        self.pixels[di + 3] = a;
                    }
                }
                Content::Color => {
                    let src_start = (row * gw * 4) as usize;
                    let byte_len = (gw * 4) as usize;
                    self.pixels[dst_row_start..dst_row_start + byte_len]
                        .copy_from_slice(&img.data[src_start..src_start + byte_len]);
                }
                Content::SubpixelMask => {
                    let src_start = (row * gw * 4) as usize;
                    for col in 0..gw as usize {
                        let si = src_start + col * 4;
                        let lum = ((img.data[si] as u16
                            + img.data[si + 1] as u16
                            + img.data[si + 2] as u16)
                            / 3) as u8;
                        let di = dst_row_start + col * 4;
                        self.pixels[di] = 255;
                        self.pixels[di + 1] = 255;
                        self.pixels[di + 2] = 255;
                        self.pixels[di + 3] = lum;
                    }
                }
            }
        }

        self.mark_dirty(ay, ay + gh);

        let entry = AtlasEntry {
            uv_x: ax as f32 / self.width as f32,
            uv_y: ay as f32 / self.height as f32,
            uv_w: gw as f32 / self.width as f32,
            uv_h: gh as f32 / self.height as f32,
            left: img.placement.left,
            top: img.placement.top,
            width: gw,
            height: gh,
            is_color,
            atlas_x: ax,
            atlas_y: ay,
        };
        self.entries.insert(key, entry);
        entry
    }

    fn alloc_rect(&mut self, w: u32, h: u32) -> (u32, u32) {
        let padded_w = w + 2;
        let padded_h = h + 2;

        if self.cursor_x + padded_w > self.width {
            self.cursor_x = 1;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }

        if self.cursor_y + padded_h > self.height {
            if self.width < 4096 {
                self.grow();
                return self.alloc_rect(w, h);
            } else {
                // Atlas is 4096x4096 and full; reset cleanly
                self.clear();
            }
        }

        let x = self.cursor_x;
        let y = self.cursor_y;
        self.cursor_x += padded_w;
        self.row_height = self.row_height.max(padded_h);
        (x, y)
    }

    fn grow(&mut self) {
        let new_w = (self.width * 2).min(4096);
        let new_h = (self.height * 2).min(4096);
        let mut new_pixels = vec![0u8; (new_w * new_h * 4) as usize];
        for row in 0..self.height {
            let src = (row * self.width * 4) as usize;
            let dst = (row * new_w * 4) as usize;
            let len = (self.width * 4) as usize;
            new_pixels[dst..dst + len].copy_from_slice(&self.pixels[src..src + len]);
        }
        self.width = new_w;
        self.height = new_h;
        self.pixels = new_pixels;
        for entry in self.entries.values_mut() {
            if entry.width > 0 {
                entry.uv_x = entry.atlas_x as f32 / new_w as f32;
                entry.uv_y = entry.atlas_y as f32 / new_h as f32;
                entry.uv_w = entry.width as f32 / new_w as f32;
                entry.uv_h = entry.height as f32 / new_h as f32;
            }
        }
        self.resized = true;
        self.dirty_rows = Some((0, new_h));
    }

    fn mark_dirty(&mut self, y0: u32, y1: u32) {
        self.dirty_rows = Some(match self.dirty_rows {
            Some((a, b)) => (a.min(y0), b.max(y1)),
            None => (y0, y1),
        });
    }
}
