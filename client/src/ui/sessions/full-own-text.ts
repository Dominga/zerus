import type { ActivityEvent } from "../../types";

const SOURCES = new Set(["codex_transcript", "claude_transcript", "kimi_wire"]);
const PROMPTS = new Set(["UserPromptSubmit", "UserPromptQueued", "TurnStarted"]);

export const ownMessageText = (event: ActivityEvent): string => typeof event.message_text === "string" ? event.message_text : event.detail ?? "";
const oldPreview = (text: string) => text.length <= 2401 && text.endsWith("…") && Array.from(text).length === 1201;
// Scan only the preview prefix, even for a large receipt body.
function hookPreview(text: string): string {
  let count = 0, end = 0;
  for (const character of text) {
    if (count++ === 1200) break;
    end += character.length;
  }
  return text.slice(0, end) + "…";
}
export const matchesOwnPreview = (preview: string, full: string): boolean => oldPreview(preview)
  && hookPreview(full) === preview;
export const ownMessageTruncated = (event: ActivityEvent): boolean => event.message_text_truncated === true
  || (event.message_text === undefined && PROMPTS.has(event.type ?? "") && oldPreview(event.detail ?? ""));

/** Display repair only. A native body never confirms delivery or inserts a
 * second user row. Ambiguous repeated previews remain explicitly incomplete. */
export function restoreOwnMessages(given: readonly ActivityEvent[], summary?: Record<string, unknown>): readonly ActivityEvent[] {
  const providers = Array.isArray(summary?.provider_messages) ? summary.provider_messages as ActivityEvent[] : [];
  const historical = Array.isArray(summary?.user_message_bodies) ? summary.user_message_bodies as ActivityEvent[] : [];
  const bodies = [...historical, ...providers].filter(event => event.type === "UserMessageBody"
    && SOURCES.has(event.source ?? "") && !event.agent_id && !event.origin);
  const byPreview = new Map<string, { body: ActivityEvent; text: string }[]>();
  const seen = new Set<string>();
  for (const body of bodies.slice(-100)) {
    const parts = Array.isArray(body.message_parts) ? body.message_parts : [body.detail];
    const identity = JSON.stringify([body.source, body.message_id, body.at, parts, body.message_text_truncated]);
    if (seen.has(identity)) continue;
    seen.add(identity);
    for (const text of new Set(parts.filter((part): part is string => typeof part === "string"))) {
      const preview = hookPreview(text);
      const candidates = byPreview.get(preview) ?? [];
      candidates.push({ body, text }); byPreview.set(preview, candidates);
    }
  }
  const matches = new Map<number, { body: ActivityEvent; text: string }[]>();
  const counts = new Map<ActivityEvent, number>();
  given.forEach((event, index) => {
    if (!PROMPTS.has(event.type ?? "") || event.agent_id && event.agent_id !== "main" || event.origin || event.turn_origin && event.turn_origin !== "user" || event.inherited_from
      || typeof event.message_text === "string" || !oldPreview(event.detail ?? "")) return;
    const when = event.at;
    if (typeof when !== "number" || !Number.isFinite(when) || when <= 0) return;
    for (const candidate of byPreview.get(event.detail ?? "") ?? []) {
      const { body } = candidate;
      if (typeof body.at !== "number" || !Number.isFinite(body.at) || body.at <= 0 || Math.abs(body.at - when) > 10) continue;
      const candidates = matches.get(index) ?? [];
      candidates.push(candidate); matches.set(index, candidates);
      counts.set(body, (counts.get(body) ?? 0) + 1);
    }
  });
  if (!matches.size) return given;
  return given.map((event, index) => {
    const candidates = matches.get(index);
    if (candidates?.length !== 1 || counts.get(candidates[0].body) !== 1) return event;
    const { body, text } = candidates[0];
    return { ...event, message_text: text, message_text_truncated: body.message_text_truncated === true };
  });
}
