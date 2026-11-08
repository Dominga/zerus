import { batch } from "solid-js";
import { tr } from "../../i18n";
import { computerOffline } from "../../store/catalog";
import { flag } from "../../store/settings";
import { notify } from "../../store/actions";
import { setDetailTab, setPage, setSelectedKey, setSubagentOpen } from "../../store/ui";
import { terminal as hasTerminal } from "./actions-ui";
import { nativeRoute } from "./native-entry";
import type { Session } from "./types";

// Where a click on a card goes (SessionsWindow.cpp:1766-1778 chipActivated, and the
// row menu's Open terminal): the card is selected first, as a click on the card is.

const select = (s: Session) => {
  setSelectedKey(s.key);
  setSubagentOpen(undefined);
  setPage("sessions");
};

/**
 * The session's terminal: Terminal tab, connected as the person opened it. The
 * terminal store loads with its first use, as the app's other terminal entries do.
 */
export function openTerminal(s: Session) {
  select(s);
  void import("../../store/terminal").then(({ terminal }) => { terminal.opened(); setDetailTab("terminal"); });
}

/** Whether Open terminal can work: the session has a terminal to attach to and its machine answers. */
export const terminalAvailable = (s: Session): boolean => !computerOffline(s.computer) && hasTerminal(s);

export type ChipKind = "services" | "background" | "git";

/**
 * A chip opens where its count lives: git in the side panel's checkouts; services
 * and background commands in Processes, else the native DeepSeek UI, else the terminal.
 */
export function openChip(s: Session, kind: ChipKind) {
  if (kind === "git") {
    select(s);
    // The side panel pulls in the skin runtime; it loads when a chip needs it.
    void import("../../store/inspector").then((inspector) => inspector.openInspector("worktrees"));
    return;
  }
  if (flag("processes/enabled") && !computerOffline(s.computer)) {
    batch(() => { select(s); setDetailTab("processes"); });
    return;
  }
  if (s.agent === "deepseek") { nativeRoute(s)(); return; }
  if (!hasTerminal(s)) { notify("error", tr("This session has no terminal to attach to."), true); return; }
  openTerminal(s);
}
