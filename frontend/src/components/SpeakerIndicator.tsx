'use client';

import { useEffect, useState } from 'react';
import { AlertTriangle } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover';
import { Input } from './ui/input';
import { Button } from './ui/button';

interface SpeakerIndicatorProps {
  /** Resolved display name (e.g. "Speaker 1"). Renders nothing when undefined - meetings
   * that haven't been diarized yet, or segments a diarization pass didn't cover. */
  name?: string;
  /** True when this segment straddled a speaker change (diarization::alignment) and was
   * assigned the dominant speaker as a best guess. */
  uncertain?: boolean;
  /** This speaker's per-meeting cluster id (e.g. "speaker_00"). Passed alongside
   * `onRename` to make the badge an inline rename control. */
  label?: string;
  /** Persists a rename for `label`. When supplied (with `label`), the name badge becomes a
   * button that opens a small editor; the rename applies to every segment of this speaker
   * at once because they all resolve through the same per-meeting label -> name map. */
  onRename?: (label: string, name: string) => void | Promise<void>;
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
 * directly picks a stable palette slot; a user-renamed speaker falls back to a hash of
 * the name so it still gets a stable, distinct color.
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
export const SpeakerIndicator: React.FC<SpeakerIndicatorProps> = ({ name, uncertain, label, onRename }) => {
  if (!name) {
    return uncertain ? <UncertainFlag /> : null;
  }

  const badgeClasses = `text-[10px] font-medium px-1.5 py-0.5 rounded-full ${speakerBadgeClasses(name)}`;

  return (
    <span className="inline-flex items-center gap-1 mb-1">
      {label && onRename ? (
        <SpeakerNameEditor
          name={name}
          label={label}
          onRename={onRename}
          triggerClassName={`${badgeClasses} cursor-pointer hover:opacity-80 transition-opacity`}
        />
      ) : (
        <span className={badgeClasses}>{name}</span>
      )}
      {uncertain && <UncertainFlag />}
    </span>
  );
};

/**
 * The name badge as an inline rename control: click to open a one-field editor seeded with
 * the current name. Enter or Save commits; Escape or Cancel discards. Empty / whitespace-
 * only input is rejected client-side (the backend command trims and rejects it too).
 */
function SpeakerNameEditor({
  name,
  label,
  onRename,
  triggerClassName,
}: {
  name: string;
  label: string;
  onRename: (label: string, name: string) => void | Promise<void>;
  triggerClassName: string;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(name);
  const [saving, setSaving] = useState(false);

  // Reseed the field whenever the editor opens or the resolved name changes underneath it
  // (e.g. a diarization re-run reset it to "Speaker N").
  useEffect(() => {
    if (open) setDraft(name);
  }, [open, name]);

  const commit = async () => {
    const trimmed = draft.trim();
    if (!trimmed || trimmed === name) {
      setOpen(false);
      return;
    }
    setSaving(true);
    try {
      await onRename(label, trimmed);
      setOpen(false);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button type="button" className={triggerClassName} title="Rename this speaker">
          {name}
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-56 p-2" align="start">
        <div className="space-y-2">
          <label className="text-xs font-medium text-gray-600">Rename speaker</label>
          <Input
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                commit();
              } else if (e.key === 'Escape') {
                e.preventDefault();
                setOpen(false);
              }
            }}
            className="h-8 text-sm"
          />
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button size="sm" className="h-7 px-2 text-xs" onClick={commit} disabled={saving}>
              Save
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

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
