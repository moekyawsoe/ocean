use crate::config::ZawgyiMode;

/// Returns true if `c` belongs to any Unicode Myanmar block:
/// - Myanmar (`U+1000..=U+109F`)
/// - Myanmar Extended-B (`U+A9E0..=U+A9FF`)
/// - Myanmar Extended-A (`U+AA60..=U+AA7F`)
/// - Myanmar Extended-C (`U+116D0..=U+116FF`)
#[inline]
pub fn is_myanmar_char(c: char) -> bool {
    matches!(
        c as u32,
        0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F | 0x116D0..=0x116FF
    )
}

/// Returns true for invisible zero-width format, joiner, separator, bidi, and variation-selector
/// codepoints that TUI cell grids (e.g. Ink/xterm.js/ratatui splitting multi-column UAX #29
/// graphemes like `နေ့`, `ရေ`, `ဂျ`, `ပြီ`, `ပြော`) or LLM streams inject between cells.
/// If left inside a Myanmar HarfBuzz buffer, these break `mym2` syllable clustering and synthesize
/// spurious `U+25CC` (`◌`) dotted circles before `ေ` (`U+1031`), `ျ` (`U+103B`), and `ြ` (`U+103C`).
/// Note: `U+FE00` (VS1) is intentionally preserved for Khamti Shan / Phake / Aiton standardized variants.
#[inline]
pub fn is_ignorable_myanmar_format_char(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD
            | 0x034F
            | 0x061C
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0xFE01..=0xFE0F
            | 0xFEFF
            | 0xFFF9..=0xFFFC
            | 0xE0000..=0xE01EF
    )
}

/// Returns true if `c` is a Myanmar dependent vowel, medial consonant sign, virama/asat, or tone/sign
/// mark that must attach to a preceding base syllable and must never be orphaned into a separate run.
#[inline]
pub fn is_myanmar_dependent_mark(c: char) -> bool {
    matches!(
        c as u32,
        0x102B..=0x103E
            | 0x1056..=0x1059
            | 0x105E..=0x1060
            | 0x1062..=0x1064
            | 0x1067..=0x106D
            | 0x1071..=0x1074
            | 0x1082..=0x108D
            | 0x108F
            | 0x109A..=0x109D
            | 0xA9E5
            | 0xAA70
            | 0xAA7B..=0xAA7D
    )
}

/// Returns true if `c` is a Myanmar character or invisible format/dotted-circle char that should
/// remain inside an active Myanmar shaping run.
#[inline]
pub fn is_myanmar_run_char(c: char) -> bool {
    is_myanmar_char(c) || is_ignorable_myanmar_format_char(c) || c == '\u{25CC}'
}

#[inline]
pub fn contains_myanmar(s: &str) -> bool {
    s.chars().any(is_myanmar_char)
}

