// Voice and round-video messages for the default demo transcript (synthetic).
// The journal event shape follows ActivityView: source "hgs_voice" with a voice
// object (state, audio duration and a 64-value waveform, optional video, and the
// recognized transcript).
import { activity } from "./data";
import type { ActivityItem } from "./types";

export type VoiceState =
  | "uploading" | "queued" | "transcribing" | "ready" | "waiting_agent"
  | "delivering" | "submitted" | "failed" | "cancelled" | "uncertain";

/** Delivery labels, as in ActivityView.cpp (unknown states read "Delivery not confirmed"). */
export const VOICE_STATE_LABEL: Record<VoiceState, string> = {
  uploading: "Sending recording…",
  queued: "Waiting for recognition",
  transcribing: "Recognizing…",
  ready: "Ready to deliver",
  waiting_agent: "Waiting for agent",
  delivering: "Delivering…",
  submitted: "Accepted by native app",
  failed: "Voice message failed",
  cancelled: "Cancelled",
  uncertain: "Delivery not confirmed",
};

/** Tooltip detail: only Accepted explains what it does not confirm. */
export function voiceStateDetail(state: VoiceState): string {
  return state === "submitted"
    ? "The native app accepted the recognized text. Queue status, when present, is shown alongside this recording. This does not confirm model delivery, reading or completion."
    : VOICE_STATE_LABEL[state];
}

/** States drawn with the warning glyph (VoicePlayerImage::deliveryIcon). */
export const voiceStateIssue = (state: VoiceState) => state === "failed" || state === "uncertain";

export interface MediaMessage {
  id: string;
  kind: "media";
  media: "voice" | "video";
  at: string;
  state: VoiceState;
  durationMs: number;
  /** Exactly 64 integer amplitudes 0–255; anything else draws a flat line. */
  waveform: number[];
  transcript?: string;
  error?: string;
}

export type TranscriptItem = ActivityItem | MediaMessage;

// A deterministic speech-like envelope: syllable bursts with short pauses.
function waveform(seed: number): number[] {
  let x = seed;
  const rand = () => ((x = (x * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
  return Array.from({ length: 64 }, (_, i) => {
    const syllable = Math.abs(Math.sin(i * 0.55 + seed)) ** 0.7;
    const pause = i % 17 === 16 || i % 23 === 0 ? 0.12 : 1;
    const fade = Math.min(1, (i + 2) / 6, (66 - i) / 8);
    return Math.max(0, Math.min(255, Math.round((40 + 200 * syllable * (0.55 + 0.45 * rand())) * pause * fade)));
  });
}

export const voiceMessage: MediaMessage = {
  id: "a3c",
  kind: "media",
  media: "voice",
  at: "Oct 27 11:02",
  state: "submitted",
  durationMs: 14_000,
  waveform: waveform(7),
  transcript: "Keep the old offset parameter working for one more release, but log a deprecation warning.",
};

export const videoMessage: MediaMessage = {
  id: "a3d",
  kind: "media",
  media: "video",
  at: "Oct 27 11:06",
  state: "submitted",
  durationMs: 12_000,
  waveform: waveform(3),
  transcript: "One more thing: the second page on staging repeats two orders when new ones arrive. Check the cursor against that case before you finish.",
};

/** api-refactor's transcript with its recorded voice message and a round video note. */
export const demoTranscript: TranscriptItem[] = activity.flatMap((it): TranscriptItem[] =>
  it.kind === "voice" ? [voiceMessage, videoMessage] : [it],
);
