import { describe, expect, it, vi } from "vitest";
import type { CatalogModel } from "../types";
import { settle } from "./newSessionFake";
import { createStandardModels } from "./newSessionProviderModels";
import type { OpenCodeHost } from "./newSessionOpencode";

function deferred<T>() {
  let release!: (value: T) => void;
  const promise = new Promise<T>((resolve) => { release = resolve; });
  return { promise, release };
}

function host(agent: string, read: OpenCodeHost["bridge"]["teamRoleModels"]): OpenCodeHost {
  const profile: Record<string, unknown> = { id: `native-${agent}` };
  return {
    bridge: { teamRoleModels: read } as OpenCodeHost["bridge"],
    host: () => "", path: () => "/workspace/example", agent: () => agent, machineName: () => "pc",
    accountProfile: () => profile, openCodeProfile: () => ({}), online: () => true,
    emit: vi.fn(), updateForm: vi.fn(), accountsLoading: () => false, reloadAccounts: vi.fn(),
    markRowFavorite: vi.fn(), setAccountStatus: vi.fn(), showError: vi.fn(), clearError: vi.fn(),
  };
}

describe("standard provider models", () => {
  it("keeps Kimi effort in native settings", async () => {
    const h = host("kimi", async () => [{ id: "model-a", label: "Model A", effort_options: ["high"], effort_labels: ["High"], default_effort: "high" }]);
    const store = createStandardModels(h);
    store.updateModels();
    await settle();
    store.actions.chooseModel("model-a");
    expect(store.view()).toMatchObject({ model: "model-a", effort: "", effortVisible: false });
    store.actions.chooseEffort("high");
    expect(store.view().effort).toBe("");
  });

  it("ignores a catalog answer for the account that is no longer selected", async () => {
    const first = deferred<CatalogModel[]>(), second = deferred<CatalogModel[]>();
    const h = host("codex", async (_computer, _agent, account) => account === "first" ? first.promise : second.promise);
    const store = createStandardModels(h);
    store.updateModels();
    (h.accountProfile as () => Record<string, unknown>) = () => ({ id: "second" });
    store.updateModels();
    first.release([{ id: "stale", label: "Stale", effort_options: [] }]);
    await settle();
    expect(store.view().models.map((model) => model.value)).toEqual([""]);
    second.release([{ id: "current", label: "Current", effort_options: [] }]);
    await settle();
    expect(store.view().models.map((model) => model.value)).toEqual(["", "current"]);
  });
});
