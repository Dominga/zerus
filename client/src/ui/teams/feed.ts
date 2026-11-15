// Adapted from WhiteWh's history.ts helpers. Our journal has its own text and
// actor, not the fork engine's `detail` payload; never infer missing events.
import { tr } from "../../i18n";
import type { DrawerRow } from "../../store/teamDrawers";

export function feedClock(atMs: number): string {
  if (!atMs) return "";
  const date = new Date(atMs);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export function inboxKind(kind: string): string {
  switch (kind.split(":")[0]) {
    case "report": return tr("Report");
    case "question": return tr("Question");
    case "permission": return tr("Permission");
    default: return kind;
  }
}

export const reportRows = (rows: readonly DrawerRow[]) => rows.filter((row) => row.kind === "report");
export const historyLine = (row: DrawerRow) => row.text || row.kind;
