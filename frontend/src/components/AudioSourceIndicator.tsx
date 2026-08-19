'use client';

interface AudioSourceIndicatorProps {
  // Live Audio Source hint: "mic" | "system" | "mixed" (see docs/adr/0001, docs/adr/0004)
  audioSource?: string;
}

const LABELS: Record<string, string> = {
  mic: 'Mic',
  system: 'System',
  mixed: 'Mixed',
};

const STYLES: Record<string, string> = {
  mic: 'bg-blue-50 text-blue-600',
  system: 'bg-purple-50 text-purple-600',
  mixed: 'bg-amber-50 text-amber-600',
};

/**
 * Small badge showing which capture stream a live transcript segment came from.
 * Renders nothing for meetings/segments without a captured hint (e.g. recorded
 * before this existed), so older transcripts still display correctly.
 */
export const AudioSourceIndicator: React.FC<AudioSourceIndicatorProps> = ({ audioSource }) => {
  if (!audioSource || !LABELS[audioSource]) {
    return null;
  }

  return (
    <span
      className={`text-[10px] font-medium px-1.5 py-0.5 rounded-full flex-shrink-0 ${STYLES[audioSource]}`}
      title={
        audioSource === 'mixed'
          ? 'Simultaneous speech from microphone and system audio'
          : `Captured from ${LABELS[audioSource].toLowerCase()} audio`
      }
    >
      {LABELS[audioSource]}
    </span>
  );
};
