use std::collections::HashMap;

use serde::Serialize;
use whatlang::{detect, Lang};

use super::processor::language_name_from_code;
use crate::script::{detect_script, DetectedScript};
use crate::whisper_engine::language::CANTONESE;

const MIN_MEANINGFUL_CHARS: usize = 20;
const MIN_RELIABLE_CONFIDENCE: f64 = 0.25;

/// Summary-language code (BCP-47) used when a Chinese transcript is written in
/// Traditional characters — Cantonese meetings resolve here per phykawing/meetily#14.
const TRADITIONAL_CHINESE_SUMMARY_LANGUAGE: &str = "zh-tw";
const CHINESE_SUMMARY_LANGUAGE: &str = "zh";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryLanguageDetectionReason {
    Detected,
    Tie,
    LowConfidence,
    Unsupported,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SummaryLanguageDetection {
    pub language: Option<String>,
    pub reason: SummaryLanguageDetectionReason,
}

/// Resolves the summary language for a transcript, consulting the Transcription Language
/// when one is known (see phykawing/meetily#14).
///
/// Cantonese transcription forces Traditional Chinese unconditionally: Whisper's `zh`
/// decode tends toward Simplified, so a Cantonese transcript can be stored Simplified (or,
/// under "leave as recognized", left however the model emitted it) and character-based
/// detection alone would not reliably resolve it to Traditional. When no Transcription
/// Language is available to consult — imports, auto-detected meetings — this falls back to
/// [`detect_summary_language`], which detects script from the transcript itself.
pub(crate) fn resolve_summary_language(
    transcription_language: Option<&str>,
    transcript_texts: &[String],
) -> SummaryLanguageDetection {
    if transcription_language == Some(CANTONESE) {
        return SummaryLanguageDetection {
            language: Some(TRADITIONAL_CHINESE_SUMMARY_LANGUAGE.to_string()),
            reason: SummaryLanguageDetectionReason::Detected,
        };
    }

    detect_summary_language(transcript_texts)
}

pub(crate) fn detect_summary_language(transcript_texts: &[String]) -> SummaryLanguageDetection {
    let mut weights: HashMap<&'static str, usize> = HashMap::new();
    let mut saw_meaningful_text = false;
    let mut saw_low_confidence = false;
    let mut saw_unsupported = false;

    for text in transcript_texts {
        let cleaned = text.trim();
        let meaningful_chars = meaningful_char_count(cleaned);
        if meaningful_chars < MIN_MEANINGFUL_CHARS {
            continue;
        }
        saw_meaningful_text = true;

        let Some(info) = detect(cleaned) else {
            saw_low_confidence = true;
            continue;
        };

        if !info.is_reliable() && info.confidence() < MIN_RELIABLE_CONFIDENCE {
            saw_low_confidence = true;
            continue;
        }

        let Some(code) = summary_code_from_whatlang(info.lang()) else {
            saw_unsupported = true;
            continue;
        };
        if language_name_from_code(code).is_none() {
            saw_unsupported = true;
            continue;
        }

        *weights.entry(code).or_insert(0) += meaningful_chars;
    }

    if weights.is_empty() {
        return SummaryLanguageDetection {
            language: None,
            reason: if !saw_meaningful_text {
                SummaryLanguageDetectionReason::Empty
            } else if saw_low_confidence {
                SummaryLanguageDetectionReason::LowConfidence
            } else if saw_unsupported {
                SummaryLanguageDetectionReason::Unsupported
            } else {
                SummaryLanguageDetectionReason::LowConfidence
            },
        };
    }

    let detection = summarize_weighted_detection(weights);
    refine_chinese_script(detection, transcript_texts)
}

/// whatlang reports Mandarin and Cantonese alike as generic "Chinese" and cannot tell
/// Traditional from Simplified. When it resolves to Chinese, disambiguate by which
/// characters the transcript actually uses (see `script::detect_script`) rather than
/// leaving the ambiguous "zh" code, which downstream prompts render as unqualified
/// "Chinese" and models tend to answer in Simplified.
fn refine_chinese_script(
    detection: SummaryLanguageDetection,
    transcript_texts: &[String],
) -> SummaryLanguageDetection {
    if detection.language.as_deref() != Some(CHINESE_SUMMARY_LANGUAGE) {
        return detection;
    }

    let combined: String = transcript_texts.join("\n");
    match detect_script(&combined) {
        DetectedScript::Traditional => SummaryLanguageDetection {
            language: Some(TRADITIONAL_CHINESE_SUMMARY_LANGUAGE.to_string()),
            ..detection
        },
        DetectedScript::Simplified | DetectedScript::Neither => detection,
    }
}

fn summarize_weighted_detection(
    weights: HashMap<&'static str, usize>,
) -> SummaryLanguageDetection {
    let mut best: Option<(&'static str, usize)> = None;
    let mut tied = false;

    for (code, weight) in weights {
        match best {
            None => {
                best = Some((code, weight));
                tied = false;
            }
            Some((_, best_weight)) if weight > best_weight => {
                best = Some((code, weight));
                tied = false;
            }
            Some((_, best_weight)) if weight == best_weight => {
                tied = true;
            }
            _ => {}
        }
    }

    if tied {
        SummaryLanguageDetection {
            language: None,
            reason: SummaryLanguageDetectionReason::Tie,
        }
    } else {
        SummaryLanguageDetection {
            language: best.map(|(code, _)| code.to_string()),
            reason: SummaryLanguageDetectionReason::Detected,
        }
    }
}

fn meaningful_char_count(text: &str) -> usize {
    text.chars().filter(|c| c.is_alphabetic()).count()
}

fn summary_code_from_whatlang(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::Eng => Some("en"),
        Lang::Cmn => Some("zh"),
        Lang::Deu => Some("de"),
        Lang::Spa => Some("es"),
        Lang::Rus => Some("ru"),
        Lang::Kor => Some("ko"),
        Lang::Fra => Some("fr"),
        Lang::Jpn => Some("ja"),
        Lang::Por => Some("pt"),
        Lang::Ita => Some("it"),
        Lang::Nld => Some("nl"),
        Lang::Pol => Some("pl"),
        Lang::Ara => Some("ar"),
        Lang::Hin => Some("hi"),
        Lang::Tam => Some("ta"),
        Lang::Tur => Some("tr"),
        Lang::Vie => Some("vi"),
        Lang::Tha => Some("th"),
        Lang::Ind => Some("id"),
        Lang::Swe => Some("sv"),
        Lang::Ces => Some("cs"),
        Lang::Dan => Some("da"),
        Lang::Fin => Some("fi"),
        Lang::Ell => Some("el"),
        Lang::Heb => Some("he"),
        Lang::Hun => Some("hu"),
        Lang::Nob => Some("no"),
        Lang::Ron => Some("ro"),
        Lang::Ukr => Some("uk"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn transcript_summary_language_detects_english() {
        let texts = strings(&[
            "The team reviewed the roadmap, discussed release blockers, and agreed on the next engineering milestones.",
        ]);

        assert_eq!(detect_summary_language(&texts).language, Some("en".to_string()));
    }

    #[test]
    fn transcript_summary_language_detects_chinese() {
        let texts = strings(&[
            "团队讨论了产品路线图、发布风险以及下一阶段的工程计划，并确认了后续负责人。",
        ]);

        assert_eq!(detect_summary_language(&texts).language, Some("zh".to_string()));
    }

    #[test]
    fn cantonese_meeting_resolves_to_traditional_chinese() {
        // No transcript text needed: a known Cantonese Transcription Language forces
        // Traditional unconditionally, even if the stored transcript happens to be
        // Simplified (e.g. under "leave as recognized").
        let texts = strings(&["团队讨论了产品路线图。"]);

        let detection = resolve_summary_language(Some(CANTONESE), &texts);

        assert_eq!(detection.language, Some("zh-tw".to_string()));
        assert_eq!(detection.reason, SummaryLanguageDetectionReason::Detected);
    }

    #[test]
    fn imported_traditional_transcript_resolves_to_traditional_chinese() {
        // No Transcription Language to consult (e.g. an import): falls back to detecting
        // the script the transcript is actually written in.
        let texts = strings(&["團隊討論咗產品路線圖、發佈風險以及下一階段嘅工程計劃。"]);

        assert_eq!(
            resolve_summary_language(None, &texts).language,
            Some("zh-tw".to_string())
        );
    }

    #[test]
    fn non_cantonese_transcription_language_does_not_force_traditional() {
        let texts = strings(&[
            "团队讨论了产品路线图、发布风险以及下一阶段的工程计划，并确认了后续负责人。",
        ]);

        assert_eq!(
            resolve_summary_language(Some("zh"), &texts).language,
            Some("zh".to_string())
        );
    }

    #[test]
    fn simplified_chinese_transcript_stays_generic_chinese() {
        let texts = strings(&[
            "团队讨论了产品路线图、发布风险以及下一阶段的工程计划，并确认了后续负责人。",
        ]);

        assert_eq!(detect_summary_language(&texts).language, Some("zh".to_string()));
    }

    #[test]
    fn non_chinese_detection_is_unaffected_by_script_refinement() {
        let texts = strings(&[
            "The team reviewed the roadmap, discussed release blockers, and agreed on the next engineering milestones.",
        ]);

        assert_eq!(
            resolve_summary_language(None, &texts).language,
            Some("en".to_string())
        );
    }

    #[test]
    fn transcript_summary_language_uses_weighted_dominant_language() {
        let texts = strings(&[
            "团队讨论了产品路线图、发布风险以及下一阶段的工程计划，并确认了后续负责人。",
            "The team spent most of the meeting reviewing launch risks, customer feedback, engineering follow-ups, staffing constraints, and the next release timeline.",
            "The group also assigned owners for documentation, QA verification, support readiness, and deployment communications.",
        ]);

        assert_eq!(detect_summary_language(&texts).language, Some("en".to_string()));
    }

    #[test]
    fn transcript_summary_language_reports_tie_reason() {
        let result = summarize_weighted_detection(HashMap::from([("en", 40), ("es", 40)]));

        assert_eq!(result.language, None);
        assert_eq!(result.reason, SummaryLanguageDetectionReason::Tie);
    }

    #[test]
    fn transcript_summary_language_returns_none_for_short_or_unsupported_text() {
        let texts = strings(&["ok", "12345", "..."]);

        assert_eq!(detect_summary_language(&texts).language, None);
    }

    #[test]
    fn transcript_summary_language_never_returns_unsupported_summary_code() {
        for lang in Lang::all() {
            if let Some(code) = summary_code_from_whatlang(*lang) {
                assert!(
                    language_name_from_code(code).is_some(),
                    "mapped unsupported summary language code {code}"
                );
            }
        }
    }
}
