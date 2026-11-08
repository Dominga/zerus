import { createSignal } from "solid-js";
import { tr } from "../i18n";
import * as bridge from "../bridge";
import type { RulesCall, RulesOutcome } from "../bridge";
import type { ComputerId } from "../types";

// Qt parity (I5b): the composer's "N iron rules" chip
// (MessageComposer::setIronRulesCount). Qt's one IronRulesPanel tells the
// composer its count; here each session's count is kept by session key: the
// Iron rules card reports what it shows, and a session no card has reported
// yet gets its project's rules read once. Bounded to the newest 256 sessions.

export const COUNTS = 256;

export function createRuleCounts(read: (call: RulesCall) => Promise<RulesOutcome>) {
  const [counts, setCounts] = createSignal<ReadonlyMap<string, number>>(new Map());
  const asked = new Set<string>();
  const set = (key: string, count: number) => {
    if (!key) return;
    setCounts((all) => {
      if (all.get(key) === count) return all;
      const next = new Map(all);
      next.delete(key);
      next.set(key, count);
      for (const old of next.keys()) { if (next.size <= COUNTS) break; next.delete(old); }
      return next;
    });
  };
  return {
    /** The session's count; 0 (no chip) while it is not known. */
    count: (key: string) => counts().get(key) ?? 0,
    known: (key: string) => counts().has(key),
    /** What the Iron rules card shows for the session (IronRulesPanel's onCount). */
    set,
    /**
     * Reads the session's project rules once while no card has reported its
     * count (Qt's panel reads them whenever a session is selected). A failed
     * read leaves the count unknown, so no chip shows.
     */
    async ensure(key: string, computer: ComputerId, project: string) {
      if (!key || !project || counts().has(key) || asked.has(key)) return;
      asked.add(key);
      for (const old of asked) { if (asked.size <= COUNTS) break; asked.delete(old); }
      let outcome: RulesOutcome;
      try { outcome = await read({ computer, target: { scope: "project", project }, op: { op: "read" } }); } catch { return; }
      if (outcome.outcome === "done" && !counts().has(key)) set(key, outcome.view.rules.length);
    },
  };
}

export const ironRuleCounts = createRuleCounts((call) => bridge.ironRules(call));

/** The chip's text (Qt setIronRulesCount); "" hides it. */
export function ironRulesChip(count: number): string {
  return count <= 0 ? "" : count === 1 ? tr("1 iron rule") : tr("%1 iron rules", count);
}
