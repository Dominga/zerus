import type { ActivityEvent, LocalMessage, Reply } from "../../types";
import { eventKey } from "../../store/activity";
import type { ActivityItem, LocalItem, MediaMessage, TranscriptItem, VoiceState } from "./types";

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

// ---------------------------------------------------------------------------
// Local message rows (Qt's m_localMessages): what this app sent until the
// agent's own history shows it, and what failed (ActivityView.cpp:665-720,
// 1053-1068).

const PROMPTS = new Set(["UserPromptSubmit", "UserPromptQueued", "TurnStarted"]);
const LOCAL_STAMP: Record<LocalMessage["status"], string> = { sending: "Sending…", error: "Not sent", sent: "Submitted to terminal" };
/** How far a sent row's own prompt event may be from it, in seconds (ActivityView.cpp:684). */
const NEAREST = 10;

/** The text the agent received (the receipt's, else the row's), trimmed. */
const submittedOf = (row: LocalMessage) => (row.submitted_text ?? row.text).trim();

/**
 * Whether a local row's file can be opened: it has its receipt's request or
 * its cached copy (Qt's rule, ActivityView.cpp:765). Activity's journal cards
 * read a file by its request only (attachment-card.ts `cardTarget`).
 */
export const localFileAvailable = (file: Record<string, unknown>) =>
  (typeof file.request_id === "string" && file.request_id !== "") || (typeof file.local_path === "string" && file.local_path !== "");

/** A failed or pending row's stamp, error line and buttons (ActivityView.cpp:1053-1068). */
export function localItem(row: LocalMessage, summary?: Record<string, unknown>): LocalItem {
  const failed = row.status === "error";
  return {
    id: row.id,
    stamp: failed && row.uncertain ? "Delivery not confirmed" : LOCAL_STAMP[row.status] ?? LOCAL_STAMP.sent,
    error: failed ? row.error : "",
    uncertain: failed && row.uncertain,
    // A delivery that may have happened is never retried from here: Check, then Allow retry in the composer.
    actions: failed && row.id ? (row.uncertain ? ["inspect", "delete"] : ["retry", "delete"]) : [],
    inspectLabel: summary?.native_ui_available === true ? "Check native UI" : "Check Terminal",
    available: row.attachments.map(localFileAvailable),
  };
}

/** A local row as a timeline event (Qt's `type: LocalMessage`). */
function localEvent(row: LocalMessage): ActivityEvent {
  return {
    type: "LocalMessage", detail: row.text, at: row.submitted_at, attachments: row.attachments as ActivityEvent["attachments"],
    source: row.source ?? undefined, voice: row.voice ?? undefined, message_id: row.message_id ?? undefined, local: row,
  };
}

/**
 * The events with the session's local rows merged in (ActivityView.cpp:665-720).
 * A voice row the journal already holds is left out. A sent row takes over its
 * own prompt event, which then shows its text and files: the one event with
 * exactly its text when it has files and no other sent row competes, else the
 * nearest prompt within 10 s that starts like it. Every other row is placed by
 * its time, after the events of the same time.
 */
export function mergeLocal(events: readonly ActivityEvent[], local: readonly LocalMessage[]): ActivityEvent[] {
  const source = events.slice();
  const appended: ActivityEvent[] = [];
  for (const row of local) {
    if (row.source === "hgs_voice") {
      if (!source.some((e) => e.source === "hgs_voice" && e.message_id === row.message_id)) appended.push(localEvent(row));
      continue;
    }
    const submitted = submittedOf(row);
    const sent = row.status === "sent";
    let match = -1;
    const exact: number[] = [];
    if (sent && row.attachments.length) {
      source.forEach((e, i) => {
        if (e.source !== "hgs_voice" && !e.agent_id && !e.attachments?.length && PROMPTS.has(e.type ?? "")
          && submitted && (e.detail ?? "").trim() === submitted) exact.push(i);
      });
      const competing = local.filter((other) => other.status === "sent" && other.attachments.length && submittedOf(other) === submitted).length;
      // Qt also remembers which of several exact events a row was bound to; with one it is that one.
      if (competing === 1 && exact.length === 1) match = exact[0];
    }
    if (sent && match < 0 && !exact.length) {
      let nearest = NEAREST;
      const head = submitted.slice(0, 1000);
      source.forEach((e, i) => {
        if (e.agent_id || !PROMPTS.has(e.type ?? "")) return;
        const body = (e.detail ?? "").trim();
        const distance = Math.abs((e.at ?? NaN) - row.submitted_at);
        if (e.source !== "hgs_voice" && !e.attachments?.length && distance <= nearest && submitted && body
          && (body.startsWith(head) || submitted.startsWith(body.slice(0, 1000)))) { match = i; nearest = distance; }
      });
    }
    if (match >= 0) {
      source[match] = {
        ...source[match], activity_key: `local-${row.id}`, detail: row.text,
        attachments: row.attachments as ActivityEvent["attachments"], attachment_submitted_text: row.submitted_text,
      };
      continue;
    }
    appended.push(localEvent(row));
  }
  if (!appended.length) return source;
  // Placed by time, after the events of the same time (Qt's stable sort by `at`).
  const placed = appended.map((e, i) => ({ e, i })).sort((a, b) => (a.e.at ?? 0) - (b.e.at ?? 0) || a.i - b.i).map((p) => p.e);
  const merged: ActivityEvent[] = [];
  let next = 0;
  for (const e of source) {
    const at = typeof e.at === "number" ? e.at : 0;
    while (next < placed.length && at > 0 && (placed[next].at ?? 0) < at) merged.push(placed[next++]);
    merged.push(e);
  }
  while (next < placed.length) merged.push(placed[next++]);
  return merged;
}

