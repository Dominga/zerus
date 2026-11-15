import { invoke } from "@tauri-apps/api/core";
import { createSignal } from "solid-js";

export interface PhaseRecord {
  n: number; order?: number; title?: string; kind: "plan" | "queue";
  state: "draft" | "approved" | "awaiting_acceptance" | "done" | "rejected";
  mode?: "standard" | "free"; approval?: "review" | "auto"; acceptance?: "lead" | "person";
  barrier?: boolean; paused?: boolean;
}
export interface DraftDep { node: number; kind?: "hard" | "soft" }
export interface DraftNode {
  id: number; title: string; text: string; criteria: string[]; role: string; pipeline: string;
  deps: DraftDep[]; assignee: string | null; writes: string[]; estimate_minutes: number; kind: "task" | "check";
}
export interface DraftInput { title: string; barrier: boolean; nodes: DraftNode[] }
export interface DraftRoute { role: string; pipeline: string }
export interface PhaseList {
  team: string; revision: number; rows: PhaseRecord[]; next: string | null;
  next_node: number; next_phase: number; drafts_available: boolean; approval_available?: boolean;
  routes: DraftRoute[]; default_pipeline: string;
}
export interface PhaseDraft {
  metrics?: import("./phase-metrics").PhaseMetricsView;
  phase: PhaseRecord; revision: number; generation: number | null; next_node: number;
  nodes: DraftNode[]; complete: boolean; editable: boolean; unknown_fields: boolean;
  accept_available?: boolean;
  publication?: { id: string; generation: number; state: string; total: number; created: number; blocked: number; reason: string | null } | null;
  rejection?: { id: string; generation: number; comment: string } | null;
  previous?: { generation: number; draft: DraftInput } | null;
  live?: PhaseNodeSnapshot | null;
  graph: {
    complete: boolean;
    nodes: { id: number; phase: number; layer: number }[];
    edges: { from: number; to: number; kind: "hard" | "soft"; external: boolean }[];
  };
}
export interface PhaseLiveNode {
  id: number; phase: number; title: string; text?: string; task?: { project: string; uuid: string } | null;
  state?: string; hold?: boolean; admitted: boolean; kind?: "task" | "stage" | "check";
  role?: string; pipeline?: string; deps?: DraftDep[]; assignee?: string | null; implementer?: string | null;
  writes?: string[]; estimate_minutes?: number; rank?: string; round: number;
  integrated?: boolean; decided_by?: string | null; stage_of?: number | null; stage_index?: number;
}
export interface PhaseNodeSnapshot { phase: number; revision: number; nodes: PhaseLiveNode[]; complete: boolean }

let focusVersion = 0;
const [requestedPhaseFocus, setRequestedPhaseFocus] = createSignal<{ team: string; phase: number; version: number } | null>(null);
export const phaseFocus = requestedPhaseFocus;
export function openPhase(team: string, phase: number) { if (Number.isSafeInteger(phase) && phase > 0) setRequestedPhaseFocus({ team, phase, version: ++focusVersion }); }
export function takePhaseFocus(team: string): number | null {
  const focus = requestedPhaseFocus();
  if (focus?.team !== team) return null;
  setRequestedPhaseFocus(null); return focus.phase;
}

export interface PhasePane {
  epoch: number; list: PhaseList | null; draft: PhaseDraft | null; selected: number | null;
  list_failure: { code: string; message: string } | null;
  draft_failure: { code: string; message: string } | null;
}

export type PhaseFollow = (team: string, phase: number | null, enabled: boolean, expectedEpoch?: number | null) => Promise<number>;
export const nativePhaseFollow: PhaseFollow = (team, phase, enabled, expectedEpoch) => invoke<number>("teams_phases", { team, phase, enabled, expectedEpoch });

/** One running follow plus the latest desired one. Closing or selecting again
 * cannot reorder native calls, and no timer or second reader is started here. */
export function phaseFollower(team: string, port: PhaseFollow = nativePhaseFollow) {
  const [epoch, setEpoch] = createSignal<number | null>(null);
  const [error, setError] = createSignal("");
  let desired: { phase: number | null; enabled: boolean; version: number } | null = null;
  let running = false, version = 0, closed = false;
  let lastEpoch: number | null = null;
  async function drain() {
    if (running) return;
    running = true;
    try {
      while (desired) {
        const next = desired; desired = null;
        try {
          if (!next.enabled && lastEpoch === null) continue;
          const received = await port(team, next.phase, next.enabled, next.enabled ? null : lastEpoch);
          lastEpoch = received;
          if (!closed && next.version === version) { setEpoch(received); setError(""); }
        } catch (cause) {
          if (!closed && next.version === version) setError(String(cause));
        }
      }
    } finally { running = false; }
  }
  const request = (phase: number | null, enabled: boolean) => {
    desired = { phase, enabled, version: ++version };
    setEpoch(null); setError(""); void drain();
  };
  return {
    epoch, error,
    select(phase: number | null) { if (!closed) request(phase, true); },
    close() { if (!closed) { closed = true; request(null, false); } },
  };
}

/** Only unsaved IDs are compacted. Persisted IDs remain stable, including gaps. */
export function removeDraftNode(nodes: DraftNode[], id: number, nextNode: number): DraftNode[] {
  const kept = nodes.filter((n) => n.id !== id);
  const fresh = kept.filter((n) => n.id >= nextNode).map((n) => n.id).sort((a, b) => a - b);
  const ids = new Map(fresh.map((old, i) => [old, nextNode + i]));
  return kept.map((n) => ({ ...n, id: ids.get(n.id) ?? n.id,
    deps: n.deps.filter((d) => d.node !== id).map((d) => ({ ...d, node: ids.get(d.node) ?? d.node })),
  }));
}
