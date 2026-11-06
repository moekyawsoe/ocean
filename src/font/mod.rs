pub mod myanmar;
pub mod shaper;

use std::str::FromStr;
use std::sync::Arc;

use fontdb::{Database, Family, ID as DbFaceId, Query, Stretch, Style, Weight};
use rustc_hash::FxHashMap;
use swash::{CacheKey, FontRef};

use crate::config::Config;
use myanmar::is_myanmar_char;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular = 0,
    Bold = 1,
    Italic = 2,
    BoldItalic = 3,
}

impl FontStyle {
    pub fn from_flags(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => Self::Regular,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (true, true) => Self::BoldItalic,
        }
    }

    pub fn is_bold(self) -> bool {
        matches!(self, Self::Bold | Self::BoldItalic)
    }

    fn db_weight_style(self) -> (Weight, Style) {
        match self {
            Self::Regular => (Weight::NORMAL, Style::Normal),
            Self::Bold => (Weight::BOLD, Style::Normal),
            Self::Italic => (Weight::NORMAL, Style::Italic),
            Self::BoldItalic => (Weight::BOLD, Style::Italic),
        }
    }
}

pub struct LoadedFont {
    pub family: String,
    pub data: Arc<Vec<u8>>,
    pub face_index: u32,
    pub swash_offset: u32,
    pub swash_key: CacheKey,
    pub units_per_em: u16,
    pub is_myanmar_preferred: bool,
}

impl LoadedFont {
    pub fn swash_ref(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.swash_offset,
            key: self.swash_key,
        }
    }

    pub fn rustybuzz_face(&self) -> Option<rustybuzz::Face<'_>> {
        rustybuzz::Face::from_slice(&self.data, self.face_index)
    }

    pub fn glyph_id(&self, c: char) -> u16 {
        self.swash_ref().charmap().map(c)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub font_px: f32,
    pub myanmar_font_px: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    /// Distance from top of cell to alphabetic/Myanmar baseline in physical pixels
    pub baseline: f32,
    pub underline_top: f32,
    pub underline_thickness: f32,
    pub strikeout_top: f32,
}

pub struct FontSystem {
    db: Database,
    fonts: Vec<LoadedFont>,
    db_to_loaded: FxHashMap<DbFaceId, usize>,
    primary_chains: [Vec<usize>; 4],
    myanmar_chains: [Vec<usize>; 2], // [Regular, Bold]
    char_cache: FxHashMap<(char, FontStyle), usize>,
    pub features: Vec<rustybuzz::Feature>,
    pub metrics: CellMetrics,
}

const DEFAULT_MONO_CANDIDATES: &[&str] = &[
    "Cascadia Code NF",
    "CaskaydiaMono Nerd Font",
    "Cascadia Code",
    "Cascadia Mono",
    "JetBrains Mono",
    "Fira Code",
    "Ubuntu Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "FreeMono",
];

const DEFAULT_MYANMAR_CANDIDATES: &[&str] = &[
    "Padauk",
    "Padauk Book",
    "Pyidaungsu",
    "Myanmar Text",
    "Noto Sans Myanmar",
    "Noto Serif Myanmar",
    "Masterpiece Uni Sans",
];

const DEFAULT_SYMBOL_EMOJI_CANDIDATES: &[&str] = &[
    "Symbols Nerd Font Mono",
    "Symbols Nerd Font",
    "Noto Color Emoji",
    "Noto Sans Symbols 2",
    "Noto Sans Symbols",
    "DejaVu Sans",
];

impl FontSystem {
    pub fn new(config: &Config, scale_factor: f64) -> Self {
        let mut db = Database::new();
        db.load_system_fonts();

        let mut sys = Self {
            db,
            fonts: Vec::new(),
            db_to_loaded: FxHashMap::default(),
            primary_chains: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            myanmar_chains: [Vec::new(), Vec::new()],
            char_cache: FxHashMap::default(),
            features: parse_opentype_features(&config.font_features),
            metrics: CellMetrics {
                font_px: 18.0,
                myanmar_font_px: 17.0,
                cell_width: 9.0,
                cell_height: 24.0,
                baseline: 18.0,
                underline_top: 21.0,
                underline_thickness: 1.0,
                strikeout_top: 12.0,
            },
        };

        sys.rebuild_chains(config);
        sys.recompute_metrics(config, scale_factor);
        sys
    }

