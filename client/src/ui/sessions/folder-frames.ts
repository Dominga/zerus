import type { Row } from "./model";
import { folderFrameColor } from "./organization-ui";

/** One project's frame in the session list: where it starts and how tall it is. */
export interface FolderFrame {
  /** The project's header row id. */
  id: string;
  /** The frame's top in the list's row coordinates (already inset for padding). */
  top: number;
  height: number;
  /** The folder's own colour; the neutral border token when it has none. */
  frame: string;
}

/** The frame hugs the header band: 4 px from the row's top, 2 px left below it. */
export const FRAME_TOP = 4;
export const FRAME_INSET = 2;

/**
 * A frame per project header, around the header row alone (owner, 2026-11-09:
 * the colour belongs to the folder's header, not to its cards). Offsets come
 * from the fixed row heights the virtualizer places rows with, so a frame needs
 * no measurement; a folded project looks the same.
 */
export function headerFrames(rows: readonly Row[], heights: readonly number[]): FolderFrame[] {
  const out: FolderFrame[] = [];
  let top = 0;
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    if (row.type === "header") {
      out.push({ id: row.id, top: top + FRAME_TOP, height: Math.max(0, (heights[i] ?? 40) - FRAME_TOP - FRAME_INSET * 2), frame: folderFrameColor(row) });
    }
    top += heights[i] ?? 0;
  }
  return out;
}
