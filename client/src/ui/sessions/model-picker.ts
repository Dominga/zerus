import type { PickerRow, PickerView } from "./opencode-models";

// What ModelPicker.tsx keeps of Qt's model popover beside the list rules of
// opencode-models.ts (`ModelPicker`): its place under the model button
// (NewSessionDialog::openModelPicker 1266-1274), the search's pause
// (OpenCodeModelPicker.cpp 75-77), the star column's text (decorate) and the
// rows the keyboard moves over.

/** Search waits for a pause in typing: a catalog can hold 10,000 models. */
export const SEARCH_PAUSE_MS = 150;
/** Qt's search field takes at most this many characters. */
export const SEARCH_LIMIT = 200;
/** The popover is at least this wide and this tall. */
export const POPOVER_WIDTH = 440, POPOVER_HEIGHT = 420;
/** The gap under the button, and the margin kept to the window's edges. */
const GAP = 4, MARGIN = 8;

export interface Box { left: number; top: number; width: number; height: number }

/**
 * Where the popover opens: under the model button, as wide as the button and
 * at least 440 × 420. Qt places a popup where the button is; the webview
 * keeps it inside the window, above the button when there is no room below.
 */
export function popoverBox(button: { left: number; bottom: number; top: number; width: number }, window: { width: number; height: number }): Box {
  const width = Math.min(Math.max(POPOVER_WIDTH, button.width), Math.max(0, window.width - 2 * MARGIN));
  const height = Math.min(POPOVER_HEIGHT, Math.max(0, window.height - 2 * MARGIN));
  const left = Math.max(MARGIN, Math.min(button.left, window.width - MARGIN - width));
  let top = button.bottom + GAP;
  if (top + height > window.height - MARGIN) {
    const above = button.top - GAP - height;
    top = above >= MARGIN ? above : Math.max(MARGIN, window.height - MARGIN - height);
  }
  return { left, top, width, height };
}

/** The star column: "…" while its change is on its way, else filled for a favorite. */
export const starText = (row: Pick<PickerRow, "pending" | "favorite">) => (row.pending ? "…" : row.favorite ? "★" : "☆");

/** The model rows in display order, the keyboard's path through the list. */
export const pickerValues = (view: Pick<PickerView, "groups">) => view.groups.flatMap((group) => group.models.map((row) => row.value));

/**
 * The row an arrow or paging key moves the list's focus to: from `from` (the
 * focused row, else the current model, else none) by `by` rows, kept inside
 * the list. Undefined for an empty list.
 */
export function stepRow(view: Pick<PickerView, "groups">, from: string | undefined, by: number): string | undefined {
  const values = pickerValues(view);
  if (!values.length) return undefined;
  const at = from === undefined ? -1 : values.indexOf(from);
  if (at < 0) return by > 0 ? values[0] : values[values.length - 1];
  return values[Math.max(0, Math.min(values.length - 1, at + by))];
}

/** The focused row Enter chooses, as a click would: never an unavailable favorite (eventFilter). */
export function enterRow(view: Pick<PickerView, "groups">, focused: string | undefined): string | undefined {
  for (const group of view.groups) for (const row of group.models) if (row.value === focused) return row.unavailable ? undefined : row.value;
  return undefined;
}
