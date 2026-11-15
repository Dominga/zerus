import { tr } from "../../i18n";
import type { PhaseRecord } from "../../store/team-phases";

// Adapted from WhiteWh's phaseHeader: our engine's states and flags are the
// source. In particular no display state is inferred from phase number - 1.
export function phaseHeader(phase: PhaseRecord) {
  const states = {
    draft: tr("Draft"), approved: tr("Approved"), awaiting_acceptance: tr("Awaiting acceptance"),
    done: tr("Done"), rejected: tr("Rejected"),
  };
  return {
    title: phase.kind === "queue" && phase.title === "Queue" ? tr("Queue") : phase.title || (phase.kind === "queue" ? tr("Queue") : tr("Phase %1", phase.n)),
    state: states[phase.state] ?? tr("Unknown phase state"),
    mode: phase.mode === "free" ? tr("Free") : tr("Standard"),
    barrier: phase.barrier ? tr("Waits for earlier phases") : "",
    paused: phase.paused ? tr("Paused") : "",
    approvalUnavailable: tr("Phase approval is not available yet."),
  };
}
