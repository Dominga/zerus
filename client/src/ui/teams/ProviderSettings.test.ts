// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ProviderSettings } from "./ProviderSettings";
import { teamView } from "./fixtures";
import payload from "../../../core/src/teams/fixtures/providers.json";
import type { ProviderInspection, ProviderRead } from "../../store/teamProviders";
import type { TeamCall, TeamOutcome, TeamView } from "../../store/teams";
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
const tick = async () => { await new Promise((resolve) => setTimeout(resolve, 0)); };
const inspection = () => structuredClone(payload.inspection) as ProviderInspection;
const view = (): TeamView => ({ ...structuredClone(teamView), revision: inspection().revision, bar: { ...teamView.bar, id: inspection().team, home: inspection().home, project: inspection().project, revision: inspection().revision } });
function setup(port = vi.fn(async (_: ProviderRead) => inspection()), outcome: TeamOutcome = { outcome: "done", receipt: {}, request_id: "example" }) {
  const host = document.createElement("div"); document.body.append(host);
  const [shown, show] = createSignal(view());
  const save = vi.fn(async (_: TeamCall) => outcome);
  dispose = render(() => createComponent(ProviderSettings, { get view() { return shown(); }, locked: false, onSave: save, port }), host);
  const button = (key: string) => host.querySelector<HTMLButtonElement>(`[data-provider-${key}]`)!;
  const input = (text: string) => { const field = host.querySelector<HTMLTextAreaElement>("[data-provider-override]")!; field.value = text; field.dispatchEvent(new Event("input", { bubbles: true })); };
  return { host, save, show, button, input, port };
}
it("opens and checks real product metadata without saving or starting work", async () => {
  const f = setup(); await tick(); expect(f.port).toHaveBeenCalledOnce(); expect(f.save).not.toHaveBeenCalled();
  expect(f.host.textContent).toContain("Detected from project metadata"); expect(f.host.textContent).toContain(inspection().digest);
  expect(f.host.textContent).toContain("cargo-test"); expect(f.host.textContent).toContain("cargo-build");
  expect(f.host.textContent).toContain("Unsupported: direct runners");
  expect(f.button("save").disabled).toBe(true); f.button("check").click(); await tick();
  expect(f.port).toHaveBeenLastCalledWith({ team: inspection().team, candidate: { override_config: null } });
  expect(f.save).not.toHaveBeenCalled(); expect(f.button("save").disabled).toBe(false);
  f.button("save").click(); await tick(); expect(f.save).toHaveBeenCalledOnce();
  expect(f.save.mock.calls[0][0]).toMatchObject({ team: inspection().team, expected_revision: inspection().revision, action: { kind: "validation_config", selection: { source_digest: inspection().source_digest, digest: inspection().digest, override_config: null } } });
});
it("invalid text and source diagnostics cannot be saved", async () => {
  const f = setup(); await tick(); f.input("["); f.button("check").click(); await tick();
  expect(f.port).toHaveBeenCalledOnce(); expect(f.button("save").disabled).toBe(true);
  expect(f.host.textContent).toContain("Enter a valid provider configuration as JSON.");
});
it("a changed draft invalidates the checked selection", async () => {
  const f = setup(); await tick(); f.button("check").click(); await tick(); expect(f.button("save").disabled).toBe(false);
  f.input('{"schema":1,"provider":[]}'); expect(f.button("save").disabled).toBe(true);
});
it("a malformed product check displays diagnostics and keeps save disabled", async () => {
  // Controlled corruption of a product-generated inspection, not a hand-written Teams row.
  const bad = inspection(); bad.valid = false; bad.diagnostics = [{ code: "invalid_config", message: "Malformed .zerus/tests.toml." }];
  const f = setup(vi.fn(async () => bad)); await tick(); f.button("check").click(); await tick();
  expect(f.host.textContent).toContain("Malformed .zerus/tests.toml."); expect(f.button("save").disabled).toBe(true); expect(f.save).not.toHaveBeenCalled();
});
it("old-service errors stay unavailable and Read again performs a fresh read", async () => {
  const f = setup(vi.fn(async () => { throw new Error("not_found: Unknown team command"); })); await tick();
  expect(f.host.textContent).toContain("Provider inspection is unavailable"); expect(f.button("save").disabled).toBe(true);
  f.button("read").click(); await tick(); expect(f.port).toHaveBeenCalledTimes(2); expect(f.save).not.toHaveBeenCalled();
});
it("late answers cannot replace another team or revision", async () => {
  const resolve: Array<(value: ProviderInspection) => void> = [];
  const f = setup(vi.fn((_read: ProviderRead) => new Promise<ProviderInspection>((r) => resolve.push(r))));
  const next = view(); next.revision++; next.bar.revision++; f.show(next); await tick();
  const fresh = inspection(); fresh.revision++; fresh.digest = "c".repeat(64); resolve[1](fresh); await tick();
  resolve[0](inspection()); await tick(); expect(f.host.textContent).toContain(fresh.digest); expect(f.host.textContent).not.toContain(inspection().digest); expect(f.save).not.toHaveBeenCalled();
});
it("an unknown save keeps a new selection locked and never reposts", async () => {
  const f = setup(undefined, { outcome: "uncertain", detail: "lost reply", request_id: "original" }); await tick();
  f.button("check").click(); await tick(); f.button("save").click(); await tick();
  expect(f.button("save").disabled).toBe(true); expect(f.button("check").disabled).toBe(true); expect(f.save).toHaveBeenCalledOnce();
  expect(f.host.textContent).toContain("Check the original action");
});

it.each(["home", "project", "team"] as const)("rejects an inspection from another %s", async (key) => {
  const bad = inspection(); bad[key] = "foreign";
  const f = setup(vi.fn(async () => bad)); await tick();
  expect(f.host.textContent).toContain("The team changed. Read its provider configuration again.");
  expect(f.button("save").disabled).toBe(true); expect(f.save).not.toHaveBeenCalled();
});
