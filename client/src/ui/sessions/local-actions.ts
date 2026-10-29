import type { Inspect } from "../../types";

// The buttons of a failed local message row (SessionsWindow.cpp messageAction,
// 4470-4527): Retry, Check Terminal or Check native UI, and Delete. Apart
// from the view so they can be tested.

/** Check native UI before the Native UI tab exists (Task 14). */
export const NATIVE_UI_LATER = "Arrives with Native UI.";
/** Delete removes the row for good: it asks first (destructive actions confirm). */
export const DELETE_TITLE = "Delete message?";
export const DELETE_TEXT = "Remove this message from Activity? This cannot be undone.";

export interface InspectPorts {
  /** The Terminal tab, connected (Qt's setCurrentWidget(m_terminal) and connectSession). */
  showTerminal(): void;
  notify(tone: "info", text: string): void;
}

/** Where Check leads (messageAction "inspect", 4477-4481); nothing once its composer went. */
export function routeInspect(inspect: Inspect | undefined, ports: InspectPorts): void {
  if (!inspect) return;
  if (inspect.kind === "terminal") ports.showTerminal();
  else if (inspect.kind === "native_ui") ports.notify("info", NATIVE_UI_LATER);
  // Rust already reads the session again (Qt's inspect()); the notice says what to check.
  else ports.notify("info", inspect.notice);
}
