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

use crate::database::repositories::setting_store::SettingToken;
use serde::Serialize;

/// Which register a meeting's transcript is displayed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrittenForm {
    /// Verbatim 口語 — the Canonical Transcript itself. The default.
    Colloquial,
    /// 書面語 — a cached Rendering produced by the local model.
    Written,
}

impl SettingToken for WrittenForm {
    const TOKENS: &'static [(&'static str, Self)] = &[
        ("colloquial", WrittenForm::Colloquial),
        ("written", WrittenForm::Written),
    ];
}

impl WrittenForm {
    /// Token stored in the database.
    pub fn as_str(self) -> &'static str {
        <Self as SettingToken>::as_token(self)
    }

    /// Resolves a stored token into a setting. Unrecognized or absent values fall back to
    /// the default (口語), matching a user picking from a fixed two-way toggle.
    pub fn from_stored(token: Option<&str>) -> Self {
        <Self as SettingToken>::from_token(token)
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

impl SettingToken for RenderingProvider {
    const TOKENS: &'static [(&'static str, Self)] = &[
        ("local", RenderingProvider::Local),
        ("summary_provider", RenderingProvider::SummaryProvider),
    ];
}

impl RenderingProvider {
    /// Token stored in the database.
    pub fn as_str(self) -> &'static str {
        <Self as SettingToken>::as_token(self)
    }

    /// Resolves a stored token into a setting. Unrecognized or absent values fall back to
    /// `Local` — the privacy-safe default, since an unrecognized value must never be
    /// treated as consent to use a remote provider.
    pub fn from_stored(token: Option<&str>) -> Self {
        <Self as SettingToken>::from_token(token)
    }
}

impl Default for RenderingProvider {
    fn default() -> Self {
        RenderingProvider::Local
    }
}

/// One segment of the Canonical Transcript together with the diarized Speaker (if any)
/// attributed to it. `speaker_label` is the same per-meeting cluster id (`speaker_00`, ...)
/// everything else resolves through `meeting_speakers` — never a display name — so renaming
/// a speaker never invalidates a cached Rendering (see `fingerprint_segments`): the name is
/// resolved at read time from the current `speaker_label` -> name map, exactly like the 口語
/// view already does.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalSegment {
    pub text: String,
    pub speaker_label: Option<String>,
}

/// One Speaker Turn in a 書面語 Rendering: consecutive rendered text attributed to one
/// Speaker, or to nobody — for a meeting that was never diarized, or for a chunk whose
/// speaker markers couldn't be reconciled after the rewrite (see `reconcile_rendered_chunk`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderedTurn {
    pub speaker_label: Option<String>,
    pub text: String,
}

/// Fingerprints a Canonical Transcript's ordered segments (text and speaker label), so a
/// cached Rendering can be checked for staleness without keeping a second full copy of the
/// source text around. Reuses `summary::service::stable_text_fingerprint` (FNV-1a) rather
/// than a second, independently-implemented hash — and unlike
/// `std::collections::hash_map::DefaultHasher`, it's not subject to changing between Rust
/// versions. Speaker labels are folded in so a diarization re-run (which can reattribute the
/// same text to different labels, or attribute previously-unattributed text) invalidates the
/// cache even though the transcript text itself didn't change.
pub fn fingerprint_segments(segments: &[CanonicalSegment]) -> String {
    // NUL between a segment's label and text, and SOH between segments, so no combination
    // of label/text lengths can collide with a different segment sequence; transcript text
    // and speaker labels never legitimately contain either control character.
    let joined = segments
        .iter()
        .map(|s| format!("{}\u{0}{}", s.speaker_label.as_deref().unwrap_or(""), s.text))
        .collect::<Vec<_>>()
        .join("\u{1}");
    crate::summary::service::stable_text_fingerprint(&joined)
}

/// The line a Speaker Turn boundary is marked with in both the prompt sent to the LLM and
/// the text parsed back out of its response. Not valid Chinese or Markdown, so asking the
/// model to copy it verbatim doesn't compete with the register-rewrite instruction, and
/// parsing it back out can't collide with real rendered content.
const TURN_MARKER_PREFIX: &str = "@@speaker-turn:";

fn turn_marker_line(label: &str) -> String {
    format!("{TURN_MARKER_PREFIX}{label}")
}

/// A line's speaker label, if the line (already trimmed by the caller) is exactly a turn
/// marker line.
fn parse_marker_line(trimmed_line: &str) -> Option<&str> {
    trimmed_line.strip_prefix(TURN_MARKER_PREFIX).filter(|label| !label.is_empty())
}

