export interface Message {
  id: string;
  content: string;
  timestamp: string;
}

export interface Transcript {
  id: string;
  text: string;
  timestamp: string; // Wall-clock time (e.g., "14:30:05")
  sequence_id?: number;
  chunk_start_time?: number; // Legacy field
  is_partial?: boolean;
  confidence?: number;
  // NEW: Recording-relative timestamps for playback sync
  audio_start_time?: number; // Seconds from recording start (e.g., 125.3)
  audio_end_time?: number;   // Seconds from recording start (e.g., 128.6)
  duration?: number;          // Segment duration in seconds (e.g., 3.3)
  // Live Audio Source hint (see docs/adr/0001, docs/adr/0004): "mic" | "system" | "mixed".
  // Undefined for meetings recorded before this field existed.
  audio_source?: string;
  // The diarized Speaker's per-meeting label (e.g. "speaker_00"), distinct from
  // audio_source - see docs/adr/0004. Undefined until a diarization pass covers this
  // chunk. Resolve to a display name via the meeting's speakers list (get_meeting_speakers).
  speaker_label?: string;
  // True when diarization found more than one distinct speaker overlapping this segment -
  // it's still attributed to the dominant one, but the attribution is a best guess.
  // Undefined for a chunk not yet covered by diarization, same as speaker_label.
  speaker_uncertain?: boolean;
}

export interface TranscriptUpdate {
  text: string;
  timestamp: string; // Wall-clock time for reference
  // Live Audio Source hint: "mic" | "system" | "mixed" (see docs/adr/0001, docs/adr/0004)
  source: string;
  sequence_id: number;
  chunk_start_time: number; // Legacy field
  is_partial: boolean;
  confidence: number;
  // NEW: Recording-relative timestamps for playback sync
  audio_start_time: number; // Seconds from recording start
  audio_end_time: number;   // Seconds from recording start
  duration: number;          // Segment duration in seconds
}

export interface Block {
  id: string;
  type: string;
  content: string;
  color: string;
}

export interface Section {
  title: string;
  blocks: Block[];
}

export interface Summary {
  [key: string]: Section;
}

export interface ApiResponse {
  message: string;
  num_chunks: number;
  data: any[];
}

export interface SummaryResponse {
  status: string;
  summary: Summary;
  raw_summary?: string;
  usage?: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
}

// BlockNote-specific types
export type SummaryFormat = 'legacy' | 'markdown' | 'blocknote';

export interface BlockNoteBlock {
  id: string;
  type: string;
  props?: Record<string, any>;
  content?: any[];
  children?: BlockNoteBlock[];
}

export interface SummaryDataResponse {
  markdown?: string;
  summary_json?: BlockNoteBlock[];
  // Legacy format fields
  MeetingName?: string;
  _section_order?: string[];
  [key: string]: any; // For legacy section data
}

// Pagination types for optimized transcript loading
export interface MeetingMetadata {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  folder_path?: string;
}

export interface PaginatedTranscriptsResponse {
  transcripts: Transcript[];
  total_count: number;
  has_more: boolean;
}

// Transcript segment data for virtualized display
export interface TranscriptSegmentData {
  id: string;
  timestamp: number; // audio_start_time in seconds
  endTime?: number; // audio_end_time in seconds
  text: string;
  confidence?: number;
  // Live Audio Source hint: "mic" | "system" | "mixed". Undefined when not captured
  // (e.g. meetings recorded before this existed, or non-live views).
  audioSource?: string;
  // The diarized Speaker's per-meeting label (e.g. "speaker_00"). Undefined until a
  // diarization pass covers this segment.
  speakerLabel?: string;
  // True when the segment straddled a speaker change and was assigned the dominant one.
  speakerUncertain?: boolean;
}
