import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { emptyUsage, type SessionUsageRef, type Usage } from "../types";

// The selected session's account snapshot. The screens worker owns bounded
// reads and the five-minute refresh; this store only follows its current view.
const [snapshot, setSnapshot] = createSignal<Usage>(emptyUsage());
let target: SessionUsageRef | null = null;
let provider = "";
let sequence = 0;
let listening: (() => void) | undefined;

async function pull(force: boolean): Promise<void> {
  const mine = target;
  const request = ++sequence;
  try {
    const next = await bridge.sessionUsage(mine, force);
    if (request === sequence && target === mine) setSnapshot(next);
  } catch {
    if (request === sequence && target === mine) {
      setSnapshot((previous) => ({ ...previous, refreshing: false, refresh_error: true }));
    }
  }
}

/** Follows a live session, or null while its strip is hidden. A new run gets no old data. */
export function followSessionUsage(next: SessionUsageRef | null, agent = ""): void {
  // Renders of the same binding keep its snapshot and any open limit popover.
  if (provider === agent && (target === next || (!!target && !!next && target.host === next.host && target.name === next.name && target.run_id === next.run_id))) return;
  target = next;
  provider = agent;
  setSnapshot(emptyUsage(provider));
  void pull(false);
}

/** Refresh usage shares a read already in flight; it never repeats one. */
export async function refreshSessionUsage(force = true): Promise<void> {
  await pull(force);
}

export const sessionUsage = (): Usage => snapshot();
export const sessionUsageTarget = (): SessionUsageRef | null => target;

/** Subscribes only while the session usage strip is mounted, so tests and headless
 * stores do not need an event bridge. */
export function startSessionUsage(): void {
  if (listening) return;
  listening = bridge.onSessionUsage(() => {
    if (target) void pull(false);
  });
}

export function stopSessionUsage(): void {
  listening?.();
  listening = undefined;
}
