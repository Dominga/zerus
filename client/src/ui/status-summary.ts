import { tr } from "../i18n";

// Task 72 (F1): the status bar's calm line. A computer whose stream has not
// answered yet ("probing", a cold start's first 7 to 10 s while `hgs watch`
// builds its first snapshot) is connecting, never offline. A real problem
// (reconnecting, ended, unsupported) is the status bar's error line instead.

export interface ComputerState { id: string; status: string }

export const connected = (status: string) => status === "live" || status === "polled";

/** Whether any computer has a real problem (anything but connected or still connecting). */
export const connectionError = (computers: readonly ComputerState[]) =>
  computers.some((c) => !connected(c.status) && c.status !== "probing");

/** The calm summary: connected of known, and the ones still connecting by name. */
export function statusSummary(computers: readonly ComputerState[], label: (id: string) => string): string {
  const live = computers.filter((c) => connected(c.status)).length;
  const connecting = computers.filter((c) => c.status === "probing").map((c) => label(c.id));
  if (live === 0 && connecting.length === computers.length) return tr("Connecting to %1…", connecting.join(", "));
  const base = tr("%1 of %2 machines connected", live, computers.length);
  return connecting.length ? base + tr(", %1 connecting", connecting.join(", ")) : base;
}

/**
 * Task 235: the status bar's note while rows from the last list on disk are
 * shown: "Last list from 14:05, updating…" with the oldest save time, or
 * nothing once every computer's live listing replaced them.
 */
export function staleNote(stale: Record<string, number>, format: (at: number) => string = clockTime): string {
  const saved = Object.values(stale).filter((at) => typeof at === "number" && Number.isFinite(at));
  if (saved.length === 0) return "";
  return tr("Last list from %1, updating…", format(Math.min(...saved)));
}

function clockTime(at: number): string {
  const date = new Date(at * 1000);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}
