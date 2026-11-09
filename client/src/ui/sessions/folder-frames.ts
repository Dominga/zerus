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

/** The frame's inner padding: 2 px above and below the folder's header and cards. */
export const FRAME_INSET = 2;

/**
 * A frame per project header, wrapping the header and every row of the project
 * in list order (a folded project is its header alone). Offsets come from the
 * fixed row heights the virtualizer places rows with, so a frame needs no
 * measurement; the rows keep their own 2 px inset, which is the folder's
 * normal inner padding.
 */
export function groupFrames(rows: readonly Row[], heights: readonly number[]): FolderFrame[] {
  const out: FolderFrame[] = [];
  const offsets: number[] = [];
  let total = 0;
  for (const h of heights) {
    offsets.push(total);
    total += h;
  }
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    if (row.type !== "header") continue;
    let end = i + 1;
    while (end < rows.length && rows[end].type !== "header") end++;
    const start = offsets[i] ?? 0;
    const bottom = end < rows.length ? offsets[end] ?? total : total;
    out.push({ id: row.id, top: start + FRAME_INSET, height: Math.max(0, bottom - start - FRAME_INSET * 2), frame: folderFrameColor(row) });
  }
  return out;
}
