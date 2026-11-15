import type { ActivityEvent } from "../../types";

const SOURCES = new Set(["codex_transcript", "claude_transcript", "kimi_wire"]);
const PROMPTS = new Set(["UserPromptSubmit", "UserPromptQueued", "TurnStarted"]);

export const ownMessageText = (event: ActivityEvent): string => typeof event.message_text === "string" ? event.message_text : event.detail ?? "";
const oldPreview = (text: string) => text.length <= 2401 && text.endsWith("…") && Array.from(text).length === 1201;
export const ownMessageTruncated = (event: ActivityEvent): boolean => event.message_text_truncated === true
  || (event.message_text_truncated !== false && event.truncated === true)
  || (event.message_text === undefined && PROMPTS.has(event.type ?? "") && oldPreview(event.detail ?? ""));

function preview(text: string): string {
  let count = 0, end = 0;
  for (const character of text) {
    if (count++ === 1200) break;
    end += character.length;
  }
  return text.slice(0, end) + "…";
}
const text = (value: unknown) => typeof value === "string" ? value : "";
const turn = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const own = (event: ActivityEvent) => PROMPTS.has(event.type ?? "") && (!event.agent_id || event.agent_id === "main")
  && !event.origin && (!event.turn_origin || event.turn_origin === "user") && !event.inherited_from && event.source !== "hgs_voice";

/** Native IDs are namespaced by provider and conversation. A typed Kimi turn
 * cannot fall back to text or another ID when either side lacks its binding. */
function bound(event: ActivityEvent, body: ActivityEvent, summary?: Record<string, unknown>): boolean {
  if (event.kimi_turn_id !== undefined || body.kimi_turn_id !== undefined) {
    const cid = text(event.kimi_conversation_id);
    return body.source === "kimi_wire" && (!event.source || event.source === "kimi_wire")
      && turn(event.kimi_turn_id) && turn(body.kimi_turn_id) && event.kimi_turn_id === body.kimi_turn_id
      && !!cid && cid === text(body.kimi_conversation_id)
      && (!summary?.conversation_id || summary.conversation_id === cid);
  }
  const cid = text(event.conversation_id) || text(summary?.conversation_id);
  return !!event.source && event.source === body.source && !!text(event.message_id)
    && event.message_id === body.message_id && !!cid && cid === text(body.conversation_id)
    && (!summary?.conversation_id || summary.conversation_id === cid);
}

/** Display repair requires an existing exact binding. Missing legacy identity
 * remains explicitly partial, even when one nearby body shares its preview. */
export function restoreOwnMessages(given: readonly ActivityEvent[], summary?: Record<string, unknown>): readonly ActivityEvent[] {
  const providers = Array.isArray(summary?.provider_messages) ? summary.provider_messages as ActivityEvent[] : [];
  const historical = Array.isArray(summary?.user_message_bodies) ? summary.user_message_bodies as ActivityEvent[] : [];
  const seen = new Set<string>();
  const matches = new Map<number, { body: ActivityEvent; text: string }[]>();
  for (const body of [...historical, ...providers].slice(-200)) {
    if (body.type !== "UserMessageBody" || !SOURCES.has(body.source ?? "") || body.agent_id || body.origin) continue;
    const parts = Array.isArray(body.message_parts) ? body.message_parts : [body.detail];
    const identity = JSON.stringify([body.source, body.message_id, body.conversation_id, body.kimi_turn_id, body.kimi_conversation_id, parts, body.message_text_truncated]);
    if (seen.has(identity)) continue;
    seen.add(identity);
    // Complete and already attached rows also reserve their exact native body.
    const owners = given.flatMap((event, index) => own(event) && bound(event, body, summary) ? [index] : []);
    if (owners.length !== 1) continue;
    const index = owners[0], event = given[index];
    if (typeof event.message_text === "string" || !oldPreview(event.detail ?? "")) continue;
    const texts = [...new Set(parts.filter((part): part is string => typeof part === "string"))].filter(part => preview(part) === event.detail);
    if (texts.length !== 1) continue;
    const candidates = matches.get(index) ?? [];
    candidates.push({ body, text: texts[0] }); matches.set(index, candidates);
  }
  return given.map((event, index) => {
    const candidates = matches.get(index);
    if (candidates?.length !== 1) return event;
    return { ...event, message_text: candidates[0].text, message_text_truncated: candidates[0].body.message_text_truncated === true };
  });
}
