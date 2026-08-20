// rendering/mod.rs
//
// Written Form rendering: a per-meeting toggle between 口語 (colloquial, what was actually
// said) and 書面語 (Standard Written Chinese). The Canonical Transcript stays verbatim 口語;
// 書面語 is a Rendering — a cached, disposable view produced by the local model, discarded
// whenever the underlying transcript changes. See
// docs/adr/0002-canonical-transcript-is-verbatim-colloquial.md and phykawing/meetily#8.
//
// Cache invalidation is fingerprint-at-read rather than a hook on every transcript writer:
// there is no single choke point where the Canonical Transcript changes (live recording,
// retranscription, import all write it independently), so `get_transcript_rendering` always
// recomputes the current fingerprint and compares it to the cached one before trusting the
// cache.

pub mod commands;

/// Which register a meeting's transcript is displayed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrittenForm {
    /// Verbatim 口語 — the Canonical Transcript itself. The default.
    Colloquial,
    /// 書面語 — a cached Rendering produced by the local model.
    Written,
}

impl WrittenForm {
    /// Token stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            WrittenForm::Colloquial => "colloquial",
            WrittenForm::Written => "written",
        }
    }

    /// Resolves a stored token into a setting. Unrecognized or absent values fall back to
    /// the default (口語), matching a user picking from a fixed two-way toggle.
    pub fn from_stored(token: Option<&str>) -> Self {
        match token {
            Some("written") => WrittenForm::Written,
            _ => WrittenForm::Colloquial,
        }
    }
}

impl Default for WrittenForm {
    fn default() -> Self {
        WrittenForm::Colloquial
    }
}

/// Which LLM provider performs a Rendering. A Rendering is a full-transcript LLM pass, so
/// this is a distinct, explicit privacy decision from the summary provider setting — see
/// docs/adr/0002 and phykawing/meetily#15.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderingProvider {
    /// The app's local Built-in AI model. The default — no transcript text leaves the
    /// machine.
    Local,
    /// Whichever provider is currently configured for summaries. May be a cloud provider;
    /// the UI warns explicitly before this can be selected.
    SummaryProvider,
}

impl RenderingProvider {
    /// Token stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            RenderingProvider::Local => "local",
            RenderingProvider::SummaryProvider => "summary_provider",
        }
    }

    /// Resolves a stored token into a setting. Unrecognized or absent values fall back to
    /// `Local` — the privacy-safe default, since an unrecognized value must never be
    /// treated as consent to use a remote provider.
    pub fn from_stored(token: Option<&str>) -> Self {
        match token {
            Some("summary_provider") => RenderingProvider::SummaryProvider,
            _ => RenderingProvider::Local,
        }
    }
}

impl Default for RenderingProvider {
    fn default() -> Self {
        RenderingProvider::Local
    }
}

/// Fingerprints a Canonical Transcript's ordered segment texts, so a cached Rendering can be
/// checked for staleness without keeping a second full copy of the source text around.
/// Reuses `summary::service::stable_text_fingerprint` (FNV-1a) rather than a second,
/// independently-implemented hash — and unlike `std::collections::hash_map::DefaultHasher`,
/// it's not subject to changing between Rust versions.
pub fn fingerprint_segments<S: AsRef<str>>(segments: &[S]) -> String {
    // NUL as a separator so ("ab", "c") and ("a", "bc") don't collide; transcript text never
    // legitimately contains it.
    let joined = segments
        .iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join("\u{0}");
    crate::summary::service::stable_text_fingerprint(&joined)
}

/// System prompt for the 口語→書面語 register rewrite. This is a rewrite within Chinese, not
/// a translation or a summary: meaning, speaker order and every fact must be preserved.
pub const RENDERING_SYSTEM_PROMPT: &str = "\
You are an expert editor of written Chinese. Rewrite the given Cantonese meeting transcript \
from 口語 (spoken, colloquial register) into 書面語 (Standard Written Chinese). Preserve the \
original meaning, speaker order, and every fact exactly — this is a register rewrite, not a \
translation or a summary. Change only register-specific vocabulary and grammar (for example \
係→是, 唔係→不是, 嘅→的, 喺→在, 咗→了). Text that is already 書面語, or is not Chinese, must be \
left unchanged. Output only the rewritten transcript text, with no preamble, headings, or \
explanation.";

/// Builds the per-chunk user prompt for a Rendering pass.
pub fn build_rendering_user_prompt(chunk: &str) -> String {
    format!("<transcript>\n{chunk}\n</transcript>")
}

