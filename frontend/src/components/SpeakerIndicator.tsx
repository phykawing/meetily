'use client';

import { AlertTriangle } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip';

interface SpeakerIndicatorProps {
  /** Resolved display name (e.g. "Speaker 1"). Renders nothing when undefined - meetings
   * that haven't been diarized yet, or segments a diarization pass didn't cover. */
  name?: string;
  /** True when this segment straddled a speaker change (diarization::alignment) and was
   * assigned the dominant speaker as a best guess. */
  uncertain?: boolean;
}

const BADGE_PALETTE = [
  'bg-blue-50 text-blue-700',
  'bg-purple-50 text-purple-700',
  'bg-emerald-50 text-emerald-700',
  'bg-amber-50 text-amber-700',
  'bg-pink-50 text-pink-700',
  'bg-cyan-50 text-cyan-700',
  'bg-rose-50 text-rose-700',
  'bg-indigo-50 text-indigo-700',
];

const BORDER_PALETTE = [
  'border-blue-300',
  'border-purple-300',
  'border-emerald-300',
  'border-amber-300',
  'border-pink-300',
  'border-cyan-300',
  'border-rose-300',
  'border-indigo-300',
];

/**
 * Deterministic color index for a speaker's display name. Names from diarization are
 * "Speaker N" (numbered by first appearance - see `default_speaker_names`), so N-1
 * directly picks a stable palette slot; a custom-renamed speaker (future rename feature)
 * falls back to a hash of the name so it still gets a stable, distinct color.
 */
export function speakerColorIndex(name: string): number {
  const match = name.match(/(\d+)\s*$/);
  if (match) {
    return parseInt(match[1], 10) - 1;
  }
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  }
  return hash;
}

export function speakerBadgeClasses(name: string): string {
  const index = ((speakerColorIndex(name) % BADGE_PALETTE.length) + BADGE_PALETTE.length) % BADGE_PALETTE.length;
  return BADGE_PALETTE[index];
}

export function speakerBorderClasses(name: string): string {
  const index = ((speakerColorIndex(name) % BORDER_PALETTE.length) + BORDER_PALETTE.length) % BORDER_PALETTE.length;
  return BORDER_PALETTE[index];
}

/**
 * Speaker name badge shown above the first segment of a consecutive run from one
 * speaker, plus an "uncertain attribution" flag when this particular segment straddled a
 * speaker change.
 */
export const SpeakerIndicator: React.FC<SpeakerIndicatorProps> = ({ name, uncertain }) => {
  if (!name) {
    return uncertain ? <UncertainFlag /> : null;
  }

  return (
    <span className="inline-flex items-center gap-1 mb-1">
      <span className={`text-[10px] font-medium px-1.5 py-0.5 rounded-full ${speakerBadgeClasses(name)}`}>
        {name}
      </span>
      {uncertain && <UncertainFlag />}
    </span>
  );
};

function UncertainFlag() {
  return (
    <Tooltip>
      <TooltipTrigger>
        <AlertTriangle className="h-3 w-3 text-amber-500" />
      </TooltipTrigger>
      <TooltipContent>
        Speaker attribution uncertain - this segment may span a speaker change.
      </TooltipContent>
    </Tooltip>
  );
}