/// Strip TUI-injected invisible zero-width format characters that sit adjacent to Myanmar script
/// characters (used when copying selected terminal text to the system clipboard).
pub fn strip_myanmar_format_artifacts(s: &str) -> String {
    if !contains_myanmar(s) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if is_ignorable_myanmar_format_char(c) {
            let prev_mm = chars[..i]
                .iter()
                .rev()
                .find(|&&p| !is_ignorable_myanmar_format_char(p))
                .is_some_and(|&p| is_myanmar_char(p));
            let next_mm = chars[i + 1..]
                .iter()
                .find(|&&n| !is_ignorable_myanmar_format_char(n))
                .is_some_and(|&n| is_myanmar_char(n));
            if prev_mm || next_mm {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// UTN #11 canonical slot rank for reorderable Myanmar medials & dependent vowels/signs.
/// Only marks with rank > 0 are reordered relative to each other within a single base syllable;
/// consonants, digits, virama (`U+1039`), and kinzi (`U+103A U+1039`) act as barriers (rank 0).
#[inline]
fn utn11_mark_rank(c: char) -> u8 {
    match c {
        '\u{103B}' => 1,              // Medial Ya (ျ)
        '\u{103C}' => 2,              // Medial Ra (ြ)
        '\u{103D}' => 3,              // Medial Wa (ွ)
        '\u{103E}' => 4,              // Medial Ha (ှ)
        '\u{1031}' => 5,              // Vowel E (ေ)
        '\u{102D}' | '\u{102E}' => 6, // Upper Vowel I / II (ိ / ီ)
        '\u{102F}' | '\u{1030}' => 7, // Lower Vowel U / UU (ု / ူ)
        '\u{102B}' | '\u{102C}' => 8, // Vowel Tall AA / AA (ါ / ာ)
        '\u{1032}' => 9,              // Vowel AI (ဲ)
        '\u{1036}' => 10,             // Anusvara (ံ)
        '\u{1037}' => 11,             // Dot below / Aukmyit (့)
        '\u{1038}' => 12,             // Visarga (း)
        _ => 0,
    }
}

/// Sanitize TUI/LLM artifacts and canonicalize Myanmar syllable mark order while preserving
/// per-character terminal column origins:
/// 1. Strips invisible zero-width TUI cell separators (`ZWSP`, `ZWNJ`, `ZWJ`, `WJ`, `BOM`, `CGJ`, etc.)
///    that split multi-cell UAX #29 graphemes (`နေ့`, `ရော`, `ဂျ`, `ပြီး`, `များ`, `ပြော`).
/// 2. Strips dummy carrier spaces (`' '` / `'\0'`) or OCR/LLM-hallucinated `U+25CC` (`◌`) wedged
///    directly between a Myanmar character and a following dependent mark (while keeping standalone `◌ြ`).
/// 3. Recovers orphan word-initial visual-order pre-base `ေ` (`U+1031`) / `ြ` (`U+103C`) before a base.
/// 4. Applies UTN #11 canonical mark ordering + swapped `ေ်ာ` -> `ော်` recovery.
pub fn sanitize_and_canonicalize_myanmar(pairs: &mut Vec<(char, usize)>) {
    // 1. Strip invisible zero-width format / separator / joiner characters
    pairs.retain(|&(c, _)| !is_ignorable_myanmar_format_char(c));
    if pairs.is_empty() {
        return;
    }

    // 2. Remove spurious carrier `' '` / `'\0'` or OCR/LLM `U+25CC` (`◌`) wedged inside a syllable
    let mut cleaned = Vec::with_capacity(pairs.len());
    for i in 0..pairs.len() {
        let (c, col) = pairs[i];
        if matches!(c, ' ' | '\0' | '\u{25CC}') {
            let has_mm_prev = cleaned
                .last()
                .is_some_and(|&(p, _)| is_myanmar_char(p) && !matches!(p, '၊' | '။'));
            let has_dep_next = pairs
                .get(i + 1)
                .is_some_and(|&(n, _)| is_myanmar_dependent_mark(n));
            if has_mm_prev && has_dep_next {
                continue;
            }
        }
        cleaned.push((c, col));
    }
    *pairs = cleaned;

    // 3. Recover orphan word-initial (or post-punctuation) visual-order `ေ` (`U+1031`) / `ြ` (`U+103C`)
    let mut idx = 0;
    while idx < pairs.len() {
        let at_syllable_start = idx == 0 || matches!(pairs[idx - 1].0, ' ' | '၊' | '။');
        if at_syllable_start && matches!(pairs[idx].0, '\u{1031}' | '\u{103C}') {
            let pre_start = idx;
            let mut pre_e: Option<(char, usize)> = None;
            let mut pre_ra: Option<(char, usize)> = None;
            while idx < pairs.len() && matches!(pairs[idx].0, '\u{1031}' | '\u{103C}') {
                if pairs[idx].0 == '\u{1031}' {
                    pre_e = Some(pairs[idx]);
                } else {
                    pre_ra = Some(pairs[idx]);
                }
                idx += 1;
            }
            if idx < pairs.len() && is_myanmar_base(pairs[idx].0) {
                let mut base_end = idx + 1;
                // Keep subjoined consonant stacks (`U+1039 + C`) attached to the base
                while base_end + 1 < pairs.len()
                    && pairs[base_end].0 == '\u{1039}'
                    && is_myanmar_base(pairs[base_end + 1].0)
                {
                    base_end += 2;
                }
                let mut rebuilt = Vec::with_capacity(base_end - pre_start + 2);
                rebuilt.extend_from_slice(&pairs[idx..base_end]);
                if let Some(ra) = pre_ra {
                    rebuilt.push(ra);
                }
                if let Some(e) = pre_e {
                    rebuilt.push(e);
                }
                let new_idx = pre_start + rebuilt.len();
                pairs.splice(pre_start..base_end, rebuilt);
                idx = new_idx;
                continue;
            }
        }
        idx += 1;
    }

    // 4. Canonicalize mark ordering + swapped Asat/AA (`တေ်ာ` -> `တော်`)
    canonicalize_myanmar_with_cols(pairs);
    let mut swapped_asat_aa = false;
    for k in 0..pairs.len().saturating_sub(1) {
        if pairs[k].0 == '\u{103A}' && matches!(pairs[k + 1].0, '\u{102B}' | '\u{102C}') {
            pairs.swap(k, k + 1);
            swapped_asat_aa = true;
        }
    }
    if swapped_asat_aa {
        canonicalize_myanmar_with_cols(pairs);
    }
}

/// Canonicalize common non-standard Myanmar medial/vowel typing orders (e.g. `ကုိ` -> `ကို`, `ကေြ` -> `ကြေ`)
/// while preserving exact character count and per-character column origins.
pub fn canonicalize_myanmar_with_cols(chars: &mut [(char, usize)]) {
    let len = chars.len();
    let mut i = 0;
    while i < len {
        // Do not touch `U+103A U+1039` (Kinzi) or `U+1039` (Virama stack)
        if utn11_mark_rank(chars[i].0) > 0 {
            let start = i;
            while i < len && utn11_mark_rank(chars[i].0) > 0 {
                i += 1;
            }
            let slice = &mut chars[start..i];
            if slice.len() > 1 {
                // Insertion sort preserves stability for equal-rank marks
                for a in 1..slice.len() {
                    let mut b = a;
                    while b > 0 && utn11_mark_rank(slice[b - 1].0) > utn11_mark_rank(slice[b].0) {
                        slice.swap(b - 1, b);
                        b -= 1;
                    }
                }
            }
        } else {
            i += 1;
        }
    }
}

/// Fast heuristic detector for legacy Zawgyi-One text vs standard Myanmar Unicode.
/// Conservative so valid standard Unicode is never misclassified as Zawgyi.
pub fn is_likely_zawgyi(s: &str) -> bool {
    let chars: Vec<char> = s
        .chars()
        .filter(|&c| !is_ignorable_myanmar_format_char(c))
        .collect();
    if chars.is_empty() {
        return false;
    }

    let mut zg_score: i32 = 0;
    let mut uni_score: i32 = 0;

    for (idx, &c) in chars.iter().enumerate() {
        let code = c as u32;
        // Exclusive Zawgyi codepoint repurposing (Mon/Shan/Khamti slots used as Burmese medials/stacks in Zawgyi)
        if matches!(
            code,
            0x1033
                | 0x1034
                | 0x105A
                | 0x1060..=0x106D
                | 0x1070..=0x108A
                | 0x108E
                | 0x1093..=0x1097
        ) {
            zg_score += 4;
        }
        // Standard Unicode Kinzi `U+1004 U+103A U+1039` or Virama `U+1039` + Consonant
        if c == '\u{103A}' {
            uni_score += 4;
        }
        // In Zawgyi, `U+1031` (ေ) or `U+103B` (ြ in ZG) appears at start of string or after space/punctuation/vowel
        if c == '\u{1031}' {
            let prev = if idx > 0 { Some(chars[idx - 1]) } else { None };
            match prev {
                None => zg_score += 4,
                Some(p)
                    if !is_myanmar_char(p)
                        || matches!(p, '၊' | '။' | '\u{1038}' | '\u{102C}' | '\u{102B}') =>
                {
                    zg_score += 3;
                }
                _ => {}
            }
        }
        // In Zawgyi, `U+1039` is used as Asat `်` at the end of a word (followed by space/end/punctuation/visarga)
        if c == '\u{1039}' {
            let next = chars.get(idx + 1).copied();
            match next {
                None => zg_score += 3,
                Some(n)
                    if !is_myanmar_char(n) || matches!(n, '\u{1038}' | '\u{1037}' | '၊' | '။') =>
                {
                    zg_score += 3;
                }
                Some('\u{1000}'..='\u{1021}') => {
                    // Could be Unicode stacked consonant OR Zawgyi asat followed by next syllable!
                    // Check if preceded by vowel (Zawgyi: consonant + vowel + 1039 asat)
                    if idx > 0
                        && matches!(
                            chars[idx - 1],
                            '\u{102B}'..='\u{1032}' | '\u{1036}' | '\u{103D}'
                        )
                    {
                        zg_score += 3;
                    }
                }
                _ => {}
            }
        }
    }

    zg_score >= 3 && zg_score > uni_score
}

/// Convert legacy Zawgyi-One text to standard Myanmar Unicode for display shaping.
pub fn maybe_convert_zawgyi(s: &str, mode: ZawgyiMode) -> Option<String> {
    match mode {
        ZawgyiMode::Off => None,
        ZawgyiMode::Auto => {
            if is_likely_zawgyi(s) {
                Some(zawgyi_to_unicode(s))
            } else {
                None
            }
        }
        ZawgyiMode::Force => {
            if contains_myanmar(s) {
                Some(zawgyi_to_unicode(s))
            } else {
                None
            }
        }
    }
}

/// Core Zawgyi-One -> Unicode 5.1+ converter covering common Burmese Zawgyi encodings.
pub fn zawgyi_to_unicode(input: &str) -> String {
    // Step 1: Map Zawgyi codepoints to intermediate/direct Unicode sequences
    let mut mapped = Vec::<char>::with_capacity(input.len() + 8);
    for c in input
        .chars()
        .filter(|&ch| !is_ignorable_myanmar_format_char(ch))
    {
        match c {
            '\u{1033}' => mapped.push('\u{102F}'), // tall u -> u
            '\u{1034}' => mapped.push('\u{1030}'), // tall uu -> uu
            '\u{1039}' => mapped.push('\u{103A}'), // ZG asat -> Unicode asat
            '\u{103A}' => mapped.push('\u{103B}'), // ZG medial ya -> Unicode medial ya
            '\u{103B}' => mapped.push('\u{103C}'), // ZG medial ra -> Unicode medial ra
            '\u{103C}' => mapped.push('\u{103D}'), // ZG medial wa -> Unicode medial wa
            '\u{103D}' => mapped.push('\u{103E}'), // ZG medial ha -> Unicode medial ha
            '\u{107E}'..='\u{1084}' => mapped.push('\u{103C}'), // ZG wide/upper medial ra variants
            '\u{106A}' => mapped.push('\u{1009}'), // nya
            '\u{106B}' => mapped.push('\u{100A}'), // nnya
            '\u{108F}' => mapped.push('\u{1014}'), // na variant
            '\u{1090}' => mapped.push('\u{101B}'), // ra variant
            '\u{1025}' => mapped.push('\u{1025}'),
            '\u{1064}' => {
                // Kinzi marker (င်္) — attach to preceding base in step 2
                mapped.push('\u{FFFF}');
            }
            '\u{108D}' => mapped.push('\u{103E}'),
            '\u{108A}' => {
                mapped.push('\u{103D}');
                mapped.push('\u{103E}');
            }
            '\u{105A}' => {
                mapped.push('\u{102B}');
                mapped.push('\u{103A}');
            }
            '\u{1088}' => {
                mapped.push('\u{103E}');
                mapped.push('\u{102F}');
            }
            '\u{1089}' => {
                mapped.push('\u{103E}');
                mapped.push('\u{1030}');
            }
            '\u{1094}' | '\u{1095}' => mapped.push('\u{1037}'),
            // Common Zawgyi subjoined consonants -> Virama (1039) + Base Consonant
            '\u{1060}' => mapped.extend(['\u{1039}', '\u{1000}']),
            '\u{1061}' => mapped.extend(['\u{1039}', '\u{1001}']),
            '\u{1062}' => mapped.extend(['\u{1039}', '\u{1002}']),
            '\u{1063}' => mapped.extend(['\u{1039}', '\u{1003}']),
            '\u{1065}' => mapped.extend(['\u{1039}', '\u{1005}']),
            '\u{1066}' | '\u{1067}' => mapped.extend(['\u{1039}', '\u{1006}']),
            '\u{1068}' => mapped.extend(['\u{1039}', '\u{1007}']),
            '\u{1069}' => mapped.extend(['\u{1039}', '\u{1008}']),
            '\u{1070}' => mapped.extend(['\u{1039}', '\u{100F}']),
            '\u{1071}' | '\u{1072}' => mapped.extend(['\u{1039}', '\u{1010}']),
            '\u{1073}' | '\u{1074}' => mapped.extend(['\u{1039}', '\u{1011}']),
            '\u{1075}' => mapped.extend(['\u{1039}', '\u{1012}']),
            '\u{1076}' => mapped.extend(['\u{1039}', '\u{1013}']),
            '\u{1077}' => mapped.extend(['\u{1039}', '\u{1014}']),
            '\u{1078}' => mapped.extend(['\u{1039}', '\u{1015}']),
            '\u{1079}' => mapped.extend(['\u{1039}', '\u{1016}']),
            '\u{107A}' => mapped.extend(['\u{1039}', '\u{1017}']),
            '\u{107B}' => mapped.extend(['\u{1039}', '\u{1018}']),
            '\u{107C}' => mapped.extend(['\u{1039}', '\u{1019}']),
            '\u{1085}' => mapped.extend(['\u{1039}', '\u{101C}']),
            _ => mapped.push(c),
        }
    }

    // Step 2: Reorder Zawgyi pre-base visual `ေ` (`U+1031`) and `ြ` (`U+103C`) to come AFTER the base consonant,
    // and move Kinzi (`\u{FFFF}`) BEFORE the base consonant as `င + ် + ္`.
    let mut reordered = Vec::<(char, usize)>::with_capacity(mapped.len() + 4);
    let mut i = 0;
    while i < mapped.len() {
        let mut pre_e = false;
        let mut pre_ra = false;
        while i < mapped.len() && matches!(mapped[i], '\u{1031}' | '\u{103C}') {
            if mapped[i] == '\u{1031}' {
                pre_e = true;
            } else {
                pre_ra = true;
            }
            i += 1;
        }
        if (pre_e || pre_ra) && i < mapped.len() && is_myanmar_base(mapped[i]) {
            let base_pos = reordered.len();
            reordered.push((mapped[i], 0));
            i += 1;
            // Check if Kinzi marker follows base consonant (in Zawgyi, kinzi was stored after base)
            let mut has_kinzi = false;
            let mut extra = Vec::new();
            while i < mapped.len() && (mapped[i] == '\u{FFFF}' || utn11_mark_rank(mapped[i]) > 0) {
                if mapped[i] == '\u{FFFF}' {
                    has_kinzi = true;
                } else {
                    extra.push((mapped[i], 0));
                }
                i += 1;
            }
            if pre_ra {
                reordered.push(('\u{103C}', 0));
            }
            if pre_e {
                reordered.push(('\u{1031}', 0));
            }
            reordered.extend(extra);
            if has_kinzi {
                reordered.splice(
                    base_pos..base_pos,
                    [('\u{1004}', 0), ('\u{103A}', 0), ('\u{1039}', 0)],
                );
            }
        } else {
            if pre_ra {
                reordered.push(('\u{103C}', 0));
            }
            if pre_e {
                reordered.push(('\u{1031}', 0));
            }
            if i < mapped.len() {
                if mapped[i] == '\u{FFFF}' {
                    // Move kinzi before preceding base consonant
                    if let Some(pos) = reordered.iter().rposition(|(ch, _)| is_myanmar_base(*ch)) {
                        reordered.splice(
                            pos..pos,
                            [('\u{1004}', 0), ('\u{103A}', 0), ('\u{1039}', 0)],
                        );
                    }
                } else {
                    reordered.push((mapped[i], 0));
                }
                i += 1;
            }
        }
    }

    canonicalize_myanmar_with_cols(&mut reordered);
    reordered.into_iter().map(|(c, _)| c).collect()
}

#[inline]
fn is_myanmar_base(c: char) -> bool {
    matches!(c as u32, 0x1000..=0x102A | 0x103F | 0x1040..=0x1049 | 0x104E)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_swapped_myanmar_vowels() {
        // Swapped `က + ု + ိ` -> `က + ိ + ု` (`ကို`)
        let mut v = vec![('\u{1000}', 0), ('\u{102F}', 1), ('\u{102D}', 1)];
        canonicalize_myanmar_with_cols(&mut v);
        let s: String = v.iter().map(|x| x.0).collect();
        assert_eq!(s, "ကို");

        // Swapped `က + ေ + ြ` -> `က + ြ + ေ` (`ကြေ`)
        let mut v2 = vec![('\u{1000}', 0), ('\u{1031}', 1), ('\u{103C}', 2)];
        canonicalize_myanmar_with_cols(&mut v2);
        let s2: String = v2.iter().map(|x| x.0).collect();
        assert_eq!(s2, "ကြေ");

        // Swapped `ကာေ`, `မာျး`, and `တေ်ာ` -> `ကော`, `များ`, `တော်`
        let cases = [
            ("\u{1000}\u{102C}\u{1031}", "ကော"),
            ("\u{1019}\u{102C}\u{103B}\u{1038}", "များ"),
            ("\u{1010}\u{1031}\u{103A}\u{102C}", "တော်"),
            ("\u{1031}\u{1015}\u{1038}", "ပေး"),
            ("\u{1031}\u{103C}\u{1015}\u{102C}", "ပြော"),
        ];
        for (raw, expected) in cases {
            let mut pairs: Vec<(char, usize)> =
                raw.chars().enumerate().map(|(i, c)| (c, i)).collect();
            sanitize_and_canonicalize_myanmar(&mut pairs);
            let got: String = pairs.iter().map(|x| x.0).collect();
            assert_eq!(got, expected, "failed canonicalizing {raw:?}");
        }
    }

    #[test]
    fn sanitizes_tui_grapheme_separators_and_spurious_dotted_circles() {
        let broken_tui =
            "ဒီန\u{200B}ေ့ ပရ\u{200C}ောဂ\u{200D}ျက်နဲ့ ပ\u{2060}ြ\u{FEFF}ောပ\u{25CC}ြပ\u{034F}ေးပါ။";
        let mut pairs: Vec<(char, usize)> = broken_tui
            .chars()
            .enumerate()
            .map(|(i, c)| (c, i))
            .collect();
        sanitize_and_canonicalize_myanmar(&mut pairs);
        let got: String = pairs.iter().map(|x| x.0).collect();
        assert_eq!(got, "ဒီနေ့ ပရောဂျက်နဲ့ ပြောပြပေးပါ။");

        // Standalone `◌ြ` at start of run is preserved (e.g. documentation / character charts)
        let mut chart = vec![('\u{25CC}', 0), ('\u{103C}', 1)];
        sanitize_and_canonicalize_myanmar(&mut chart);
        let chart_str: String = chart.iter().map(|x| x.0).collect();
        assert_eq!(chart_str, "\u{25CC}\u{103C}");

        // Clipboard helper strips TUI separators around Myanmar while preserving unrelated text
        assert_eq!(
            strip_myanmar_format_artifacts("AI: ဒီန\u{200B}ေ့ ပရ\u{200B}ောဂ\u{200B}ျက်နဲ့"),
            "AI: ဒီနေ့ ပရောဂျက်နဲ့"
        );
    }

    #[test]
    fn zawgyi_detection_never_flags_standard_unicode() {
        assert!(!is_likely_zawgyi("မင်္ဂလာပါ"));
        assert!(!is_likely_zawgyi("ပြည်ထောင်စုသမ္မတမြန်မာနိုင်ငံတော်"));
        assert!(!is_likely_zawgyi("မြန်မာဘာသာစကား"));
        // Even with TUI-injected ZWSP before `ေ`, standard Unicode must never be misflagged as Zawgyi!
        assert!(!is_likely_zawgyi("ဒီန\u{200B}ေ့ ပရ\u{200B}ောဂ\u{200B}ျက်နဲ့"));

        // Classic Zawgyi strings:
        // Zawgyi "ကောင်း" = \u1031\u1000\u102c\u1004\u1039\u1038
        let zg_kaung = "\u{1031}\u{1000}\u{102c}\u{1004}\u{1039}\u{1038}";
        assert!(is_likely_zawgyi(zg_kaung));
        assert_eq!(zawgyi_to_unicode(zg_kaung), "ကောင်း");

        // Zawgyi "မြန်မာ" = \u107e\u1019\u1014\u1039\u{1019}\u{102c}
        let zg_myanmar = "\u{107e}\u{1019}\u{1014}\u{1039}\u{1019}\u{102c}";
        assert!(is_likely_zawgyi(zg_myanmar));
        assert_eq!(zawgyi_to_unicode(zg_myanmar), "မြန်မာ");
    }
}