/// Groups ordered Canonical Transcript segments into chunks for a Rendering pass, each at
/// most `chunk_size_tokens` (by `rough_token_count`), joined with newlines.
///
/// Deliberately does not character-slice the joined text the way
/// `summary::processor::chunk_text` does for summaries: that function's word/sentence
/// boundary search can trim a chunk's end backward without moving the next chunk's start to
/// match, silently dropping the text in between. A register rewrite promises to preserve
/// "every fact exactly", so this groups whole segments instead — every segment lands in
/// exactly one chunk, in order, and none are ever dropped. A single segment longer than the
/// budget still gets its own chunk rather than being split or lost.
pub fn build_rendering_chunks<S: AsRef<str>>(segments: &[S], chunk_size_tokens: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_tokens = 0usize;

    for segment in segments {
        let text = segment.as_ref();
        let segment_tokens = crate::summary::processor::rough_token_count(text);

        if !current.is_empty() && current_tokens + segment_tokens > chunk_size_tokens {
            chunks.push(std::mem::take(&mut current));
            current_tokens = 0;
        }

        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(text);
        current_tokens += segment_tokens;
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_token_round_trips() {
        for form in [WrittenForm::Colloquial, WrittenForm::Written] {
            assert_eq!(WrittenForm::from_stored(Some(form.as_str())), form);
        }
    }

    #[test]
    fn missing_or_unknown_stored_token_defaults_to_colloquial() {
        assert_eq!(WrittenForm::from_stored(None), WrittenForm::Colloquial);
        assert_eq!(
            WrittenForm::from_stored(Some("bogus")),
            WrittenForm::Colloquial
        );
    }

    #[test]
    fn rendering_provider_round_trips_through_stored_tokens() {
        for provider in [RenderingProvider::Local, RenderingProvider::SummaryProvider] {
            assert_eq!(RenderingProvider::from_stored(Some(provider.as_str())), provider);
        }
    }

    #[test]
    fn missing_or_unknown_rendering_provider_token_defaults_to_local() {
        assert_eq!(RenderingProvider::from_stored(None), RenderingProvider::Local);
        assert_eq!(
            RenderingProvider::from_stored(Some("bogus")),
            RenderingProvider::Local
        );
    }

    #[test]
    fn fingerprint_is_stable_for_identical_input() {
        let segments = vec!["係咁㗎啦".to_string(), "唔該晒".to_string()];
        assert_eq!(
            fingerprint_segments(&segments),
            fingerprint_segments(&segments)
        );
    }

    #[test]
    fn fingerprint_changes_when_any_segment_text_changes() {
        let original = vec!["係咁㗎啦".to_string(), "唔該晒".to_string()];
        let mut edited = original.clone();
        edited[1] = "唔該晒晒".to_string();

        assert_ne!(fingerprint_segments(&original), fingerprint_segments(&edited));
    }

    #[test]
    fn fingerprint_changes_when_segments_are_appended_or_removed() {
        let shorter = vec!["係咁㗎啦".to_string()];
        let longer = vec!["係咁㗎啦".to_string(), "唔該晒".to_string()];

        assert_ne!(fingerprint_segments(&shorter), fingerprint_segments(&longer));
    }

    #[test]
    fn fingerprint_does_not_collide_across_a_shifted_segment_boundary() {
        let a = vec!["ab".to_string(), "c".to_string()];
        let b = vec!["a".to_string(), "bc".to_string()];

        assert_ne!(fingerprint_segments(&a), fingerprint_segments(&b));
    }

    #[test]
    fn fingerprint_is_order_sensitive() {
        let forward = vec!["first".to_string(), "second".to_string()];
        let reversed = vec!["second".to_string(), "first".to_string()];

        assert_ne!(fingerprint_segments(&forward), fingerprint_segments(&reversed));
    }

    /// Every segment's text must survive chunking somewhere in the output, regardless of
    /// chunk size — this is the property that matters for a Rendering, since dropping text
    /// would silently violate "preserve every fact exactly".
    fn assert_no_text_is_lost(segments: &[&str], chunk_size_tokens: usize) {
        let chunks = build_rendering_chunks(segments, chunk_size_tokens);
        let rejoined: String = chunks.join("\n");
        for segment in segments {
            assert!(
                rejoined.contains(segment),
                "segment {:?} missing from chunks {:?} (chunk_size_tokens={})",
                segment,
                chunks,
                chunk_size_tokens
            );
        }
    }

    #[test]
    fn chunking_never_drops_a_segment_regardless_of_budget() {
        let segments = [
            "係咁㗎啦，今日開會傾吓個project嘅進度。",
            "第一樣嘢係要review低個timeline。",
            "跟住我哋要decide埋個budget點分配。",
            "仲有就係下次開會嘅時間未定。",
            "唔該晒大家。",
        ];

        // A budget smaller than any single segment still can't lose text.
        assert_no_text_is_lost(&segments, 1);
        // A budget that fits a couple of segments per chunk.
        assert_no_text_is_lost(&segments, 20);
        // A budget large enough for everything in one chunk.
        assert_no_text_is_lost(&segments, 10_000);
    }

    #[test]
    fn small_budget_still_produces_one_chunk_per_segment_rather_than_dropping_any() {
        let segments = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let chunks = build_rendering_chunks(&segments, 1);
        assert_eq!(chunks, vec!["a", "b", "c"]);
    }

    #[test]
    fn segments_are_grouped_up_to_the_budget_and_joined_with_newlines() {
        let segments = vec!["aa".to_string(), "bb".to_string(), "cc".to_string()];
        // rough_token_count("aa") rounds up to 1 token, so a budget of 2 fits two segments.
        let chunks = build_rendering_chunks(&segments, 2);
        assert_eq!(chunks, vec!["aa\nbb", "cc"]);
    }

    #[test]
    fn empty_segment_list_produces_no_chunks() {
        let segments: Vec<String> = vec![];
        assert!(build_rendering_chunks(&segments, 100).is_empty());
    }
}
