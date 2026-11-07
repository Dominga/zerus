import * as bridge from "../bridge";
import { computerOffline } from "./catalog";
import { stillListed } from "./actions";
import type { MenuEntry } from "./ui";
import type { Details } from "../types";
import type { Session } from "../ui/sessions/types";
import { snapshotOf, terminal } from "../ui/sessions/actions-ui";
import { toast } from "./notices";
import { tr } from "../i18n";

/** External connection, separate from the embedded Terminal and Native UI. */
export function sessionTerminalEntry(s: Session, details?: Details): MenuEntry {
  const row = snapshotOf(s);
  const reason = computerOffline(s.computer) ? tr("Machine unavailable. Reconnect when it is online.")
    : s.state !== "running" ? tr("Resume this session before opening its terminal.")
    : !s.runId || !s.conversationId ? tr("Refresh this session's identity before opening its terminal.")
    : !terminal(s, details) ? (s.cmd === "dsh" || s.cmd === "deepseek" ? tr("This native adapter does not provide Terminal.") : tr("Terminal is unavailable for this session.")) : undefined;
  return { label: tr("Open terminal"), icon: "square-terminal", disabled: !!reason, hint: reason,
    action: () => {
      const live = stillListed(row);
      if (reason || !live || live.runId !== row.runId || live.name !== row.name || computerOffline(s.computer)) return;
      void bridge.sessionTerminalOpen({ computer: row.computer, name: row.name, run_id: row.runId!, conversation_id: row.conversationId! }).then((advisory) => {
        if (advisory) toast("Zerus: session opened, but not the way you asked", advisory, "warn");
      }, (error) => toast(tr("Could not open session terminal"), String(error), "error", true));
    } };
}
