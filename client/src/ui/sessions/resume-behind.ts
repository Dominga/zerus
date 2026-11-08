import { tr } from "../../i18n";

// Task #247: Qt's SessionsWindow::resumeBehind (SessionsWindow.cpp:6048): when
// `hgs resume` refuses because Claude's transcript is behind Zerus's events,
// the owner chooses the lost dialogue, the stale conversation as it is, or
// Cancel. The refusal ends with a JSON object: {"error":"transcript_behind",
// "transcript_at":…,"events_at":…,"missing":N}; older ones only name the lag.

export interface TranscriptBehind {
  transcriptAt: number;
  eventsAt: number;
  missing: number;
  /** Seconds the transcript stops before the recorded events. */
  lag: number;
}

export function transcriptBehind(detail: string): TranscriptBehind | undefined {
  if (!detail.includes("transcript_behind")) return undefined;
  let transcriptAt = 0, eventsAt = 0, missing = 0;
  const json = /\{"error"\s*:\s*"transcript_behind"[^}]*\}/.exec(detail)?.[0];
  if (json) {
    try {
      const value = JSON.parse(json) as Record<string, unknown>;
      transcriptAt = Number(value.transcript_at) || 0;
      eventsAt = Number(value.events_at) || 0;
      missing = Number(value.missing) || 0;
    } catch { /* the plain-text lag form still names the gap */ }
  }
  const lag = /stops\s+(\d+)\s*s\s+before/.exec(detail);
  return { transcriptAt, eventsAt, missing, lag: lag ? Number(lag[1]) : 0 };
}

/** Qt's " (%1 entries missing)" suffix, empty when none are known missing. */
export function missingSuffix(missing: number): string {
  return missing > 0 ? tr(" (%1 entries missing)", missing) : "";
}

/** Qt's lag in whole hours when it is an hour or more, else in minutes (at least one). */
export function lagText(seconds: number): string {
  if (seconds <= 0) return "";
  const minutes = Math.floor(seconds / 60);
  return minutes >= 60 ? tr("%1 h", Math.floor(minutes / 60)) : tr("%1 min", Math.max(1, minutes));
}

/** Qt's "d MMM HH:mm" for a Unix time. */
export function defaultWhen(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

/** The dialog's body: both times when the refusal names them, else the lag, else only the gap. */
export function resumeBehindText(behind: TranscriptBehind, when: (seconds: number) => string = defaultWhen): string {
  const missing = missingSuffix(behind.missing);
  if (behind.transcriptAt > 0 && behind.eventsAt > 0) {
    return tr("Claude's transcript stops at %1, but Zerus recorded the conversation up to %2%3.\n\nResuming as it is restores the older conversation and leaves that recorded dialogue out.",
      when(behind.transcriptAt), when(behind.eventsAt), missing);
  }
  if (behind.lag > 0) {
    return tr("Claude's transcript stops %1 before the events Zerus recorded%2.\n\nResuming as it is restores the older conversation and leaves that recorded dialogue out.",
      lagText(behind.lag), missing);
  }
  return tr("Claude's transcript is behind the events Zerus recorded%1.\n\nResuming as it is restores the older conversation and leaves that recorded dialogue out.", missing);
}