    pub fn update_size_or_config(
        &mut self,
        config: &Config,
        scale_factor: f64,
        rebuild_fonts: bool,
    ) {
        self.features = parse_opentype_features(&config.font_features);
        if rebuild_fonts {
            self.primary_chains = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
            self.myanmar_chains = [Vec::new(), Vec::new()];
            self.char_cache.clear();
            self.rebuild_chains(config);
        }
        self.recompute_metrics(config, scale_factor);
    }

    pub fn font(&self, idx: usize) -> &LoadedFont {
        &self.fonts[idx]
    }

    pub fn primary_font_name(&self) -> &str {
        self.primary_chains[0]
            .first()
            .and_then(|&id| self.fonts.get(id))
            .map(|f| f.family.as_str())
            .unwrap_or("monospace")
    }

    pub fn myanmar_font_name(&self) -> &str {
        self.myanmar_chains[0]
            .first()
            .and_then(|&id| self.fonts.get(id))
            .map(|f| f.family.as_str())
            .unwrap_or("none")
    }

    pub fn list_available_families(&self) -> (Vec<String>, Vec<String>) {
        let mut mono = Vec::new();
        let mut myanmar = Vec::new();
        for face in self.db.faces() {
            if let Some((fam, _)) = face.families.first()
                && face.monospaced && !mono.contains(fam) {
                    mono.push(fam.clone());
                }
        }
        mono.sort();

        // Check faces that cover Myanmar U+1000 ('က')
        let mut seen = Vec::new();
        for face in self.db.faces() {
            if let Some((fam, _)) = face.families.first() {
                if seen.contains(fam) {
                    continue;
                }
                seen.push(fam.clone());
                if fam.to_ascii_lowercase().contains("myanmar")
                    || fam.to_ascii_lowercase().contains("padauk")
                    || fam.to_ascii_lowercase().contains("pyidaungsu")
                {
                    myanmar.push(fam.clone());
                }
            }
        }
        myanmar.sort();
        (mono, myanmar)
    }

    fn load_db_face(&mut self, face_id: DbFaceId, is_myanmar_preferred: bool) -> Option<usize> {
        if let Some(&existing) = self.db_to_loaded.get(&face_id) {
            if is_myanmar_preferred {
                self.fonts[existing].is_myanmar_preferred = true;
            }
            return Some(existing);
        }

        let family = self
            .db
            .face(face_id)
            .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
            .unwrap_or_else(|| "Unknown".to_string());
        let face_index = self.db.face(face_id).map(|f| f.index).unwrap_or(0);

        let bytes = self.db.with_face_data(face_id, |slice, _| slice.to_vec())?;
        let data = Arc::new(bytes);
        let swash_font = FontRef::from_index(&data, face_index as usize)?;
        let swash_offset = swash_font.offset;
        let swash_key = swash_font.key;
        let units_per_em = swash_font.metrics(&[]).units_per_em.max(64);

        let idx = self.fonts.len();
        self.fonts.push(LoadedFont {
            family,
            data,
            face_index,
            swash_offset,
            swash_key,
            units_per_em,
            is_myanmar_preferred,
        });
        self.db_to_loaded.insert(face_id, idx);
        Some(idx)
    }

    fn query_and_load(
        &mut self,
        family: Family<'_>,
        weight: Weight,
        style: Style,
        is_myanmar_preferred: bool,
    ) -> Option<usize> {
        let q = Query {
            families: &[family],
            weight,
            stretch: Stretch::Normal,
            style,
        };
        let face_id = self.db.query(&q)?;
        self.load_db_face(face_id, is_myanmar_preferred)
    }

