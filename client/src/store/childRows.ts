import type { Session } from "../ui/sessions/types";

// A new child session's row starts compact (zerus-sa9qn.14, owner option,
// default on). "New" means a parent link recorded after the desktop last
// looked: this device remembers, per computer whose clock stamped the links
// (the link's machine), the latest link time it handled and the children
// handled at that second. A computer seen for the first time only records its
// current children, so existing rows never change; a row the person expands
// afterwards keeps that choice, across restarts too.

export const COMPACT_NEW_CHILDREN = "sessions/compactNewChildren";
export const CHILD_ROWS_SEEN = "sessions/childRowsSeen";
const MACHINES = 256;
const SAME_SECOND = 64;

/** Per stamping computer: the latest link time handled and the children handled at that second. */
export type Seen = Record<string, [number, string[]]>;
type Child = Pick<Session, "key" | "sessionId" | "parent">;

/** The stored record, or undefined when missing or malformed (then nothing is compacted). */
export function parseSeen(text: unknown): Seen | undefined {
  if (typeof text !== "string" || !text) return undefined;
  try {
    const value: unknown = JSON.parse(text);
    if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
    const seen: Seen = {};
    for (const [machine, entry] of Object.entries(value).slice(0, MACHINES)) {
      if (!Array.isArray(entry) || !Number.isSafeInteger(entry[0]) || !Array.isArray(entry[1])) continue;
      seen[machine] = [entry[0] as number, (entry[1] as unknown[]).filter((id): id is string => typeof id === "string").slice(0, SAME_SECOND)];
    }
    return seen;
  } catch { return undefined; }
}

/** The rows to make compact now and the updated record (pure). */
export function newChildren(rows: readonly Child[], seen: Seen): { fresh: string[]; seen: Seen; changed: boolean } {
  const links = new Map<string, { at: number; id: string; key: string }[]>();
  for (const row of rows) {
    const at = row.parent?.at ?? 0;
    const machine = row.parent?.machine ?? "";
    if (!machine || !Number.isSafeInteger(at) || at <= 0) continue;
    let list = links.get(machine);
    if (!list) links.set(machine, list = []);
    list.push({ at, id: row.sessionId || row.key, key: row.key });
  }
  const next: Seen = { ...seen };
  const fresh: string[] = [];
  let changed = false;
  for (const [machine, list] of links) {
    let latest = 0;
    for (const link of list) latest = Math.max(latest, link.at);
    const known = seen[machine];
    const added = known ? list.filter((link) => link.at > known[0] || (link.at === known[0] && !known[1].includes(link.id))) : [];
    if (known && !added.length) continue;
    for (const link of added) fresh.push(link.key);
    const top = Math.max(known?.[0] ?? 0, latest);
    const atTop = new Set(known && known[0] === top ? known[1] : []);
    for (const link of list) if (link.at === top) atTop.add(link.id);
    next[machine] = [top, [...atTop].slice(-SAME_SECOND)];
    changed = true;
  }
  // Bounded: forget the computers with the oldest links first.
  const machines = Object.keys(next);
  if (machines.length > MACHINES) {
    for (const machine of machines.sort((a, b) => next[a][0] - next[b][0]).slice(0, machines.length - MACHINES)) delete next[machine];
  }
  return { fresh, seen: next, changed };
}
