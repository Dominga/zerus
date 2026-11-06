import { describe, expect, it } from "vitest";
import { EDITOR, afterEditor, saveAction, withValue } from "./account-permissions";
import type { PermissionCheck, PermissionChoice } from "../../bridge";

// Task 68: the permissions dialog keeps only which row is chosen and the
// native editor's selection; client/core (account_actions::permissions)
// supplies the rows, the explanations and each selection's problem.

const t = (source: string) => ({ source });
const choice = (value: string, over: Partial<PermissionChoice> = {}): PermissionChoice =>
  ({ label: t(value), value, tooltip: t(""), explanation: t(""), bypass: false, unavailable: false, editor: false, ...over });
const choices = [choice("provider"), choice("bypass", { bypass: true }), choice(EDITOR, { editor: true, label: t("Native permission settings…") })];
const check = (mode: string, label = "Native permission settings… (%1)"): PermissionCheck =>
  ({ mode, summary: t("Never"), choice_label: { source: label, args: [{ kind: "text", value: t("Never") }] }, problem: t(""), rows: [] });

describe("account permissions dialog", () => {
  it("saves an offered row, opens the editor for the bare editor row and blocks a saved row no longer offered", () => {
    expect(saveAction(choices, "provider")).toBe("save");
    expect(saveAction(choices, EDITOR)).toBe("editor");
    const withSaved = [...choices, choice("auto", { unavailable: true })];
    expect(saveAction(withSaved, "auto")).toBe("blocked");
    expect(saveAction(choices, "forged")).toBe("blocked");
  });

  it("puts the editor's map on the editor row, and an empty map back on the harness's own settings", () => {
    const native = 'native:{"approval":"never"}';
    const after = afterEditor(choices, check(native));
    expect(after.value).toBe(native);
    expect(after.choices.map((c) => c.value)).toEqual(["provider", "bypass", native]);
    expect(after.choices[2]).toMatchObject({ editor: true, unavailable: false });
    expect(after.choices[2].label.source).toBe("Native permission settings… (%1)");
    expect(saveAction(after.choices, native)).toBe("save");
    // A saved map with an unknown setting is fixed in the editor, then saved.
    const unknown = [choices[0], choices[1], choice('native:{"gone":"x"}', { editor: true, unavailable: true })];
    expect(saveAction(unknown, 'native:{"gone":"x"}')).toBe("blocked");
    expect(saveAction(afterEditor(unknown, check(native)).choices, native)).toBe("save");
    expect(afterEditor(choices, check("provider")).value).toBe("provider");
  });

  it("keeps a control's value or drops it for the harness's own setting", () => {
    expect(withValue({ approval: "never" }, "sandbox", "read-only")).toEqual({ approval: "never", sandbox: "read-only" });
    expect(withValue({ approval: "never", sandbox: "x" }, "sandbox", "")).toEqual({ approval: "never" });
  });
});
