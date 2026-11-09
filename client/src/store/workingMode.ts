import { createSignal } from "solid-js";
import type { TeamView } from "../types";

// Task #337: the working-mode block the project's profile suggests. hgs keeps
// the owner's keep (the block stays away until the profile changes); the close
// hides the block for this session of the profile, keyed by the profile's
// fingerprint, until the app restarts or the profile changes. Both are local
// until the team read brings the kept record back.
const [closed, setClosed] = createSignal<Record<string, string>>({});
const [kept, setKept] = createSignal<Record<string, string>>({});

/** The profile the block speaks for: hgs's fingerprint, else what the block says. */
export function workingModeFingerprint(team: TeamView): string {
  const mode = team.working_mode;
  return mode?.proposal?.fingerprint
    ?? `${mode?.proposal?.preset ?? mode?.preset ?? ""}\u001f${(mode?.disagreeing ?? []).join(",")}`;
}

/** A closed block never comes back for the same profile. */
export const workingModeClosed = (team: TeamView) => closed()[team.id] === workingModeFingerprint(team);

export function closeWorkingMode(team: TeamView) {
  setClosed({ ...closed(), [team.id]: workingModeFingerprint(team) });
}

/** The owner kept this profile: the block goes away until the profile changes. */
export const workingModeKeptHere = (team: TeamView) => kept()[team.id] === workingModeFingerprint(team)
  || (team.working_mode?.kept?.fingerprint ?? "") === workingModeFingerprint(team);

export function markWorkingModeKept(team: TeamView) {
  setKept({ ...kept(), [team.id]: workingModeFingerprint(team) });
}

/** The compact row shows only while the project's profile disagrees and the owner has neither kept nor closed it. */
export function workingModeShown(team: TeamView): boolean {
  return !!team.working_mode?.disagreeing?.length && !workingModeKeptHere(team) && !workingModeClosed(team);
}

/** Tests and the replay stay independent of a previous run's local state. */
export function resetWorkingMode() {
  setClosed({});
  setKept({});
}
