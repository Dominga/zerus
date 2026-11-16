import { tr, trNoop } from "../i18n";
import type { TeamSession } from "./teams";

export type PhaseControlScope =
  | { kind: "draft"; phase: number; generation: number }
  | { kind: "published"; phase: number; generation: number; publication: string;
      rework: { id: string; generation: number | null; approval: string | null } | null };
export type PauseOwner = { kind: "person"; person: string }
  | { kind: "lead"; person: string; member: string; session: TeamSession };
export interface PauseEpisode { id: string; owner: PauseOwner; reason: string; created_ms: number }
export interface PhaseAvailability { allowed: boolean; reason: string | null }
export interface PhaseMove { target: PhaseControlScope | null; availability: PhaseAvailability }
/** Core validates this read before it reaches the window. Flags are facts,
 * never native Person permission or a Session binding. */
export interface PhaseControls {
  version: 1; scope: PhaseControlScope;
  pause: { state: "running" } | { state: "paused"; episode: PauseEpisode | null };
  unstarted: PhaseAvailability; barrier: PhaseAvailability; pause_change: PhaseAvailability;
  resume: PhaseAvailability; acceptance: PhaseAvailability; earlier: PhaseMove; later: PhaseMove;
}

// Closed product/Core reasons only. Unknown backend prose stays verbatim.
const REASONS: readonly string[] = [
  trNoop("A current task document has execution history, a reservation or an unavailable unstarted identity."),
  trNoop("A draft node already has a runtime identity."),
  trNoop("A published phase is not in its unstarted approved state."),
  trNoop("A retained phase task or pipeline stage has already started or ended."),
  trNoop("A retained task has prior attempts, delivery, stage, decision or integration history, or lacks a current unstarted observation."),
  trNoop("An open correction retains this phase's acceptance and order."),
  trNoop("Only a nonterminal planned phase of an active team can change controls."),
  trNoop("Only an unstarted draft, rejected or approved phase can change order or acceptance."),
  trNoop("The complete phase membership is unavailable."),
  trNoop("The current phase task documents are unavailable. Read again."),
  trNoop("The current task document no longer matches its retained unstarted observation."),
  trNoop("The draft's current identity is unavailable."),
  trNoop("The exact phase task copy changed under custody."),
  trNoop("The exact phase task custody is incomplete."),
  trNoop("The exact phase task custody is unavailable."),
  trNoop("The phase has no complete nonempty draft."),
  trNoop("The phase has no complete publication scope."),
  trNoop("The phase is not paused."),
  trNoop("The phase state conflicts with its publication."),
  trNoop("The phase task copy changed while custody was acquired."),
  trNoop("The phase task custody exceeds its node bound."),
  trNoop("The phase task custody read exceeded its time bound."),
  trNoop("The phase task scope changed before custody was acquired."),
  trNoop("The phase task scope exceeds the control proof bounds."),
  trNoop("The phase task store is busy; its current facts are unavailable."),
  trNoop("The retained correction controls conflict with the phase."),
  trNoop("The selected phase has no complete current draft scope."),
  trNoop("The selected phase has no complete frozen publication scope."),
  trNoop("The whole live and archived publication scope is incomplete or unsettled."),
  trNoop("There is no neighboring planned phase in that direction."),
  trNoop("This draft already has runtime task history."),
];
export function phaseControlReason(reason: string): string {
  const known = REASONS.find((source) => source === reason);
  return known ? tr(known) : reason;
}
