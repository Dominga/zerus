import { createEffect, onCleanup, untrack } from "solid-js";
import type { ComputerId } from "../types";
import type { NewTeamView } from "../ui/teams/types";

// Task 42: the open draft is read while New team shows it, and each read the
// lead's `hgs team draft` edits arrive in reaches the form (the owner's unsaved
// fields stay theirs: store/teamsNew.ts mergeDraft). Call inside a component
// or root: the follow ends with it.

export interface DraftFollowDeps {
  computer: () => ComputerId;
  draft: () => string;
  /** store/teams.ts showTeam: reads that team while needed. */
  show: (computer: ComputerId, team: string) => () => void;
  /** store/teamsView.ts newTeam: the draft's latest view. */
  view: (computer: ComputerId, team: string) => NewTeamView | undefined;
  apply: (view: NewTeamView) => void;
}

export function followDraft(deps: DraftFollowDeps) {
  createEffect(() => {
    const team = deps.draft();
    if (!team) return;
    const computer = deps.computer();
    onCleanup(untrack(() => deps.show(computer, team)));
  });
  createEffect(() => {
    const team = deps.draft();
    const view = team ? deps.view(deps.computer(), team) : undefined;
    // Only a new view reruns this: applying it reads and writes the form's own state.
    if (view) untrack(() => deps.apply(view));
  });
}
