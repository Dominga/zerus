import { describe, expect, it } from "vitest";
import { terminalMenu } from "./terminal-menu";

describe("the terminal context menu", () => {
  it("copies a selection and always pastes, as Qt's TerminalScreen menu", () => {
    expect(terminalMenu("the selected text")).toEqual([
      { id: "copy", label: "Copy selection", disabled: false },
      { id: "paste", label: "Paste", disabled: false },
    ]);
  });

  it("offers Copy disabled without a selection", () => {
    expect(terminalMenu("")[0]).toEqual({ id: "copy", label: "Copy selection", disabled: true });
  });
});
