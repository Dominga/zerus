// Task 242: the window's navigation commands as Qt's SessionsWindowShortcuts
// steps them (stepSession, stepTab, stepFocus). Pure: the callers pass the
// list as it shows, the tabs and the areas.

/** One session in list order; `hidden` when its project is collapsed. */
export interface Listed { key: string; hidden: boolean }

/**
 * The session list's sessions in list order (Qt's m_sessions rows): every
 * card as shown, and the sessions of a collapsed project at its header, hidden.
 */
export function listedOrder(
  rows: readonly ({ type: "header"; project: string; collapsed: boolean } | { type: "card"; session: { key: string } } | { type: string })[],
  members: ReadonlyMap<string, readonly { key: string }[]>,
  listed: (key: string) => boolean,
): Listed[] {
  const out: Listed[] = [];
  for (const row of rows) {
    if (row.type === "card") out.push({ key: (row as { session: { key: string } }).session.key, hidden: false });
    else if (row.type === "header" && (row as { collapsed: boolean }).collapsed) {
      for (const session of members.get((row as { project: string }).project) ?? []) if (listed(session.key)) out.push({ key: session.key, hidden: true });
    }
  }
  return out;
}

/**
 * Next and Previous session (Qt stepSession, not attention): the next shown
 * session in the list, stopping at its ends; with none selected, the first
 * (or the last going back). Undefined at an end.
 */
export function stepSession(order: readonly Listed[], current: string | undefined, direction: 1 | -1): string | undefined {
  const shown = order.filter((entry) => !entry.hidden);
  let at = shown.findIndex((entry) => entry.key === current);
  if (at < 0) at = direction > 0 ? -1 : shown.length;
  return shown[at + direction]?.key;
}

/**
 * Next and Previous session that needs attention (Qt stepSession, attention):
 * every listed session, those in collapsed projects too, in list order,
 * wrapping around; undefined when none needs it.
 */
export function stepAttention(order: readonly Listed[], current: string | undefined, direction: 1 | -1, needs: (key: string) => boolean): Listed | undefined {
  const count = order.length;
  if (!count) return undefined;
  let start = order.findIndex((entry) => entry.key === current);
  if (start < 0) start = direction > 0 ? -1 : count;
  for (let step = 1; step <= count; step++) {
    const entry = order[(((start + direction * step) % count) + count) % count];
    if (needs(entry.key)) return entry;
  }
  return undefined;
}

/** Next and Previous tab (Qt stepTab): the next shown tab, wrapping; undefined with no other. */
export function stepTab<T extends string>(tabs: readonly { id: T; show: boolean }[], current: T, direction: 1 | -1): T | undefined {
  const count = tabs.length;
  const at = Math.max(0, tabs.findIndex((tab) => tab.id === current));
  for (let step = 1; step < count; step++) {
    const tab = tabs[(((at + direction * step) % count) + count) % count];
    if (tab.show) return tab.id;
  }
  return undefined;
}

/**
 * F6 and Shift+F6 (Qt stepFocus): the next area after the innermost one that
 * holds focus (`current`, -1 for none), wrapping; with none, the first (or the
 * last going back).
 */
export function stepArea(count: number, current: number, direction: 1 | -1): number {
  if (count <= 0) return -1;
  if (current < 0) return direction > 0 ? 0 : count - 1;
  return (((current + direction) % count) + count) % count;
}
