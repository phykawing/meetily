# Context

Glossary of domain terms for Meetily. No implementation details — see `docs/adr/` for decisions.

## Language and script

### Transcription Language
The language the speech-recognition engine is told to expect. Chosen by the user before
recording, or auto-detected. Distinct from **Summary Language** — a Cantonese meeting can
produce an English summary.

### Summary Language
The language the LLM writes the summary in. Independent of **Transcription Language**, and
may be auto-selected from the transcript.

### Script
Which Han character set written Chinese appears in: **Traditional (繁體)** or
**Simplified (简体)**. Orthogonal to language and to **Written Form** — the same Cantonese
sentence can be written in either script. Traditional has regional conventions; Meetily
targets the **Hong Kong** convention.

### Written Form
Which written register Cantonese speech is transcribed into:

- **口語 (colloquial written Cantonese)** — mirrors what was said: 係, 唔係, 嘅, 喺, 咗.
- **書面語 (Standard Written Chinese)** — what Cantonese speakers conventionally *write*:
  是, 不是, 的, 在, 了.

Not a script difference: both can be written in either **Script**. Converting 口語 → 書面語
is a rewrite, not a character mapping.

### Canonical Transcript
The stored record of what was actually said: verbatim, in the **Written Form** the speaker
used. It is the single source of truth — everything else about a meeting is derived from
it. Editing it changes the record; producing a **Rendering** does not.

### Rendering
A derived presentation of a **Canonical Transcript** — for example the same meeting
rewritten from 口語 into 書面語. A Rendering is disposable: it can always be regenerated
from the Canonical Transcript, and is discarded when that transcript changes. Summaries
are produced from the Canonical Transcript, never from a Rendering.

## Speakers

### Audio Source
Which capture stream audio came from: the **microphone** (the person using this computer)
or **system audio** (everyone joining through the meeting app). Known at capture time, and
destroyed once the two streams are mixed.

Note: much of the existing Rust code uses the word "speaker" to mean an *audio output
device* (loudspeaker). That is unrelated to **Speaker** below. Prefer "output device" in
new code.

### Speaker
One distinct human voice in a meeting, identified by how the voice sounds rather than by
which stream carried it. A **Speaker** is discovered by diarization, not declared in
advance.

### Speaker Turn
A contiguous stretch of audio attributed to a single **Speaker**. Turn boundaries are
found from the audio and do not necessarily line up with transcript segment boundaries.

## Configuration

### Setting
A named preference persisted app-wide (one value for the whole app, not per meeting) —
for example Script or the Rendering provider. Distinct from a **Rendering**, which is
per-meeting and derived rather than chosen, and from the per-meeting Written Form
preference: both of those live on the meeting itself, not in app-wide Settings.
