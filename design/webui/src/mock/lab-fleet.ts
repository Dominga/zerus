import { machines } from "./data";
import type { Machine, Platform } from "./types";

// Mockup only: 45 synthetic lab machines, so the Accounts "Add to machine"
// flow can be reviewed at the scale of a large fleet (about 50 machines).
// They appear only in that flow and in the account machine list.

const platformOf = (i: number): Platform => (i % 9 === 0 ? "windows" : i % 6 === 0 ? "macos" : "linux");

export const LAB_MACHINES: Machine[] = Array.from({ length: 45 }, (_, k) => {
  const i = k + 1;
  const connection = i % 13 === 0 ? "offline" : i % 17 === 0 ? "reconnecting" : "online";
  return {
    id: `m-lab-${i}`, alias: `lab-${String(i).padStart(2, "0")}`, label: `lab-${String(i).padStart(2, "0")}`, platform: platformOf(i),
    connection, thisComputer: false, colorIndex: i % 7, cpu: 10 + (i * 7) % 60, cores: 16, load: 1, memUsedGiB: 8, memTotalGiB: 32,
    hgsVersion: i % 8 === 0 ? "0.47.2" : "0.48.0-dev.912", lastSeen: connection === "offline" ? `${i} min ago` : "now", latencyMs: 20 + i,
  } satisfies Machine;
});

/** The demo fleet plus the lab machines. */
export const ALL_MACHINES: Machine[] = [...machines, ...LAB_MACHINES];
export const anyMachine = (alias: string) => ALL_MACHINES.find((m) => m.alias === alias);

/** Deterministic per-machine facts for the flow: the agent is missing, or a saved sign-in exists. */
export const labIndex = (alias: string) => Number(alias.match(/^lab-(\d+)$/)?.[1] ?? 0);
export const agentMissing = (alias: string) => labIndex(alias) % 7 === 3;
export const hasSavedSignIn = (alias: string) => labIndex(alias) % 5 === 1;
