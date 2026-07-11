//! Unicode sanitization for user-supplied session tags — strips
//! invisible/format characters that could otherwise smuggle control
//! sequences into a tag value. See `session_mutations.py`'s
//! `_sanitize_unicode`.

use std::sync::LazyLock;

use regex::Regex;
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;

/// Upper bound on normalize/strip rounds — a fixed point is reached
/// well before this in practice; never raises on non-convergence.
const MAX_ITERATIONS: usize = 10;

static STRIP_RANGES_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("[\u{200b}-\u{200f}\u{202a}-\u{202e}\u{2066}-\u{2069}\u{feff}\u{e000}-\u{f8ff}]")
        .expect("valid regex")
});

/// Normalizes to NFKC, strips characters in the Format/PrivateUse/
/// Unassigned Unicode general categories, then strips an explicit set
/// of zero-width/BOM/directional-mark/private-use ranges (a redundant
/// safety net over the category strip) — repeated until a fixed point
/// or [`MAX_ITERATIONS`] rounds.
#[must_use]
pub(crate) fn sanitize_unicode(value: &str) -> String {
    let mut current = value.to_string();
    for _ in 0..MAX_ITERATIONS {
        let previous = current.clone();
        let normalized: String = current.nfkc().collect();
        let category_stripped: String = normalized
            .chars()
            .filter(|c| {
                !matches!(
                    get_general_category(*c),
                    GeneralCategory::Format
                        | GeneralCategory::PrivateUse
                        | GeneralCategory::Unassigned
                )
            })
            .collect();
        current = STRIP_RANGES_RE
            .replace_all(&category_stripped, "")
            .into_owned();
        if current == previous {
            break;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_plain_text_untouched() {
        assert_eq!(sanitize_unicode("release-1.0"), "release-1.0");
    }

    #[test]
    fn strips_zero_width_characters() {
        assert_eq!(sanitize_unicode("a\u{200b}b"), "ab");
    }

    #[test]
    fn strips_bom() {
        assert_eq!(sanitize_unicode("\u{feff}tag"), "tag");
    }

    #[test]
    fn strips_directional_marks() {
        assert_eq!(sanitize_unicode("a\u{202e}b"), "ab");
    }

    #[test]
    fn strips_private_use_area() {
        assert_eq!(sanitize_unicode("a\u{e000}b"), "ab");
    }

    #[test]
    fn nfkc_normalizes_compatibility_characters() {
        // U+FF21 FULLWIDTH LATIN CAPITAL LETTER A -> "A" under NFKC.
        assert_eq!(sanitize_unicode("\u{FF21}"), "A");
    }

    #[test]
    fn purely_invisible_input_sanitizes_to_empty_string() {
        assert_eq!(sanitize_unicode("\u{200b}\u{200c}\u{feff}"), "");
    }

    #[test]
    fn converges_within_max_iterations() {
        let input = "\u{200b}".repeat(5) + "clean";
        assert_eq!(sanitize_unicode(&input), "clean");
    }
}
