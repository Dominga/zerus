import { invoke } from "@tauri-apps/api/core";
import { createSignal } from "solid-js";
export interface ObservedUnitKey {
  team: string; home: string; node: number; task: { project: string; uuid: string };
  kind: "implementation" | "check" | "stage"; phase: number;
  publication: { id: string; draft_generation: number } | null;
  node_generation: number; attempt: { id: string; generation: number; fingerprint: string } | null;
  effect: string | null; binding: { kind: string; member: string; session: { machine: string; session_id: string }; run_id: string | null; conversation_id: string | null; native_incarnation: null } | null;
}
export interface ObservedFact {
  schema: number; seq: number; unit: number; key: ObservedUnitKey;
  snapshot: { title: string; role: string | null; configured_model: string | null; native_model: null };
  source: { kind: "team_journal" | "task_document"; identity: string; team_revision: number; event_id: string | null; event_sequence: number | null; observer_epoch: string | null };
  category: string; event_at_ms: number | null; observed_at_ms: number; last_verified_ms: number | null;
  break_before: boolean; reasons: string[]; implementation_observed_ms: null;
}
export interface ObservedPage {
  schema: number; scope: { team: string; home: string; phase: number; node: number | null; publication: { id: string; draft_generation: number } | null; window: { from_ms: number | null; to_ms: number | null; window_ms: number } };
  cohort: { units: { id: number; key: ObservedUnitKey }[]; more: boolean };
  rows: ObservedFact[]; source_revision: number; fact_ceiling: number;
  as_of_ms: number | null; last_verified_ms: number | null;
  next: string | null; more: boolean; truncated: boolean; reasons: string[];
}
export interface UnitCoverage { key: ObservedUnitKey; coverage: "sampled" | "unavailable"; reasons: string[]; implementation_observed_ms: null }
export interface ObservedPane { epoch: number; page: ObservedPage; coverage: UnitCoverage[]; after: string | null }
export type ObservedFollow = (team: string, phase: number, enabled: boolean, expectedEpoch: number | null) => Promise<number>;
const nativeFollow: ObservedFollow = (team, phase, enabled, expectedEpoch) => invoke("teams_observed", { team, phase, node: null, windowMs: 86_400_000, enabled, expectedEpoch });
/** Serializes scope/cleanup commands, never creates a reader or retries a write. */
export function observedFollower(team: string, port: ObservedFollow = nativeFollow) {
  const [epoch, setEpoch] = createSignal<number | null>(null), [error, setError] = createSignal("");
  let lease: number | null = null, phase = 0, closed = false, running = false, version = 0;
  let desired: { phase: number; enabled: boolean; version: number } | null = null;
  async function drain() {
    if (running) return; running = true;
    try { while (desired) {
      const next = desired; desired = null;
      if (!next.enabled && lease === null) continue;
      try {
        const received = await port(team, next.phase, next.enabled, next.enabled ? null : lease);
        lease = received;
        if (!closed && next.version === version) { setEpoch(received); setError(""); }
      } catch (cause) { if (!closed && next.version === version) setError(String(cause)); }
    }} finally { running = false; }
  }
  function request(enabled: boolean) { desired = { phase, enabled, version: ++version }; setEpoch(null); setError(""); void drain(); }
  return { epoch, error, select(selected: number, enabled: boolean) { if (!closed) { phase = selected; request(enabled); } }, close() { if (!closed) { closed = true; request(false); } } };
}
