// whisper_engine/language.rs
//
// Maps the language the user picks in the UI onto the language token a *particular*
// model was trained to accept, and builds the initial prompt that biases script and
// vocabulary.
//
// The mapping is per-model on purpose: "Cantonese" in the UI is not always `yue` at the
// engine. Stock OpenAI checkpoints carry a `yue` token (id 99 in whisper.cpp's table) but
// were barely trained on it, and forcing it sends the decoder into repetition loops. The
// stock path therefore decodes as `zh` and leans on the initial prompt for register and
// script, while a Cantonese fine-tune declares whichever token it was actually trained
// with. See docs/adr/0003 for the script half of this.

/// UI-facing code for Cantonese. Not always what reaches whisper.cpp — see
/// [`engine_language_for`].
pub const CANTONESE: &str = "yue";

/// Seeds Cantonese decoding with Traditional characters and colloquial 口語 markers, and
/// with an English word left in place so embedded English survives rather than being
/// translated into Chinese.
pub const CANTONESE_PROMPT_SEED: &str = "以下係一段粵語會議錄音嘅逐字紀錄，用繁體中文書寫，保留原本嘅英文字詞，例如 deadline。";

/// whisper.cpp accepts at most `n_text_ctx / 2` = 224 prompt tokens and silently drops the
/// rest. Chinese runs roughly a token per character, so cap on characters with headroom;
/// the seed is kept whole and the user's vocabulary is what gets trimmed.
const MAX_PROMPT_CHARS: usize = 180;

/// Whether `model_name` can be trusted with Cantonese.
///
/// The `large-v3` family (including turbo and quantized variants) qualifies because it is
/// the documented `zh`-plus-prompt baseline. Smaller checkpoints are excluded: they will
/// happily accept the language and return unusable text, which is worse than refusing.
/// Registered custom models declare their own capability and bypass this entirely.
pub fn is_cantonese_capable_builtin(model_name: &str) -> bool {
    model_name.starts_with("large-v3")
}

/// The language token to hand whisper.cpp, or `None` to let it auto-detect.
///
/// `custom_engine_language` is the token declared by a registered custom model, when the
/// loaded model is one.
pub fn engine_language_for<'a>(
    ui_language: &'a str,
    model_name: &str,
    custom_engine_language: Option<&'a str>,
) -> LanguageResolution<'a> {
    match ui_language {
        "auto" => LanguageResolution::AutoDetect { translate: false },
        "auto-translate" => LanguageResolution::AutoDetect { translate: true },
        CANTONESE => match custom_engine_language {
            Some(token) => LanguageResolution::Forced(token),
            None if is_cantonese_capable_builtin(model_name) => LanguageResolution::Forced("zh"),
            None => LanguageResolution::Unsupported,
        },
        other => LanguageResolution::Forced(other),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageResolution<'a> {
    /// Let whisper detect the language; `translate` mirrors the old "auto-translate" mode.
    AutoDetect { translate: bool },
    /// Force this token.
    Forced(&'a str),
    /// The loaded model cannot serve this language. `WhisperEngine::resolve_decoding` turns
    /// this into an [`UnsupportedLanguageError`] carried by its `anyhow::Error`; the
    /// transcription providers (`whisper_provider.rs`, `worker.rs`) then map it onto
    /// `TranscriptionError::UnsupportedLanguage` so the user sees the actionable message
    /// with `actionable: true`. Recording start also runs `resolve_decoding` as a
    /// pre-flight check and refuses to begin capture on this case.
    Unsupported,
}

/// The loaded model cannot decode the language the user picked in the UI.
///
/// Constructed by `WhisperEngine::resolve_decoding` and wrapped in its `anyhow::Error` so
/// the provider boundary can `downcast_ref` it back out and raise
/// `TranscriptionError::UnsupportedLanguage` instead of a doubly-wrapped `EngineFailed`
/// string. The `Display` text is the actionable, ready-to-show message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedLanguageError {
    /// UI-facing language code (e.g. `yue`).
    pub ui_language: String,
    /// Name of the model that is loaded but cannot serve it.
    pub model_name: String,
}

impl std::fmt::Display for UnsupportedLanguageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Model '{}' cannot transcribe '{}'. Load a Cantonese-capable model \
             (large-v3 family, or a registered Cantonese model).",
            self.model_name, self.ui_language
        )
    }
}

impl std::error::Error for UnsupportedLanguageError {}

