'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, Users } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Label } from '../ui/label';
import { fetchSpeakerNames } from '@/lib/meeting-speakers';
import {
  MAX_EXPECTED_SPEAKERS,
  MIN_EXPECTED_SPEAKERS,
  parseExpectedSpeakerCount,
} from '@/lib/expected-speaker-count';

interface SpeakerDetectionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  meetingId: string;
  /**
   * Called once the user confirms, with the parsed expected-speaker hint (`null` = detect
   * automatically). The dialog closes itself immediately after; the caller owns the
   * actual `run_diarization_command` invoke and its progress/error reporting.
   */
  onConfirm: (expectedSpeakers: number | null) => void;
}

/**
 * Gathers the two inputs a speaker-detection run needs before it starts
 * (phykawing/meetily#19):
 *
 * - an **optional** expected attendee count, which pins clustering when supplied and is
 *   never required;
 * - the user's acknowledgement, shown only when the meeting already has discovered
 *   speakers, that re-running discards the names assigned to them (clustering is not
 *   stable across runs - see docs/adr/0001).
 *
 * Cancelling - the Cancel button, Escape, or a click outside - changes nothing: no invoke,
 * no state written. The caller only learns anything happened through `onConfirm`.
 */
export function SpeakerDetectionDialog({
  open,
  onOpenChange,
  meetingId,
  onConfirm,
}: SpeakerDetectionDialogProps) {
  const [countInput, setCountInput] = useState('');
  // How many speakers this meeting currently has from a prior pass. 'loading' until the
  // count is back; 'error' if it could not be fetched. The discard warning shows for a
  // positive count AND for 'error' - a re-run discards names either way, so a failed count
  // must fail safe toward showing the warning, not hiding it.
  const [existingSpeakers, setExistingSpeakers] = useState<number | 'loading' | 'error'>('loading');

  const prevOpenRef = useRef(false);

  useEffect(() => {
    const wasOpen = prevOpenRef.current;
    prevOpenRef.current = open;
    if (!open || wasOpen) return;

    // Closed -> open: reset the field and re-count the meeting's speakers. Counting on
    // open (not on mount) means a pass that finished while this dialog was closed - e.g.
    // the automatic post-recording one - is reflected the next time it opens.
    setCountInput('');
    setExistingSpeakers('loading');

    let cancelled = false;
    fetchSpeakerNames(meetingId)
      .then((names) => {
        if (!cancelled) setExistingSpeakers(Object.keys(names).length);
      })
      .catch((error) => {
        console.error('Failed to count existing speakers:', error);
        if (!cancelled) setExistingSpeakers('error');
      });

    return () => {
      cancelled = true;
    };
  }, [open, meetingId]);

  const parsed = parseExpectedSpeakerCount(countInput);
  const countIsInvalid = parsed.kind === 'invalid';
  // 'loading' is the one state that suppresses the warning: it resolves within a moment of
  // the dialog opening, and showing then hiding the warning would be worse than a brief
  // absence. Every other state (a positive count, or a failed fetch) shows it.
  const willDiscardNames =
    existingSpeakers === 'error' || (typeof existingSpeakers === 'number' && existingSpeakers > 0);

  const handleConfirm = () => {
    if (countIsInvalid) return;
    onConfirm(parsed.kind === 'valid' ? parsed.count : null);
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[440px]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Users className="h-5 w-5 text-blue-600" />
            {willDiscardNames ? 'Re-run speaker detection' : 'Detect speakers'}
          </DialogTitle>
          <DialogDescription>
            Speaker detection listens to this meeting&rsquo;s recording and labels each part
            of the transcript by who was speaking.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {willDiscardNames && (
            <div className="flex gap-2 rounded-md border border-amber-200 bg-amber-50 p-3">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
              <p className="text-sm text-amber-800">
                This meeting&rsquo;s current speaker labels &mdash; including any names
                you&rsquo;ve assigned to them &mdash; will be discarded. Detection finds
                speakers from scratch each time and can&rsquo;t carry the old names over.
              </p>
            </div>
          )}

          <div className="space-y-2">
            <Label htmlFor="expected-speaker-count" className="text-sm font-medium">
              Expected number of speakers <span className="text-muted-foreground">(optional)</span>
            </Label>
            <Input
              id="expected-speaker-count"
              type="text"
              inputMode="numeric"
              placeholder="Detect automatically"
              value={countInput}
              onChange={(e) => setCountInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleConfirm();
              }}
              aria-invalid={countIsInvalid}
              className={countIsInvalid ? 'border-red-400 focus-visible:ring-red-400' : undefined}
            />
            <p className={`text-xs ${countIsInvalid ? 'text-red-600' : 'text-muted-foreground'}`}>
              {countIsInvalid
                ? `Enter a whole number from ${MIN_EXPECTED_SPEAKERS} to ${MAX_EXPECTED_SPEAKERS}, or leave this blank.`
                : 'If you know how many people attended, this helps detection get it right. Leave it blank to detect automatically.'}
            </p>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleConfirm} disabled={countIsInvalid} className="bg-blue-600 hover:bg-blue-700">
            <Users className="mr-2 h-4 w-4" />
            {willDiscardNames ? 'Re-run detection' : 'Run detection'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
