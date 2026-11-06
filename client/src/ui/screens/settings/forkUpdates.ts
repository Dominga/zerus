import type { ForkUpdatesView } from "../../../types";

// WhiteWh fork, task #63: what the fork channel's part of Settings > Updates
// offers, as Qt's Settings > About does: Check now, Replace this build with
// fork build N... (asked first) and Restart to install fork build N.

export interface ForkActions {
  /** Check now: a portable installation that is not busy or installing. */
  check: boolean;
  /** The offered replacement of this unnumbered build (0 for none). */
  replace: number;
  /** The verified build Restart installs (0 for none). */
  restart: number;
}

export function forkActions(view: ForkUpdatesView | null | undefined): ForkActions {
  if (!view || !view.supported) return { check: false, replace: 0, restart: 0 };
  const idle = !view.busy && !view.installing;
  return {
    check: idle,
    replace: idle && view.offered_build > 0 ? view.offered_build : 0,
    restart: !view.installing && view.ready_build > 0 ? view.ready_build : 0,
  };
}

/** The running build as the section names it: its fork build, else its commit. */
export function forkInstalled(view: ForkUpdatesView): { build: number; commit: string } {
  return { build: view.installed_build, commit: view.running_commit.slice(0, 10) };
}
