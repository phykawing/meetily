# The canonical transcript is verbatim 口語; 書面語 is a derived rendering

Cantonese is spoken in 口語 (係, 唔係, 嘅, 喺) but conventionally *written* in 書面語
(是, 不是, 的, 在), and users legitimately want either. Whisper cannot be switched between
registers by a parameter — a model emits whatever register it was fine-tuned on — so the
choice cannot live in the ASR layer. We store exactly what was said, in 口語, as the
**Canonical Transcript**, and treat 書面語 as a **Rendering**: an LLM-produced, cached,
regenerable view that is discarded whenever the underlying transcript changes.

## Consequences

- There is one source of truth, and it is the one people appeal to when they disagree about
  what was said. 口語 is the default view; the 書面語 toggle is per-meeting and persists.
- Summaries are generated from the Canonical Transcript, never from a Rendering — otherwise
  one model's rewriting errors compound into another model's summary.
- Rendering is an LLM pass over the *entire* transcript, which makes it a privacy decision,
  not a formatting one. It therefore has its own provider setting, defaulting to the local
  built-in model, and warns explicitly when pointed at a cloud provider. A display toggle
  must never quietly upload a meeting.
