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

/// Converts `text` according to `setting`. Non-Han text (English, numbers, punctuation)
/// passes through unchanged, since OpenCC only maps Han characters.
pub fn convert(text: &str, setting: ScriptSetting) -> String {
    match setting {
        ScriptSetting::TraditionalHk => S2HK.convert(text),
        ScriptSetting::Simplified => T2S.convert(text),
        ScriptSetting::LeaveAsRecognized => text.to_string(),
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
}
