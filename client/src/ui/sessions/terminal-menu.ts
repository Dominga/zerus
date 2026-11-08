import { tr } from "../../i18n";

// Task #247: Qt's TerminalScreen::contextMenuEvent (TerminalScreen.cpp:731):
// Copy selection (enabled only with a selection) and Paste. The menu is the
// Tauri in-app popup, never the webview's own context menu.

export interface TerminalMenuItem {
  id: "copy" | "paste";
  label: string;
  disabled: boolean;
}

/** The context menu of a screen with this selection. */
export function terminalMenu(selection: string): TerminalMenuItem[] {
  return [
    { id: "copy", label: tr("Copy selection"), disabled: selection === "" },
    { id: "paste", label: tr("Paste"), disabled: false },
  ];
}
