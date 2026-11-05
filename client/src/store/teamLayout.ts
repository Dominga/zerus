import { createSignal } from "solid-js";
import { num, setSetting } from "./settings";

// The Team tab's two dividers, which only the owner moves (core design,
// owner decisions 2026-10-24: "split by a divider the owner moves (both
// halves scroll)"; the session list never collapses by itself). Qt keeps them
// as QSplitter states (teams/workspaceSplit, teams/workspaceSideSplit); the
// client keeps plain numbers in its settings store:
// - client/teamSideWidth: the side column's width in px, beside the canvas;
// - client/teamMembersShare: the members' share of the side column, in
//   percent, above the coordinator's conversation.
// A drag shows the value live and stores it once, when the pointer lets go;
// nothing else (a narrow window, a hidden tab) changes the stored choice.

export const SIDE = { key: "client/teamSideWidth", min: 340, max: 900, initial: 420, step: 20 } as const;
export const SHARE = { key: "client/teamMembersShare", min: 15, max: 85, initial: 45, step: 5 } as const;

type Bounds = { readonly min: number; readonly max: number; readonly initial: number };

export const clampTo = (bounds: Bounds, value: number) =>
  Number.isFinite(value) ? Math.max(bounds.min, Math.min(bounds.max, Math.round(value))) : bounds.initial;

/** The share a drag of the members/coordinator divider gives: `start` plus the pointer's move over the column's height. */
export function dragShare(start: number, startY: number, y: number, height: number): number {
  if (!(height > 0)) return clampTo(SHARE, start);
  return clampTo(SHARE, start + ((y - startY) / height) * 100);
}

/** The width a drag of the canvas/side divider gives: the side column grows as the pointer moves left. */
export function dragSide(start: number, startX: number, x: number, available: number): number {
  const width = clampTo(SIDE, start + (startX - x));
  // The canvas keeps at least a third of the tab; the side never drops below its minimum.
  return available > 0 ? Math.max(SIDE.min, Math.min(width, Math.round(available * 2 / 3))) : width;
}

export interface LayoutStore {
  read(key: string): number;
  write(key: string, value: number): void;
}

export function createTeamLayout(store: LayoutStore) {
  const [sideDrag, setSideDrag] = createSignal<number>();
  const [shareDrag, setShareDrag] = createSignal<number>();
  const storedSide = () => clampTo(SIDE, store.read(SIDE.key));
  const storedShare = () => clampTo(SHARE, store.read(SHARE.key));
  /** Stores a value once, and only when it differs from the stored one. */
  const commit = (bounds: Bounds & { key: string }, value: number | undefined, stored: number) => {
    if (value === undefined) return;
    const next = clampTo(bounds, value);
    if (next !== stored) store.write(bounds.key, next);
  };
  return {
    sideWidth: () => sideDrag() ?? storedSide(),
    membersShare: () => shareDrag() ?? storedShare(),
    previewSide: (width: number | undefined) => setSideDrag(width === undefined ? undefined : clampTo(SIDE, width)),
    previewShare: (share: number | undefined) => setShareDrag(share === undefined ? undefined : clampTo(SHARE, share)),
    /** The pointer let go: the dragged width is the owner's choice. */
    commitSide: () => { const value = sideDrag(); setSideDrag(undefined); commit(SIDE, value, storedSide()); },
    commitShare: () => { const value = shareDrag(); setShareDrag(undefined); commit(SHARE, value, storedShare()); },
    /** A drag cut off (capture lost, the tab closed) keeps the stored value. */
    cancel: () => { setSideDrag(undefined); setShareDrag(undefined); },
    /** A double click: the default again. */
    resetSide: () => { setSideDrag(undefined); commit(SIDE, SIDE.initial, storedSide()); },
    resetShare: () => { setShareDrag(undefined); commit(SHARE, SHARE.initial, storedShare()); },
    /** Arrow keys on a divider: one step, stored at once. */
    stepSide: (direction: number) => commit(SIDE, storedSide() + direction * SIDE.step, storedSide()),
    stepShare: (direction: number) => commit(SHARE, storedShare() + direction * SHARE.step, storedShare()),
  };
}

export type TeamLayout = ReturnType<typeof createTeamLayout>;

/** The window's dividers, kept in the client's settings store. */
export const teamLayout = createTeamLayout({ read: num, write: (key, value) => { void setSetting(key, value); } });
