import { createEffect, createMemo, createRoot, createSignal, onCleanup } from "solid-js";
import { catalog } from "./catalog";
import { desktopService } from "./desktopService";
import { anchorRetry, type RetryState } from "./retry";

export { anchorRetry, secondsLeft, type RetryState } from "./retry";

// One computer's connection state for Machines, from the supervisor that owns
// it: the background service's (`fleet_computers`) when the desktop is
// connected to it, otherwise this app's own (`catalog.computers`). The time
// to the next automatic attempt comes only from that supervisor's own
// deadline, anchored on this view's monotonic clock once per wait.

export interface MachineConnection {
  id: string;
  status: string;
  attempt: number;
  /** The service source a reconnect binds to; undefined for this app's own. */
  incarnation?: number;
  retry?: RetryState;
  /** Whose supervisor this is. */
  via: "service" | "app";
}

/** Service schedules anchored so far, by computer; bounded by its roster. */
const anchored = new Map<string, RetryState>();

const connectionMap = createRoot(() => createMemo((): Map<string, MachineConnection> => {
  const service = desktopService.state();
  const out = new Map<string, MachineConnection>();
  if (service?.connected && service.fleet_computers) {
    const now = performance.now();
    for (const row of service.fleet_computers) {
      const retry = anchorRetry(anchored.get(row.id), row.retry, now, String(row.incarnation ?? ""));
      if (retry) anchored.set(row.id, retry); else anchored.delete(row.id);
      out.set(row.id, { id: row.id, status: row.status, attempt: row.attempt, incarnation: row.incarnation ?? undefined, retry, via: "service" });
    }
    for (const id of [...anchored.keys()]) if (!out.has(id)) anchored.delete(id);
    return out;
  }
  for (const c of Object.values(catalog.computers)) out.set(c.id, { id: c.id, status: c.status, attempt: c.attempt, retry: c.retry, via: "app" });
  return out;
}));

/** The connection state of `computer` ("" is this computer), if known. */
export const machineConnection = (computer: string): MachineConnection | undefined => connectionMap().get(computer);

// One shared clock for every visible countdown; it runs only while one is shown.
const [now, setNow] = createSignal(performance.now());
let users = 0;
let timer: ReturnType<typeof setInterval> | undefined;
const TICK_MS = 250;

/** `performance.now()`, updated while `active()` holds for some caller. */
export function useCountdownClock(active: () => boolean): () => number {
  createEffect(() => {
    if (!active()) return;
    users += 1;
    if (!timer) { setNow(performance.now()); timer = setInterval(() => setNow(performance.now()), TICK_MS); }
    onCleanup(() => {
      users -= 1;
      if (!users && timer) { clearInterval(timer); timer = undefined; }
    });
  });
  return now;
}