const PROVIDER_SOURCES = new Set(["codex_transcript", "claude_transcript", "kimi_wire"]);
const isExcerpt = (excerpt: string, full: string) => {
  const prefix = excerpt.endsWith("…") ? excerpt.slice(0, -1) : excerpt;
  return !!prefix && full.startsWith(prefix);
};

/**
 * Qt's provider_messages merge: watch event frames also arrive here, folded
 * into the summary by the core. Keep journal cursors intact and project the
 * bounded provider window at render time, including progress between tools.
 */
function mergeProvider(given: readonly ActivityEvent[], summary?: Record<string, unknown>): readonly ActivityEvent[] {
  if (!Array.isArray(summary?.provider_messages) || !summary.provider_messages.length) return given;
  const events = [...given];
  const matchedStops = new Set<number>();
  for (const value of summary.provider_messages.slice(-100)) {
    if (!value || typeof value !== "object") continue;
    const message = value as ActivityEvent;
    if (!PROVIDER_SOURCES.has(message.source ?? "") || message.agent_id
      || (message.type !== "AgentMessage" && !(message.type === "AgentThinking" && message.source === "claude_transcript"))) continue;
    const same = events.findIndex((e) => e.type === message.type && e.source === message.source && !e.agent_id
      && (message.message_id && e.message_id ? message.message_id === e.message_id
        : e.detail === message.detail && Math.abs((e.at ?? 0) - (message.at ?? 0)) < 3));
    if (same >= 0) continue;
    // The hook's final excerpt and the provider's full response are one row.
    // An empty Stop never absorbs commentary immediately before completion.
    const stop = message.type === "AgentMessage" ? events.findIndex((e, index) => !matchedStops.has(index) && e.type === "Stop" && !e.agent_id
      && Math.abs((e.at ?? 0) - (message.at ?? 0)) <= 2
      && (e.detail ? isExcerpt(e.detail, message.detail ?? "") : message.message_phase !== "commentary")) : -1;
    if (stop >= 0) {
      matchedStops.add(stop);
      events[stop] = { ...events[stop], detail: message.detail, source: message.source, message_id: message.message_id };
    }
    else events.push(message);
  }
  // Provider messages have no journal sequence. Stable occurrence-time order
  // puts mid-turn commentary between the surrounding tool events.
  return events.sort((a, b) => (a.at ?? 0) - (b.at ?? 0));
}

/**
 * The items of a transcript, oldest first; ids are stable while events only
 * append. `local` are the open session's local rows (the composer view's).
 */
export function buildTranscript(given: readonly ActivityEvent[], summary?: Record<string, unknown>, local: readonly LocalMessage[] = []): TranscriptItem[] {
  const provider = mergeProvider(given, summary);
  if (!provider.length && !local.length) return recordedContext(summary);
  const items: TranscriptItem[] = provider.length ? [] : recordedContext(summary);
  const events = local.length ? mergeLocal(provider, local) : provider;
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
    if (e.type === "LocalMessage" && role === "user") {
      // The stamp stands in for the time and the receipt (ActivityView.cpp:1053-1055).
      const row = e.local as LocalMessage;
      items.push({ id: `local-${row.id}`, kind: "user", at: "", body: row.text, attachments: attachments(e), local: localItem(row, summary) });
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