/// Turns `segments` into the flat list of units `build_rendering_chunks` groups into
/// chunks: each segment's own text, except the first segment of each new Speaker Turn (a
/// run of consecutive segments sharing the same `speaker_label`), which gets its turn
/// marker line prepended as part of the *same* unit. Keeping the marker glued to its
/// segment's text (rather than as its own separate unit) guarantees `build_rendering_chunks`
/// can never split a marker from the text it introduces across a chunk boundary — chunking
/// never splits a pushed unit, only groups whole ones. A meeting where every segment has
/// `speaker_label: None` (never diarized) gets no markers at all, so the unit list is
/// exactly the segment texts — byte-identical to Rendering's behavior before turn markers
/// existed.
pub fn interleave_turn_markers(segments: &[CanonicalSegment]) -> Vec<String> {
    let mut units = Vec::with_capacity(segments.len());
    let mut current_label: Option<&str> = None;

    for segment in segments {
        let label = segment.speaker_label.as_deref();
        if label.is_some() && label != current_label {
            units.push(format!("{}\n{}", turn_marker_line(label.unwrap()), segment.text));
        } else {
            units.push(segment.text.clone());
        }
        current_label = label;
    }

    units
}

/// Turn markers found in `text`, in order — used to compare what a chunk's rewritten
/// output actually contains against what its input prompt asked it to preserve.
fn extract_markers(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| parse_marker_line(line.trim()))
        .map(|label| label.to_string())
        .collect()
}

