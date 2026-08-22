// script.rs
//
// Script conversion for Chinese transcripts: Whisper's `zh` decoding emits mostly
// Simplified characters regardless of what was spoken, so Cantonese and Mandarin
// meetings alike need converting to the user's chosen script. Conversion is
// deterministic and idempotent, so it is applied once as transcripts are stored
// (see docs/adr/0003-script-conversion-at-ingest.md), not on every read.
//
// Uses `ferrous-opencc` — pure Rust, so no OpenCC C++ build dependency.

use crate::database::repositories::setting_store::SettingToken;
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use once_cell::sync::Lazy;

/// The Script setting: which Han character set (if any) transcripts are converted into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptSetting {
    /// Convert to Traditional Chinese, Hong Kong convention. The default.
    TraditionalHk,
    /// Convert to Simplified Chinese.
    Simplified,
    /// No conversion — store whatever the recognizer produced.
    LeaveAsRecognized,
}

impl SettingToken for ScriptSetting {
    const TOKENS: &'static [(&'static str, Self)] = &[
        ("traditional-hk", ScriptSetting::TraditionalHk),
        ("simplified", ScriptSetting::Simplified),
        ("as-recognized", ScriptSetting::LeaveAsRecognized),
    ];
}

impl ScriptSetting {
    /// Token stored in the database and in meeting metadata.
    pub fn as_str(self) -> &'static str {
        <Self as SettingToken>::as_token(self)
    }

    /// Resolves a stored token into a setting. Unrecognized or absent values fall back
    /// to the default (Traditional HK) rather than erroring, since this reads a value a
    /// user picked from a fixed dropdown.
    pub fn from_stored(token: Option<&str>) -> Self {
        <Self as SettingToken>::from_token(token)
    }
}

impl Default for ScriptSetting {
    fn default() -> Self {
        ScriptSetting::TraditionalHk
    }
}

static S2HK: Lazy<OpenCC> = Lazy::new(|| {
    OpenCC::from_config(BuiltinConfig::S2hk).expect("embedded s2hk OpenCC config failed to load")
});

static T2S: Lazy<OpenCC> = Lazy::new(|| {
    OpenCC::from_config(BuiltinConfig::T2s).expect("embedded t2s OpenCC config failed to load")
});

/// Plain Simplified-to-Traditional, without the HK variants pass `S2HK` applies. Used only
/// by `detect_script`'s round-trip check: `S2HK` also rewrites ordinary Traditional
/// characters to their Hong Kong glyph variant (e.g. 說→説), which would make genuine
/// Traditional text look like it contains Simplified-only characters.
static S2T: Lazy<OpenCC> = Lazy::new(|| {
    OpenCC::from_config(BuiltinConfig::S2t).expect("embedded s2t OpenCC config failed to load")
});

/// Converts `text` according to `setting`. Non-Han text (English, numbers, punctuation)
/// passes through unchanged, since OpenCC only maps Han characters.
pub fn convert(text: &str, setting: ScriptSetting) -> String {
    match setting {
        ScriptSetting::TraditionalHk => S2HK.convert(text),
        ScriptSetting::Simplified => T2S.convert(text),
        ScriptSetting::LeaveAsRecognized => text.to_string(),
    }
}

/// Which Han character set `detect_script` found `text` to be written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedScript {
    Traditional,
    Simplified,
    /// No distinguishing characters: non-Chinese text, or text using only characters
    /// shared by both scripts (numerals, punctuation, Han characters with no
    /// Traditional/Simplified variant).
    Neither,
}