/// Builds the initial prompt from the language's built-in seed plus the user's meeting
/// vocabulary (names, jargon, product terms). Returns `None` when there is nothing to say.
pub fn build_initial_prompt(ui_language: &str, vocabulary: Option<&str>) -> Option<String> {
    let seed = match ui_language {
        CANTONESE => Some(CANTONESE_PROMPT_SEED),
        _ => None,
    };
    let vocabulary = vocabulary.map(str::trim).filter(|v| !v.is_empty());

    let mut prompt = match (seed, vocabulary) {
        (None, None) => return None,
        (Some(seed), None) => seed.to_string(),
        (None, Some(vocab)) => vocab.to_string(),
        (Some(seed), Some(vocab)) => format!("{seed}{vocab}"),
    };

    // Trim from the end so the seed survives: it is what biases script and register, while
    // losing the tail of a vocabulary list only costs a few proper nouns.
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        prompt = prompt.chars().take(MAX_PROMPT_CHARS).collect();
    }
    Some(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cantonese_on_stock_large_v3_decodes_as_chinese() {
        // Forcing `yue` on a stock checkpoint collapses the decoder; `zh` plus the prompt
        // is the supported baseline.
        for model in ["large-v3", "large-v3-turbo", "large-v3-turbo-q5_0", "large-v3-q5_0"] {
            assert_eq!(
                engine_language_for(CANTONESE, model, None),
                LanguageResolution::Forced("zh"),
                "model {model}"
            );
        }
    }

    #[test]
    fn cantonese_is_unsupported_on_small_models() {
        for model in ["tiny", "base", "small", "medium", "medium-q5_0", "base-q5_1"] {
            assert_eq!(
                engine_language_for(CANTONESE, model, None),
                LanguageResolution::Unsupported,
                "model {model}"
            );
        }
    }

    #[test]
    fn custom_model_declares_its_own_token() {
        // A fine-tune trained with the `yue` token gets `yue`, even though the same UI
        // choice resolves to `zh` on stock models.
        assert_eq!(
            engine_language_for(CANTONESE, "my-cantonese-turbo", Some("yue")),
            LanguageResolution::Forced("yue")
        );
        // And one trained with `zh` gets `zh`, on a model name the builtin rule rejects.
        assert_eq!(
            engine_language_for(CANTONESE, "my-cantonese-small", Some("zh")),
            LanguageResolution::Forced("zh")
        );
    }

    #[test]
    fn unsupported_language_error_renders_the_actionable_message() {
        let err = UnsupportedLanguageError {
            ui_language: CANTONESE.to_string(),
            model_name: "base".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Model 'base' cannot transcribe 'yue'. Load a Cantonese-capable model \
             (large-v3 family, or a registered Cantonese model)."
        );
    }

    #[test]
    fn other_languages_pass_through_unchanged() {
        assert_eq!(
            engine_language_for("en", "base", None),
            LanguageResolution::Forced("en")
        );
        assert_eq!(
            engine_language_for("zh", "small", None),
            LanguageResolution::Forced("zh")
        );
        assert_eq!(
            engine_language_for("auto", "base", None),
            LanguageResolution::AutoDetect { translate: false }
        );
        assert_eq!(
            engine_language_for("auto-translate", "base", None),
            LanguageResolution::AutoDetect { translate: true }
        );
    }

    #[test]
    fn capability_rule_covers_the_whole_large_v3_family_and_nothing_else() {
        assert!(is_cantonese_capable_builtin("large-v3"));
        assert!(is_cantonese_capable_builtin("large-v3-turbo"));
        assert!(is_cantonese_capable_builtin("large-v3-q5_0"));
        assert!(!is_cantonese_capable_builtin("medium"));
        assert!(!is_cantonese_capable_builtin("large-v2"));
    }

    #[test]
    fn prompt_combines_seed_and_vocabulary() {
        let prompt = build_initial_prompt(CANTONESE, Some("陳大文、Zackriya")).unwrap();
        assert!(prompt.starts_with(CANTONESE_PROMPT_SEED));
        assert!(prompt.ends_with("陳大文、Zackriya"));
    }

    #[test]
    fn vocabulary_alone_is_a_prompt_for_any_language() {
        assert_eq!(
            build_initial_prompt("en", Some("Zackriya, Meetily")).as_deref(),
            Some("Zackriya, Meetily")
        );
        assert_eq!(build_initial_prompt("en", None), None);
        assert_eq!(build_initial_prompt("en", Some("   ")), None);
    }

    #[test]
    fn prompt_is_truncated_but_keeps_the_seed() {
        let long_vocab = "詞".repeat(500);
        let prompt = build_initial_prompt(CANTONESE, Some(&long_vocab)).unwrap();
        assert_eq!(prompt.chars().count(), MAX_PROMPT_CHARS);
        // Truncation must not eat the seed, which is what biases script and register.
        let seed_head: String = CANTONESE_PROMPT_SEED.chars().take(10).collect();
        assert!(prompt.starts_with(&seed_head));
    }
}
