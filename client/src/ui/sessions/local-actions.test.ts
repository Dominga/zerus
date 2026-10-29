import { describe, expect, it } from "vitest";
import { DELETE_TEXT, DELETE_TITLE, NATIVE_UI_LATER, routeInspect } from "./local-actions";

describe("a failed local row's Check (SessionsWindow.cpp messageAction 4477-4481)", () => {
  const ports = () => {
    const shown: string[] = [];
    return { shown, ports: { showTerminal: () => { shown.push("terminal"); }, notify: (tone: "info", text: string) => { shown.push(`${tone} ${text}`); } } };
  };

  it("leads to Terminal, Native UI or a refresh notice, and nowhere once its composer went", () => {
    const t = ports();
    routeInspect({ kind: "terminal" }, t.ports);
    routeInspect({ kind: "native_ui" }, t.ports);
    routeInspect({ kind: "refresh", notice: "Refreshing the native session. Check its provider configuration on studio if the error remains." }, t.ports);
    routeInspect(undefined, t.ports);
    expect(t.shown).toEqual([
      "terminal",
      `info ${NATIVE_UI_LATER}`,
      "info Refreshing the native session. Check its provider configuration on studio if the error remains.",
    ]);
    expect(NATIVE_UI_LATER).toBe("Arrives with Native UI.");
  });

  it("Delete asks first", () => {
    expect([DELETE_TITLE, DELETE_TEXT]).toEqual(["Delete message?", "Remove this message from Activity? This cannot be undone."]);
  });
});