    fn rebuild_chains(&mut self, config: &Config) {
        let styles = [
            FontStyle::Regular,
            FontStyle::Bold,
            FontStyle::Italic,
            FontStyle::BoldItalic,
        ];

        for style in styles {
            let (weight, db_style) = style.db_weight_style();
            let mut chain = Vec::new();

            // Explicit style-specific family override (Ghostty `font-family-bold`, etc.)
            let explicit_style_fam = match style {
                FontStyle::Regular => None,
                FontStyle::Bold => config.font_family_bold.as_deref(),
                FontStyle::Italic => config.font_family_italic.as_deref(),
                FontStyle::BoldItalic => config.font_family_bold_italic.as_deref(),
            };
            if let Some(fam) = explicit_style_fam
                && let Some(id) = self.query_and_load(Family::Name(fam), weight, db_style, false)
            {
                push_unique(&mut chain, id);
            }

            // User-configured `font-family` entries
            for fam in &config.font_families {
                if let Some(id) = self.query_and_load(Family::Name(fam), weight, db_style, false) {
                    push_unique(&mut chain, id);
                }
            }

            // Default crisp monospace candidates if user didn't configure one (or as fallback)
            for &cand in DEFAULT_MONO_CANDIDATES {
                if let Some(id) = self.query_and_load(Family::Name(cand), weight, db_style, false) {
                    push_unique(&mut chain, id);
                    if config.font_families.is_empty() && chain.len() == 1 {
                        break;
                    }
                }
            }

            if let Some(id) = self.query_and_load(Family::Monospace, weight, db_style, false) {
                push_unique(&mut chain, id);
            }

            // Lightweight symbol fallback (Nerd Fonts); heavy emoji is lazy-loaded on-demand when rendered
            for &sym in &["Symbols Nerd Font Mono", "Symbols Nerd Font", "DejaVu Sans Mono"] {
                if let Some(id) =
                    self.query_and_load(Family::Name(sym), Weight::NORMAL, Style::Normal, false)
                {
                    push_unique(&mut chain, id);
                    break;
                }
            }

            self.primary_chains[style as usize] = chain;
        }

        // Build dedicated Myanmar chains ([Regular, Bold]) so Burmese always shapes with a genuine
        // OpenType `mym2`/`mymr` font (`Padauk`, `Pyidaungsu`, `Noto Sans Myanmar`, etc.)
        for (slot, weight) in [(0usize, Weight::NORMAL), (1usize, Weight::BOLD)] {
            let mut mm_chain = Vec::new();

            // 1. Explicit `font-family-myanmar`
            if let Some(fam) = &config.font_family_myanmar
                && let Some(id) =
                    self.query_and_load(Family::Name(fam), weight, Style::Normal, true)
                && self.fonts[id].glyph_id('\u{1000}') != 0
            {
                push_unique(&mut mm_chain, id);
            }

            // 2. Any user `font-family` entry that natively supports Myanmar (`က` U+1000 + `ြ` U+103C)
            for fam in &config.font_families {
                if let Some(id) =
                    self.query_and_load(Family::Name(fam), weight, Style::Normal, true)
                    && self.fonts[id].glyph_id('\u{1000}') != 0
                    && self.fonts[id].glyph_id('\u{103C}') != 0
                {
                    push_unique(&mut mm_chain, id);
                }
            }

            // 3. Built-in priority Myanmar Unicode fonts (break once top match is loaded to avoid duplicate font memory)
            for &cand in DEFAULT_MYANMAR_CANDIDATES {
                if let Some(id) =
                    self.query_and_load(Family::Name(cand), weight, Style::Normal, true)
                    && self.fonts[id].glyph_id('\u{1000}') != 0
                {
                    push_unique(&mut mm_chain, id);
                    break;
                }
            }

            // 4. Scan remaining system fonts if none of the named candidates were found
            if mm_chain.is_empty() {
                let all_ids: Vec<DbFaceId> = self.db.faces().map(|f| f.id).collect();
                for fid in all_ids {
                    if let Some(id) = self.load_db_face(fid, true)
                        && self.fonts[id].glyph_id('\u{1000}') != 0
                        && self.fonts[id].glyph_id('\u{103C}') != 0
                    {
                        push_unique(&mut mm_chain, id);
                        break;
                    }
                }
            }

            self.myanmar_chains[slot] = mm_chain;
        }
    }

