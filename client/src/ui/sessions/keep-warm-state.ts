import { createSignal } from "solid-js";
import type { KeepWarmState } from "../../types";
import { inspectorClosed, openInspector, toggleInspectorCard } from "../../store/inspector";
import { keepWarmOf, mergeKeepWarm } from "./keep-warm";
import type { Session } from "./types";

// The one keep-warm state the session header (zerus-yacns) and Details >
// Session > Keep cache warm both show: the listing, the Details summary of the
// followed session and the change this window confirmed last. Nothing here
// reads from a computer.

const record = (value: unknown): Record<string, unknown> => (value && typeof value === "object" ? value as Record<string, unknown> : {});

/** A change belongs to the computer, row, run and conversation it was made for. */
export const keepWarmTarget = (s: Pick<Session, "computer" | "key" | "runId" | "conversationId">) => `${s.computer}\n${s.key}\n${s.runId ?? ""}\n${s.conversationId ?? ""}`;

// One slot: only the newest confirmed change is kept, for its target.
const [confirmed, setConfirmed] = createSignal<{ target: string; state: KeepWarmState }>();

/** Details confirmed this state for `target`. */
export const confirmKeepWarm = (target: string, state: KeepWarmState) => setConfirmed({ target, state });

/** The newest of the listing, the Details summary (`inspected`) and a change confirmed here, by revision. */
export function currentKeepWarm(s: Session, inspected: unknown): KeepWarmState | undefined {
  const mine = confirmed();
  return mergeKeepWarm(s.keepWarm, keepWarmOf(record(inspected).keep_warm), mine?.target === keepWarmTarget(s) ? mine.state : undefined);
}

/** The context the Details summary reports in use, for the tooltip's cost line. */
export const contextTokensOf = (inspected: unknown) => Number(record(record(record(inspected).session_usage).context).used) || 0;

const [revealed, setRevealed] = createSignal(0);
let handled = 0;

/** Opens Details with its Session card, where the Keep cache warm row scrolls into view and takes focus. */
export function revealKeepWarm() {
  openInspector("details");
  if (inspectorClosed().has("details-session")) toggleInspectorCard("details-session");
  setRevealed((n) => n + 1);
}

/** Whether a reveal is waiting for the Keep cache warm row (read in an effect); each is taken once. */
export function takeKeepWarmReveal(): boolean {
  const n = revealed();
  if (n <= handled) return false;
  handled = n;
  return true;
}
