import { setDetailTab, setSelectedKey } from "../../store/ui";
import type { Session } from "../sessions/types";

/** Opens a session row in the Activity inspector (Qt's open in Activity). */
export function openRow(row: Session | undefined) {
  if (!row) return;
  setSelectedKey(row.key);
  setDetailTab("activity");
}
