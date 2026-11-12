import { describe, expect, it } from "vitest";
import type { Account, Profile } from "../types";
import {
  accountDisplayName, accountNameForId, accountOptionLabel, accountOptions, effortOptions, modelOptions,
  normalizeModelCatalog, selectAccount, selectModel, selectionCatalogKey,
} from "./provider-selection";

const usage = (email = "", organization = "") => ({ identity: { email, organization } }) as Profile["usage"];
const profile = (extra: Partial<Profile> = {}): Profile => ({
  key: "work", host: "", machine: "workstation", id: "work", label: "Default account", provider: "codex",
  native: false, installed: true, is_default: false, signed_in: true, usage: usage(), ...extra,
}) as Profile;
const account = (members: Profile[], label = "Work"): Account => ({
  key: "account:work", label, provider: members[0]?.provider ?? "codex", installed: true,
  machines: members.map((member) => member.machine), members, usage: members[0]?.usage ?? usage(),
} as Account);

describe("provider account presentation", () => {
  it("uses the Accounts group label before profile and provider identity", () => {
    const member = profile({ label: "Default account", usage: usage("person@example.invalid") });
    expect(accountDisplayName(account([member]), member)).toBe("Work");
    expect(accountDisplayName(undefined, member)).toBe("Default account");
    expect(accountDisplayName(undefined, profile({ label: "", usage: usage("person@example.invalid") }))).toBe("person@example.invalid");
    expect(accountDisplayName(undefined, profile({ label: "", usage: usage() }))).toBe("work");
  });

  it("resolves a configured name by exact computer, provider and profile id", () => {
    const member = profile();
    const view = { accounts: [account([member])] };
    expect(accountNameForId(view, "", "codex", "work")).toBe("Work");
    expect(accountNameForId(view, "studio", "codex", "work")).toBeUndefined();
    expect(accountNameForId(view, "", "claude", "work")).toBeUndefined();
  });

  it("builds exact launch refs with grouped labels and optional native profiles", () => {
    const member = profile();
    const native = profile({ key: "native", id: "native-codex", native: true });
    const view = { accounts: [account([member, native])] };
    expect(accountOptions(view, { host: "", includeNative: false })).toEqual([
      expect.objectContaining({ label: "Work", native: false, ref: { host: "", provider: "codex", profileId: "work" } }),
    ]);
    expect(accountOptions(view, { host: "" })).toHaveLength(2);
    expect(accountOptions(view, { host: "studio" })).toEqual([]);
  });

  it("disambiguates equal labels without changing identity", () => {
    const first = accountOptions({ accounts: [account([profile()])] }, { host: "" })[0];
    const secondGroup = { ...account([profile({ host: "studio", key: "remote" })], "Work"), key: "account:other" };
    const second = accountOptions({ accounts: [secondGroup] }, { host: "studio" })[0];
    expect(accountOptionLabel(first, [first, second])).toBe("Work (This computer / work)");
    expect(accountOptionLabel(second, [first, second])).toBe("Work (studio / work)");
    expect(accountOptionLabel(first, [first])).toBe("Work");
  });
});

describe("provider model and effort selection", () => {
  it("normalizes account models with aligned labels, defaults and no duplicates", () => {
    const catalog = normalizeModelCatalog({
      effort_supported: true,
      defaults: { model: "model-a", effort: "high" },
      provider_defaults: { model: "model-b", effort: "low" },
      models: [
        { id: "model-a", label: "Model A", effort_options: ["low", "high", "low"], effort_labels: ["Low", "High"], default_effort: "high" },
        { id: "model-a", label: "Duplicate" },
      ],
    });
    expect(catalog.models).toEqual([{ id: "model-a", label: "Model A", defaultEffort: "high",
      efforts: [{ id: "low", label: "Low" }, { id: "high", label: "High" }] }]);
    expect(catalog.defaults).toEqual({ model: "model-a", effort: "high" });
    expect(catalog.providerDefaults).toEqual({ model: "model-b", effort: "low" });
  });

  it("normalizes OpenCode value, efforts and effortLabels", () => {
    const catalog = normalizeModelCatalog({ model_options: [
      { value: "provider/model", label: "Model", efforts: ["low"], effortLabels: ["Low"] },
    ], model_options_truncated: true });
    expect(catalog.models[0]).toEqual({ id: "provider/model", label: "Model", efforts: [{ id: "low", label: "Low" }] });
    expect(catalog.truncated).toBe(true);
  });

  it("retains unavailable current values without inventing generic efforts", () => {
    const catalog = normalizeModelCatalog([{ id: "model-a", effort_options: [] }]);
    expect(modelOptions(catalog, "custom")).toContainEqual({ id: "custom", label: "custom", efforts: [], unavailable: true });
    expect(effortOptions(catalog, "model-a")).toEqual([{ id: "", label: "Provider default" }]);
    expect(effortOptions(catalog, "model-a", "max")).toContainEqual({ id: "max", label: "max" });
  });

  it("resets dependents on account change and preserves only a supported effort", () => {
    const initial = { account: { kind: "default", host: "", provider: "codex" } as const, modelId: "model-a", effortId: "high" };
    expect(selectAccount(initial, { kind: "profile", ref: { host: "", provider: "codex", profileId: "work" } }))
      .toMatchObject({ modelId: "", effortId: "" });
    const catalog = normalizeModelCatalog([{ id: "model-a", effort_options: ["low", "high"], default_effort: "high" }]);
    expect(selectModel(catalog, "model-a", "low")).toEqual({ modelId: "model-a", effortId: "low" });
    expect(selectModel(catalog, "model-a", "max")).toEqual({ modelId: "model-a", effortId: "high" });
    expect(selectionCatalogKey({ host: "", provider: "codex", profileId: "work", folder: "/project" }))
      .toBe('["","codex","work","/project"]');
  });
});