/// Detects which Han script `text` is written in by character-set membership, not by
/// Unicode range (Traditional and Simplified share the same Han range, so a range check
/// cannot tell them apart — see docs/adr/0003 and phykawing/meetily#2).
///
/// Round-trips `text` through both conversion directions: if simplifying it changes it,
/// it contains Traditional-only characters; if traditionalizing it changes it, it
/// contains Simplified-only characters. Text with both (mixed script) or neither
/// (non-Chinese, or only shared characters) resolves to `Neither`.
pub fn detect_script(text: &str) -> DetectedScript {
    let has_traditional_only_chars = T2S.convert(text) != text;
    let has_simplified_only_chars = S2T.convert(text) != text;

    match (has_traditional_only_chars, has_simplified_only_chars) {
        (true, false) => DetectedScript::Traditional,
        (false, true) => DetectedScript::Simplified,
        _ => DetectedScript::Neither,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplified_converts_to_traditional_hk() {
        assert_eq!(
            convert("开放中文转换是完全由 Rust 实现的。", ScriptSetting::TraditionalHk),
            "開放中文轉換是完全由 Rust 實現的。"
        );
    }

    #[test]
    fn traditional_conversion_is_idempotent() {
        let once = convert("开放中文转换是完全由 Rust 实现的。", ScriptSetting::TraditionalHk);
        let twice = convert(&once, ScriptSetting::TraditionalHk);
        assert_eq!(once, twice);
    }

    #[test]
    fn simplified_setting_converts_traditional_to_simplified() {
        assert_eq!(
            convert("開放中文轉換是完全由 Rust 實現的。", ScriptSetting::Simplified),
            "开放中文转换是完全由 Rust 实现的。"
        );
    }

    #[test]
    fn simplified_conversion_is_idempotent() {
        let once = convert("開放中文轉換是完全由 Rust 實現的。", ScriptSetting::Simplified);
        let twice = convert(&once, ScriptSetting::Simplified);
        assert_eq!(once, twice);
    }

    #[test]
    fn leave_as_recognized_is_identity() {
        let text = "开放中文转换是完全由 Rust 实现的。";
        assert_eq!(convert(text, ScriptSetting::LeaveAsRecognized), text);
    }

    #[test]
    fn embedded_english_numbers_and_punctuation_are_unaffected() {
        let text = "我哋今日 deadline 係 2026-08-18, OK?";
        let converted = convert(text, ScriptSetting::TraditionalHk);
        assert!(converted.contains("deadline"));
        assert!(converted.contains("2026-08-18"));
        assert!(converted.contains("OK?"));
    }

    #[test]
    fn pure_english_text_is_unchanged_by_any_setting() {
        let text = "Hello world, this is a test.";
        assert_eq!(convert(text, ScriptSetting::TraditionalHk), text);
        assert_eq!(convert(text, ScriptSetting::Simplified), text);
        assert_eq!(convert(text, ScriptSetting::LeaveAsRecognized), text);
    }

    #[test]
    fn stored_token_round_trips() {
        for setting in [
            ScriptSetting::TraditionalHk,
            ScriptSetting::Simplified,
            ScriptSetting::LeaveAsRecognized,
        ] {
            assert_eq!(ScriptSetting::from_stored(Some(setting.as_str())), setting);
        }
    }

    #[test]
    fn missing_or_unknown_stored_token_defaults_to_traditional_hk() {
        assert_eq!(ScriptSetting::from_stored(None), ScriptSetting::TraditionalHk);
        assert_eq!(
            ScriptSetting::from_stored(Some("bogus")),
            ScriptSetting::TraditionalHk
        );
    }

    // detect_script -------------------------------------------------------------

    #[test]
    fn detects_traditional_text() {
        assert_eq!(
            detect_script("開放中文轉換是完全由 Rust 實現的。"),
            DetectedScript::Traditional
        );
    }

    #[test]
    fn detects_simplified_text() {
        assert_eq!(
            detect_script("开放中文转换是完全由 Rust 实现的。"),
            DetectedScript::Simplified
        );
    }

    #[test]
    fn non_chinese_text_is_neither() {
        assert_eq!(
            detect_script("Hello world, this is a test."),
            DetectedScript::Neither
        );
    }

    #[test]
    fn text_with_no_distinguishing_han_characters_is_neither() {
        // "中文" is written identically in both scripts; nothing here disambiguates.
        assert_eq!(detect_script("中文 123"), DetectedScript::Neither);
    }

    #[test]
    fn mixed_traditional_and_simplified_text_is_neither() {
        // Contains both a Traditional-only character (轉) and a Simplified-only one (转).
        let mixed = "開放中文轉換 开放中文转换";
        assert_eq!(detect_script(mixed), DetectedScript::Neither);
    }

    #[test]
    fn embedded_english_and_punctuation_do_not_affect_detection() {
        assert_eq!(
            detect_script("我哋今日 deadline 係 2026-08-18, OK?"),
            DetectedScript::Traditional
        );
    }

    #[test]
    fn traditional_text_with_hk_variant_characters_is_still_traditional() {
        // 說/溫/戶/臺 are ordinary Traditional characters that S2HK's HKVariants pass
        // rewrites to a Hong Kong glyph (説/温/户/台); detection must not mistake that
        // for evidence of Simplified-only characters (see phykawing/meetily#14 review).
        assert_eq!(detect_script("他說得很清楚。"), DetectedScript::Traditional);
        assert_eq!(detect_script("溫度戶外臺灣"), DetectedScript::Traditional);
    }
}
