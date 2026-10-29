import type { ActivityEvent, Reply } from "../../types";
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


/** A message's files as Qt's attachment cards show them (ActivityView.cpp:759-782). */
export function attachments(e: ActivityEvent): ActivityItem["attachments"] {
  const list = Array.isArray(e.attachments) ? e.attachments : [];
  if (!list.length) return undefined;
  return list.map((a) => {
    const text = (value: unknown) => (typeof value === "string" ? value : "");
    const name = text(a?.name), reference = text(a?.reference), mime = text(a?.mime);
    const bytes = typeof a?.bytes === "number" && Number.isFinite(a.bytes) ? a.bytes : 0;
    return {
      name,
      caption: (reference ? `${reference} ` : "") + name,
      mime,
      meta: bytes > 0 ? `${Math.max(1, Math.ceil(bytes / 1024))} KiB` : "File",
      request_id: text(a?.request_id),
      index: typeof a?.index === "number" && Number.isInteger(a.index) ? a.index : -1,
      image: mime.startsWith("image/") && mime !== "image/svg+xml",
    };
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

/**
 * The request and reply the summary recorded, when the timeline has no events
 * (a journal that kept nothing of this conversation). As in Qt, they are shown
 * as preceding context, never as new messages.
 */
function recordedContext(summary: Record<string, unknown> | undefined): TranscriptItem[] {
  const prompt = typeof summary?.prompt === "string" ? summary.prompt.trim() : "";
  const answer = typeof summary?.last_message === "string" ? summary.last_message.trim() : "";
  if (!prompt && !answer) return [];
  const items: TranscriptItem[] = [{ id: "context", kind: "context", at: "", body: "Recorded context — outside the available timeline" }];
  // A recorded prompt that holds Codex replies reads as one (Rust's prompt_replies).
  const replies = Array.isArray(summary?.prompt_replies) && summary.prompt_replies.length ? summary.prompt_replies as Reply[] : undefined;
  if (prompt) items.push({ id: "prompt-snapshot", kind: "user", at: "", label: replies ? "You (recorded answer)" : "You (recorded request)", body: prompt, replies });
  if (answer) items.push({ id: "answer-snapshot", kind: "agent", at: "", label: "Agent (recorded response)", body: answer });
  return items;
}

/** The items of a transcript, oldest first; ids are stable while events only append. */
export function buildTranscript(events: readonly ActivityEvent[], summary?: Record<string, unknown>): TranscriptItem[] {
  if (!events.length) return recordedContext(summary);
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
      // An answer to a question card, or a message holding Codex replies (ActivityView.cpp:1036-1038).
      const replies = role === "user" && e.question_replies?.length ? e.question_replies : undefined;
      // Qt's order: a queued prompt first, then an answer (ActivityView.cpp:1036-1038).
      const label = role !== "user" ? undefined : e.type === "UserPromptQueued" ? "You (queued)"
        : e.type === "QuestionAnswered" || replies ? "You (answer)" : undefined;
      items.push({ id, kind: role === "user" ? "user" : "agent", at, body: e.detail ?? "", attachments: attachments(e), receipt, label, replies });
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
