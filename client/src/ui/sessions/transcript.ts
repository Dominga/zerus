import type { ActivityEvent } from "../../types";
import { eventKey } from "../../store/activity";
import type { ActivityItem, MediaMessage, TranscriptItem, VoiceState } from "./types";

// Journal events as the Activity items of the approved design, after the
// rules of tray/src/ActivityView.cpp: messages by role, thinking, cleared
// sessions, notices (subagent reports, peer messages, background tasks),
// voice and video messages, and adjacent operational events collapsed into one
// tool group (at most 24, never across a message or an attention event).

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

const EVENT_TITLE: Record<string, string> = {
  SessionStart: "Session started", SessionEnd: "Session closed",
  TurnStarted: "Turn started", UserPromptQueued: "Message queued",
  PreToolUse: "Tool started", PostToolUse: "Tool finished",
  PostToolUseFailure: "Tool failed", PermissionRequest: "Approval requested",
  PermissionResult: "Approval resolved", Stop: "Response finished",
  StopFailure: "Response failed", Interrupt: "Turn interrupted",
  PreCompact: "Compacting context", PostCompact: "Context compacted",
  SubagentStart: "Subagent started", SubagentStop: "Subagent finished",
  TaskStarted: "Background task started", Notification: "Agent notification",
};
export const eventTitle = (type: string | undefined) => (type ? EVENT_TITLE[type] ?? type : "Activity");

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/** "Oct 27 09:12", local time, as the design shows message times. */
export function timeText(at: number | undefined): string {
  if (!at || at <= 0) return "";
  const d = new Date(at * 1000);
  return `${MONTHS[d.getMonth()]} ${d.getDate()} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

type Role = "user" | "assistant" | "notice" | "";

/** ActivityView.cpp messageRole: who wrote an event, if anyone. */
export function messageRole(e: ActivityEvent): Role {
  if (e.source === "zerus_task" && e.type === "TaskEntry") return e.task_role === "person" ? "user" : e.task_role === "agent" ? "assistant" : "";
  if (e.source === "hgs_voice" && e.voice && typeof e.voice === "object") return "user";
  if (e.agent_id || (!e.detail && !(e.attachments?.length))) return "";
  if (e.origin === "task_notification" || e.origin === "subagent_report" || e.origin === "peer_message") return "notice";
  switch (e.type) {
    case "UserPromptSubmit": case "UserPromptQueued": case "TurnStarted": case "QuestionAnswered": case "LocalMessage": return "user";
    case "Stop": case "AgentMessage": return "assistant";
  }
  return "";
}

/** Approvals and failures open their group and are never merged with others. */
const attention = (e: ActivityEvent) => e.type === "PermissionRequest" || e.type === "PostToolUseFailure" || e.type === "StopFailure" || e.type === "Interrupt";

function size(bytes: unknown): string {
  const n = typeof bytes === "number" ? bytes : 0;
  if (!n) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function attachments(e: ActivityEvent): ActivityItem["attachments"] {
  const list = Array.isArray(e.attachments) ? e.attachments : [];
  if (!list.length) return undefined;
  return list.map((a) => {
    const record = a as Record<string, unknown>;
    const name = String(record.name ?? record.reference ?? "attachment");
    const mime = String(record.mime ?? "");
    return { name, size: size(record.bytes), kind: mime.startsWith("image/") ? "image" : "file" };
  });
}

function media(e: ActivityEvent, id: string): MediaMessage {
  const voice = (e.voice ?? {}) as Record<string, unknown>;
  const audio = (voice.audio ?? {}) as Record<string, unknown>;
  const video = (voice.video ?? undefined) as Record<string, unknown> | undefined;
  const state = String(voice.state ?? "uncertain") as VoiceState;
  const waveform = Array.isArray(audio.waveform) ? (audio.waveform as unknown[]).map((v) => Math.max(0, Math.min(255, Number(v) || 0))) : [];
  return {
    id, kind: "media", media: video && Object.keys(video).length ? "video" : "voice", at: timeText(e.at),
    state: state in VOICE_STATE_LABEL ? state : "uncertain",
    durationMs: Math.max(0, Number(audio.duration_ms) || 0),
    waveform: waveform.length === 64 ? waveform : [],
    transcript: typeof voice.transcript === "string" && voice.transcript ? voice.transcript : undefined,
    error: typeof voice.error === "string" && voice.error ? voice.error : undefined,
  };
}

const GROUP_LIMIT = 24;
const PREVIEW = 110;
const preview = (text: string | undefined) => {
  const flat = (text ?? "").replace(/\s+/g, " ").trim();
  return flat.length > PREVIEW ? `${flat.slice(0, PREVIEW - 1)}…` : flat;
};

/** The items of a transcript, oldest first; ids are stable while events only append. */
export function buildTranscript(events: readonly ActivityEvent[]): TranscriptItem[] {
  const items: TranscriptItem[] = [];
  for (let i = 0; i < events.length;) {
    const e = events[i];
    const id = eventKey(e);
    const role = messageRole(e);
    const at = timeText(e.at);
    if (e.type === "SessionCleared") {
      items.push({ id, kind: "cleared", at, sender: "Session cleared", body: "The agent no longer sees the activity above. New messages start with a fresh conversation context." });
      i++; continue;
    }
    if (e.type === "AgentThinking") {
      if ((e.detail ?? "").trim()) items.push({ id, kind: "thinking", at, body: e.detail });
      i++; continue;
    }
    if (role === "notice") {
      const report = typeof e.report === "string" ? e.report : "";
      if (e.origin === "subagent_report") items.push({ id, kind: "report", at, title: preview(e.detail) || "Subagent report", body: report || e.detail, sender: e.sender || undefined });
      else items.push({ id, kind: "system", at, sender: e.origin === "peer_message" ? `Message from ${e.sender ?? "another session"}` : "Background task", body: report || e.detail });
      i++; continue;
    }
    if (role === "user" && e.source === "hgs_voice") {
      items.push(media(e, id));
      i++; continue;
    }
    if (role) {
      let receipt: string | undefined;
      if (role === "user") {
        let accepted = e.type !== "UserPromptQueued";
        for (let j = i + 1; !accepted && j < events.length; j++) {
          const next = events[j];
          if ((next.type === "UserPromptSubmit" || next.type === "TurnStarted") && next.detail === e.detail) accepted = true;
        }
        receipt = accepted ? "Sent" : "Queued at this time";
      }
      items.push({ id, kind: role === "user" ? "user" : "agent", at, body: e.detail ?? "", attachments: attachments(e), receipt });
      i++; continue;
    }
    // Adjacent operational events form one group; messages and attention stay boundaries.
    const group = [e];
    i++;
    while (i < events.length && group.length < GROUP_LIMIT && events[i].type !== "AgentThinking" && events[i].type !== "SessionCleared"
      && !messageRole(events[i]) && !attention(e) && !attention(events[i])) group.push(events[i++]);
    const tools: string[] = [];
    let calls = 0;
    for (const g of group) {
      if (g.tool && !tools.includes(g.tool)) tools.push(g.tool);
      if (g.type === "PreToolUse") calls++;
    }
    const last = group[group.length - 1];
    const lines = group.map((g) => `${eventTitle(g.type)}${g.tool ? ` / ${g.tool}` : ""}${g.agent_id ? ` (subagent ${g.agent_id})` : ""}  ${preview(g.detail)}`);
    const shownTools = tools.slice(0, 3).join(", ") + (tools.length > 3 ? " …" : "");
    if (group.length === 1) {
      items.push({ id, kind: "tool", at: timeText(last.at), title: `${eventTitle(e.type)}${e.tool ? ` / ${e.tool}` : ""}`, body: preview(e.detail), lines, warn: attention(e) });
    } else {
      const title = calls > 0 ? `${calls === 1 ? "1 tool call" : `${calls} tool calls`}, ${group.length} events` : `${group.length} activity events`;
      items.push({ id, kind: "tools", at: timeText(last.at), title, body: shownTools || preview(last.detail), lines, warn: group.some(attention) });
    }
  }
  return items;
}
