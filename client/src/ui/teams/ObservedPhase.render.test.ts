// @vitest-environment happy-dom
// Root published phase + planned-task writers -> core model and batch -> pane.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { PhasePane } from "./PhasePane";
import type { TeamView } from "../../store/teams";
import data from "../../../core/src/teams/fixtures/observed-phase-view.json";
import reworkViews from "../../../core/src/teams/fixtures/observed-rework-views.json";
const calls: { command: string; args: unknown }[] = [];
let observedLease = 1;
vi.mock("@tauri-apps/api/core", async (original) => ({ ...(await original<typeof import("@tauri-apps/api/core")>()), isTauri: () => false, invoke: async (command: string, args: unknown) => { calls.push({ command, args }); return observedLease; } }));
let dispose: (() => void) | undefined;
afterEach(async () => { dispose?.(); await Promise.resolve(); await Promise.resolve(); document.body.replaceChildren(); calls.length = 0; });
it("opens recorded observations from the actual published phase without sending an action", async () => {
  const view = structuredClone(data) as unknown as TeamView;
  observedLease = view.plan!.observed!.epoch;
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(PhasePane, { view, busy: false, follow: vi.fn().mockResolvedValue(view.plan!.epoch) }), host);
  await vi.waitFor(() => expect(host.querySelector("[data-observed-panel]")).not.toBeNull());
  expect(calls).toHaveLength(0);
  (host.querySelector("[data-observed-panel] > button") as HTMLButtonElement).click();
  await vi.waitFor(() => expect(host.querySelectorAll("[data-observed-fact]")).toHaveLength(2));
  expect(calls).toEqual([{ command: "teams_observed", args: { team: view.bar.id, phase: 1, node: null, windowMs: 86_400_000, enabled: true, expectedEpoch: null } }]);
  expect(host.textContent).toContain("Implementation time is unavailable");
  expect(view.plan!.observed!.page.rows.every((r) => r.key.publication === null && r.implementation_observed_ms === null)).toBe(true);
});

it.each(["reserved", "published"] as const)("renders the actual admitted %s rework observation scope without original-membership inference", async (state) => {
  const view = structuredClone(reworkViews[state]) as unknown as TeamView;
  observedLease = view.plan!.observed!.epoch;
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(PhasePane, { view, busy: false, follow: vi.fn().mockResolvedValue(view.plan!.epoch) }), host);
  await vi.waitFor(() => expect(host.querySelector("[data-observed-panel]")).not.toBeNull());
  (host.querySelector("[data-observed-panel] > button") as HTMLButtonElement).click();
  await vi.waitFor(() => expect(host.querySelectorAll("[data-observed-fact]")).toHaveLength(2));
  expect(view.plan!.observed!.page.scope.publication!.id).toBe(view.plan!.draft!.publication!.id);
  expect(view.plan!.observed!.page.rows.every((row) => row.key.publication === null && row.implementation_observed_ms === null)).toBe(true);
  expect(host.textContent).toContain("Implementation time is unavailable");
  expect(calls.every((call) => call.command === "teams_observed")).toBe(true);
});
