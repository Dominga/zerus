import { describe, expect, it } from "vitest";
import { APPROVALS_TITLE, APPROVALS_BODY, APPROVALS_CONFIRM, APPROVALS_WARNING, APPROVALS_HINT } from "./approval-opt-in";
import contract from "./fixtures/text-contracts.json";
import catalog from "../../../i18n.ru.json";

// These exported translation keys are shared by the reviewed consent dialog.
// Ordinary Settings labels/order no longer mirror the retired Qt UI.
const keys = { APPROVALS_TITLE, APPROVALS_BODY, APPROVALS_CONFIRM, APPROVALS_WARNING, APPROVALS_HINT };
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const repo = new URL("../../../../../", import.meta.url);
const mockup = ["design/webui/src/screens/settings/Policies.tsx", "design/webui/src/dialogs/SettingsDialogs.tsx"]
  .map((file) => fs.readFileSync(new URL(file, repo), "utf8")).join("\n");

describe("Automatic approval consent translation keys", () => {
  it("keeps the exported keys used by the consent dialog", () => {
    expect(keys).toEqual(contract.approvalTranslationKeys);
  });
  it.each(Object.values(contract.approvalTranslationKeys))("retains the reviewed and translated consent: %s", (key) => {
    expect(mockup).toContain(key);
    expect(Object.hasOwn(catalog, key)).toBe(true);
  });
});