    pub fn recompute_metrics(&mut self, config: &Config, scale_factor: f64) {
        // Convert point size (96 DPI: 1pt = 4/3 px) * display scale factor
        let font_px = (config.font_size * (96.0 / 72.0) * scale_factor as f32).max(6.0);
        let myanmar_font_px = (font_px * config.myanmar_font_scale).max(6.0);

        let (
            mut cell_w,
            mut ascender,
            mut descender,
            mut line_gap,
            mut ul_offset,
            mut ul_thick,
            mut strike_off,
        ) = (
            font_px * 0.6,
            font_px * 0.82,
            font_px * 0.22,
            0.0_f32,
            font_px * 0.12,
            (font_px / 14.0).max(1.0),
            font_px * 0.3,
        );

        if let Some(&primary_id) = self.primary_chains[0].first() {
            let f = &self.fonts[primary_id];
            let sref = f.swash_ref();
            let m = sref.metrics(&[]).scale(font_px);
            let gm = sref.glyph_metrics(&[]).scale(font_px);
            let m_gid = sref.charmap().map('M');
            let adv = gm.advance_width(m_gid);
            if adv > 1.0 {
                cell_w = adv;
            }
            ascender = m.ascent.max(font_px * 0.75);
            descender = m.descent.abs().max(font_px * 0.20);
            line_gap = m.leading.max(0.0);
            if m.underline_offset != 0.0 {
                ul_offset = m.underline_offset.abs();
            }
            if m.stroke_size > 0.0 {
                ul_thick = m.stroke_size.max(1.0);
            }
            if m.strikeout_offset > 0.0 {
                strike_off = m.strikeout_offset;
            }
        }

        // Ensure vertical metrics accommodate full stacked Burmese syllables (`မြို့`, `ဣန္ဒြေ`, `ကြွေး`, `င်္ဂ`)
        // We measured Padauk empirically: upper marks reach ~0.93em above baseline and lower marks reach ~0.44em below!
        if !self.myanmar_chains[0].is_empty() {
            ascender = ascender.max(myanmar_font_px * 0.96);
            descender = descender.max(myanmar_font_px * 0.46);
        }

        let raw_cell_w = config.adjust_cell_width.apply(cell_w).round().max(4.0);
        let raw_cell_h = config
            .adjust_cell_height
            .apply(ascender + descender + line_gap)
            .ceil()
            .max(6.0);

        // Center extra vertical space evenly above/below while keeping baseline pixel-snapped
        let extra_h = (raw_cell_h - (ascender + descender)).max(0.0);
        let baseline = (ascender + extra_h * 0.5).round();
        let underline_thickness = ul_thick.round().max(1.0);
        let underline_top = (baseline + ul_offset)
            .round()
            .min(raw_cell_h - underline_thickness);
        let strikeout_top = (baseline - strike_off).round().max(1.0);

        self.metrics = CellMetrics {
            font_px,
            myanmar_font_px,
            cell_width: raw_cell_w,
            cell_height: raw_cell_h,
            baseline,
            underline_top,
            underline_thickness,
            strikeout_top,
        };
    }

    /// Resolve the best font index for a given character & style, with O(1) memoization.
    pub fn resolve_font_for_char(&mut self, c: char, style: FontStyle) -> usize {
        if let Some(&cached) = self.char_cache.get(&(c, style)) {
            return cached;
        }

        // Route Myanmar script directly to the Myanmar OpenType font chain!
        if is_myanmar_char(c) {
            let slot = usize::from(style.is_bold());
            for &fid in &self.myanmar_chains[slot] {
                if self.fonts[fid].glyph_id(c) != 0 {
                    self.char_cache.insert((c, style), fid);
                    return fid;
                }
            }
        }

        for &fid in &self.primary_chains[style as usize] {
            if self.fonts[fid].glyph_id(c) != 0 {
                self.char_cache.insert((c, style), fid);
                return fid;
            }
        }

        // On-demand Symbol & Emoji resolution (loads Noto Color Emoji etc. only when encountered)
        for &sym in DEFAULT_SYMBOL_EMOJI_CANDIDATES {
            if let Some(id) =
                self.query_and_load(Family::Name(sym), Weight::NORMAL, Style::Normal, false)
                && self.fonts[id].glyph_id(c) != 0
            {
                self.char_cache.insert((c, style), id);
                return id;
            }
        }

        // Dynamic system-wide fallback scan (runs at most once per unique rare Unicode char)
        let all_ids: Vec<DbFaceId> = self.db.faces().map(|f| f.id).collect();
        for db_id in all_ids {
            if let Some(fid) = self.load_db_face(db_id, is_myanmar_char(c))
                && self.fonts[fid].glyph_id(c) != 0
            {
                self.char_cache.insert((c, style), fid);
                return fid;
            }
        }

        let fallback = self.primary_chains[0].first().copied().unwrap_or(0);
        self.char_cache.insert((c, style), fallback);
        fallback
    }
}

fn push_unique(vec: &mut Vec<usize>, val: usize) {
    if !vec.contains(&val) {
        vec.push(val);
    }
}

fn parse_opentype_features(specs: &[String]) -> Vec<rustybuzz::Feature> {
    specs
        .iter()
        .filter_map(|s| rustybuzz::Feature::from_str(s).ok())
        .collect()
}
