// Keep cache warm (Qt's CacheKeepalive, WhiteWh fork): the hours each session
// keeps its prompt cache warm, a device-local choice under Qt's own key. The
// value is Qt's compact JSON object of session identity (the row's exact
// Qt-compatible key, calculated in Rust) -> hours; Rust sends the keepalives.

export const KEEP_WARM_KEY = "workspace/cacheKeepWarm";
/** On from the session menu is one hour. */
export const DEFAULT_HOURS = 1;
/** Details' slider offers 1 to 8 hours; storage keeps up to 24. */
export const SLIDER_MAX_HOURS = 8;
export const MAX_HOURS = 24;
/** The service message's envelope, as hgs recognizes it. */
export const KEEPALIVE_ENVELOPE = "<zerus-cache-keepalive>";

/** QJsonValue::toInt(): a whole number within 32 bits, otherwise 0. */
function qtInt(value: unknown): number {
  return typeof value === "number" && Number.isInteger(value) && value >= -2147483648 && value <= 2147483647 ? value : 0;
}

/** The stored object's hours by identity; empty when it is missing or not a JSON object. */
export function parseHours(raw: unknown): ReadonlyMap<string, number> {
  const hours = new Map<string, number>();
  if (typeof raw !== "string" || !raw.trim()) return hours;
  let value: unknown;
  try { value = JSON.parse(raw); } catch { return hours; }
  if (!value || typeof value !== "object" || Array.isArray(value)) return hours;
  for (const [id, stored] of Object.entries(value as Record<string, unknown>)) {
    const n = Math.min(MAX_HOURS, Math.max(0, qtInt(stored)));
    if (n > 0) hours.set(id, n);
  }
  return hours;
}

/** The session's hours, 0 when off: read by its exact identity only, as Qt does. */
export function hoursOf(saved: ReadonlyMap<string, number>, keys: readonly [string, string] | undefined): number {
  return keys && /^[0-9a-f]{64}$/u.test(keys[0]) ? saved.get(keys[0]) ?? 0 : 0;
}

/** Qt's menu and Details rule: a session with a conversation, never an archived one. */
export function warmable(s: { conversationId?: string; state: string; compactKeys?: readonly [string, string] }): boolean {
  return !!s.conversationId && s.state !== "archived" && !!s.compactKeys;
}

/** The session menu's toggle: off, or one hour. */
export const toggledHours = (current: number) => (current > 0 ? 0 : DEFAULT_HOURS);

/** What the slider shows: the saved hours within its range, else one hour. */
export const sliderHours = (hours: number) => (hours > 0 ? Math.min(SLIDER_MAX_HOURS, Math.max(1, Math.round(hours))) : DEFAULT_HOURS);

/** A journal event of the keepalive: hgs's quiet notice, or the message itself in a native history. */
export function isKeepalive(event: { origin?: string; detail?: string }, role: string): boolean {
  return role === "notice" ? event.origin === "cache_keepalive" : role === "user" && (event.detail ?? "").startsWith(KEEPALIVE_ENVELOPE);
}