/// Removes every turn-marker line from `text`.
fn strip_markers(text: &str) -> String {
    text.lines()
        .filter(|line| parse_marker_line(line.trim()).is_none())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Reconciles one rendered chunk against the turn markers that were actually sent as that
/// chunk's input prompt. The system prompt asks the model to copy marker lines verbatim,
/// but nothing enforces that — if the markers that come back don't match the ones that went
/// in, in order, this falls back to `rendered` with every marker-looking line stripped, so a
/// mangled or hallucinated marker never survives into the stored Rendering. An unlabeled
/// chunk is a safe, testable failure mode; a mislabeled one is not.
pub fn reconcile_rendered_chunk(input_chunk: &str, rendered: &str) -> String {
    let expected = extract_markers(input_chunk);
    let actual = extract_markers(rendered);

    if expected == actual {
        return rendered.to_string();
    }

    if !expected.is_empty() {
        log::warn!(
            "Rendering chunk's speaker turn markers changed during rewrite (expected {:?}, got {:?}); discarding markers for this chunk",
            expected,
            actual
        );
    }
    strip_markers(rendered)
}

/// Parses a stored or freshly generated Rendering's text back into Speaker Turns for
/// display and copy — the inverse of `interleave_turn_markers` plus the rewrite. Works
/// uniformly on a meeting that was never diarized, and on a Rendering cached before turn
/// markers existed: neither has any marker line, so both come back as a single
/// `speaker_label: None` turn holding the whole text — exactly what every Rendering was
/// before this feature. Adjacent turns that end up sharing a label (e.g. one Speaker Turn
/// split across a chunk boundary) are merged into one.
pub fn parse_rendered_text(rendered_text: &str) -> Vec<RenderedTurn> {
    let mut turns: Vec<RenderedTurn> = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    for line in rendered_text.lines() {
        if let Some(label) = parse_marker_line(line.trim()) {
            flush_turn(&current_label, &mut current_lines, &mut turns);
            current_label = Some(label.to_string());
        } else {
            current_lines.push(line);
        }
    }
    flush_turn(&current_label, &mut current_lines, &mut turns);

    turns
}

fn flush_turn(label: &Option<String>, lines: &mut Vec<&str>, turns: &mut Vec<RenderedTurn>) {
    if lines.is_empty() {
        return;
    }
    let text = lines.join("\n");
    lines.clear();

    if let Some(last) = turns.last_mut() {
        if last.speaker_label == *label {
            last.text.push('\n');
            last.text.push_str(&text);
            return;
        }
    }
    turns.push(RenderedTurn { speaker_label: label.clone(), text });
}

/// System prompt for the 口語→書面語 register rewrite. This is a rewrite within Chinese, not
/// a translation or a summary: meaning, speaker order and every fact must be preserved.
pub const RENDERING_SYSTEM_PROMPT: &str = "\
You are an expert editor of written Chinese. Rewrite the given Cantonese meeting transcript \
from 口語 (spoken, colloquial register) into 書面語 (Standard Written Chinese). Preserve the \
original meaning, speaker order, and every fact exactly — this is a register rewrite, not a \
translation or a summary. Change only register-specific vocabulary and grammar (for example \
係→是, 唔係→不是, 嘅→的, 喺→在, 咗→了). Text that is already 書面語, or is not Chinese, must be \
left unchanged. Some lines begin with \"@@speaker-turn:\" followed by an identifier — these \
are speaker-turn markers, not transcript content. Copy every such line exactly as given, \
verbatim, alone on its own line, in the same position relative to the surrounding text. \
Never translate, remove, merge, or add such lines. Output only the rewritten transcript \
text, with no preamble, headings, or explanation.";

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

    fn seg(text: &str) -> CanonicalSegment {
        CanonicalSegment { text: text.to_string(), speaker_label: None }
    }

    fn seg_with(text: &str, label: &str) -> CanonicalSegment {
        CanonicalSegment { text: text.to_string(), speaker_label: Some(label.to_string()) }
    }

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
        let segments = vec![seg("係咁㗎啦"), seg("唔該晒")];
        assert_eq!(
            fingerprint_segments(&segments),
            fingerprint_segments(&segments)
        );
    }

    #[test]
    fn fingerprint_changes_when_any_segment_text_changes() {
        let original = vec![seg("係咁㗎啦"), seg("唔該晒")];
        let mut edited = original.clone();
        edited[1] = seg("唔該晒晒");

        assert_ne!(fingerprint_segments(&original), fingerprint_segments(&edited));
    }

    #[test]
    fn fingerprint_changes_when_segments_are_appended_or_removed() {
        let shorter = vec![seg("係咁㗎啦")];
        let longer = vec![seg("係咁㗎啦"), seg("唔該晒")];

        assert_ne!(fingerprint_segments(&shorter), fingerprint_segments(&longer));
    }

    #[test]
    fn fingerprint_does_not_collide_across_a_shifted_segment_boundary() {
        let a = vec![seg("ab"), seg("c")];
        let b = vec![seg("a"), seg("bc")];

        assert_ne!(fingerprint_segments(&a), fingerprint_segments(&b));
    }

    #[test]
    fn fingerprint_is_order_sensitive() {
        let forward = vec![seg("first"), seg("second")];
        let reversed = vec![seg("second"), seg("first")];

        assert_ne!(fingerprint_segments(&forward), fingerprint_segments(&reversed));
    }

    #[test]
    fn fingerprint_changes_when_a_speaker_label_changes_but_text_does_not() {
        let unlabeled = vec![seg("hello")];
        let labeled = vec![seg_with("hello", "speaker_00")];

        assert_ne!(fingerprint_segments(&unlabeled), fingerprint_segments(&labeled));
    }

    #[test]
    fn fingerprint_changes_when_the_speaker_label_itself_changes() {
        let a = vec![seg_with("hello", "speaker_00")];
        let b = vec![seg_with("hello", "speaker_01")];

        assert_ne!(fingerprint_segments(&a), fingerprint_segments(&b));
    }

    // interleave_turn_markers --------------------------------------------------

    #[test]
    fn undiarized_segments_produce_no_markers() {
        let segments = vec![seg("first"), seg("second"), seg("third")];
        assert_eq!(interleave_turn_markers(&segments), vec!["first", "second", "third"]);
    }

    #[test]
    fn a_new_speaker_label_gets_a_marker_glued_to_its_first_segment() {
        let segments = vec![seg_with("hi", "speaker_00"), seg_with("there", "speaker_00")];
        let units = interleave_turn_markers(&segments);

        assert_eq!(units, vec!["@@speaker-turn:speaker_00\nhi", "there"]);
    }

    #[test]
    fn a_speaker_change_gets_a_fresh_marker() {
        let segments = vec![seg_with("hi", "speaker_00"), seg_with("hello", "speaker_01")];
        let units = interleave_turn_markers(&segments);

        assert_eq!(
            units,
            vec!["@@speaker-turn:speaker_00\nhi", "@@speaker-turn:speaker_01\nhello"]
        );
    }

    #[test]
    fn returning_to_a_label_after_an_unattributed_gap_gets_a_new_marker() {
        let segments =
            vec![seg_with("hi", "speaker_00"), seg("uncertain"), seg_with("again", "speaker_00")];
        let units = interleave_turn_markers(&segments);

        assert_eq!(
            units,
            vec!["@@speaker-turn:speaker_00\nhi", "uncertain", "@@speaker-turn:speaker_00\nagain"]
        );
    }

    // reconcile_rendered_chunk ---------------------------------------------------

    #[test]
    fn reconcile_keeps_markers_that_round_trip_unchanged() {
        let input = "@@speaker-turn:speaker_00\n係咁㗎啦";
        let rendered = "@@speaker-turn:speaker_00\n是這樣的";
        assert_eq!(reconcile_rendered_chunk(input, rendered), rendered);
    }

    #[test]
    fn reconcile_strips_markers_when_the_model_drops_one() {
        let input = "@@speaker-turn:speaker_00\nfirst\n@@speaker-turn:speaker_01\nsecond";
        let rendered = "first\n@@speaker-turn:speaker_01\nsecond"; // dropped the first marker
        assert_eq!(reconcile_rendered_chunk(input, rendered), "first\nsecond");
    }

    #[test]
    fn reconcile_strips_a_hallucinated_marker_not_present_in_the_input() {
        let input = "plain text, never diarized";
        let rendered = "@@speaker-turn:speaker_00\nplain text, never diarized";
        assert_eq!(
            reconcile_rendered_chunk(input, rendered),
            "plain text, never diarized"
        );
    }

    #[test]
    fn reconcile_is_a_no_op_when_neither_input_nor_output_has_markers() {
        assert_eq!(reconcile_rendered_chunk("plain", "書面語"), "書面語");
    }

    // parse_rendered_text ---------------------------------------------------------

    #[test]
    fn text_with_no_markers_parses_as_one_unlabeled_turn() {
        let turns = parse_rendered_text("line one\nline two");
        assert_eq!(
            turns,
            vec![RenderedTurn { speaker_label: None, text: "line one\nline two".to_string() }]
        );
    }

    #[test]
    fn empty_text_parses_to_no_turns() {
        assert!(parse_rendered_text("").is_empty());
    }

    #[test]
    fn marked_text_splits_into_labeled_turns() {
        let turns = parse_rendered_text(
            "@@speaker-turn:speaker_00\n是這樣的\n@@speaker-turn:speaker_01\n唔該晒",
        );
        assert_eq!(
            turns,
            vec![
                RenderedTurn { speaker_label: Some("speaker_00".to_string()), text: "是這樣的".to_string() },
                RenderedTurn { speaker_label: Some("speaker_01".to_string()), text: "唔該晒".to_string() },
            ]
        );
    }

    #[test]
    fn leading_unlabeled_text_before_the_first_marker_becomes_its_own_turn() {
        let turns = parse_rendered_text("intro line\n@@speaker-turn:speaker_00\nlater");
        assert_eq!(
            turns,
            vec![
                RenderedTurn { speaker_label: None, text: "intro line".to_string() },
                RenderedTurn { speaker_label: Some("speaker_00".to_string()), text: "later".to_string() },
            ]
        );
    }

    #[test]
    fn adjacent_turns_sharing_a_label_across_a_chunk_boundary_are_merged() {
        // Two chunks reconciled independently can each open with the same speaker's
        // marker if the turn was split by chunking; the merged result must read as one
        // turn, not two back-to-back headers.
        let turns = parse_rendered_text(
            "@@speaker-turn:speaker_00\nfirst half\n@@speaker-turn:speaker_00\nsecond half",
        );
        assert_eq!(
            turns,
            vec![RenderedTurn {
                speaker_label: Some("speaker_00".to_string()),
                text: "first half\nsecond half".to_string(),
            }]
        );
    }

    #[test]
    fn round_trip_from_segments_through_chunking_and_back_preserves_turns() {
        let segments = vec![
            seg_with("係咁㗎啦", "speaker_00"),
            seg_with("唔該晒", "speaker_00"),
            seg_with("好呀", "speaker_01"),
        ];
        let units = interleave_turn_markers(&segments);
        let chunks = build_rendering_chunks(&units, 10_000);
        // A single large-enough chunk budget keeps everything in one chunk here, so
        // there is exactly one chunk to reconcile and no chunk-boundary splitting to
        // account for.
        assert_eq!(chunks.len(), 1);
        let rendered = reconcile_rendered_chunk(&chunks[0], &chunks[0]);
        let turns = parse_rendered_text(&rendered);

        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].speaker_label.as_deref(), Some("speaker_00"));
        assert_eq!(turns[0].text, "係咁㗎啦\n唔該晒");
        assert_eq!(turns[1].speaker_label.as_deref(), Some("speaker_01"));
        assert_eq!(turns[1].text, "好呀");
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
