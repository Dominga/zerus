// @vitest-environment happy-dom
// Root product writers -> Core/model/Change/batch -> actual lazy PhasePane.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { PhasePane } from "./PhasePane";
import type { TeamView } from "../../store/teams";
import frames from "./fixtures/phase-estimates-view.json";
let lease = 1;
const calls: string[] = [];
vi.mock("@tauri-apps/api/core", async (original) => ({ ...(await original<typeof import("@tauri-apps/api/core")>()), isTauri: () => false,
  invoke: async (command: string) => { calls.push(command); return lease; } }));
let stop: (() => void) | undefined;
afterEach(async () => { stop?.(); await Promise.resolve(); await Promise.resolve(); document.body.replaceChildren(); calls.length = 0; });
async function mount(value: TeamView) {
  lease = value.plan!.observed!.epoch;
  const [view, setView] = createSignal(value);
  const host = document.createElement("div"); document.body.append(host);
  stop = render(() => createComponent(PhasePane, { get view() { return view(); }, busy: false, follow: vi.fn().mockResolvedValue(value.plan!.epoch) }), host);
  await vi.waitFor(() => expect(host.querySelector("[data-observed-panel]")).not.toBeNull());
  expect(host.querySelector("[data-phase-estimates]")).toBeNull();
  expect(calls).toHaveLength(0);
  (host.querySelector("[data-observed-panel] > button") as HTMLButtonElement).click();
  await vi.waitFor(() => expect(host.querySelector("[data-phase-estimates]")).not.toBeNull());
  return { host, setView };
}
it("shows known producer-written draft estimates on a valid empty observation page", async () => {
  const { host } = await mount(structuredClone(frames.preview) as unknown as TeamView);
  expect([...host.querySelectorAll("[data-estimate-minutes]")].map(n => n.textContent)).toEqual(["30 min", "30 min"]);
  expect(host.textContent).toContain("Unapproved draft preview");
  expect(host.textContent).toContain("has no actual comparison");
  expect(host.querySelectorAll("[data-estimate-unit]")).toHaveLength(0);
  expect(calls).toEqual(["teams_observed"]);
});
it.each(["published", "rework_reserved", "rework_published"] as const)("mounts %s producer estimates without filling original observation membership", async (name) => {
  const { host } = await mount(structuredClone(frames[name]) as unknown as TeamView);
  expect(host.textContent).toContain("Frozen publication estimates");
  expect(host.querySelectorAll("[data-estimate-unit]").length).toBeGreaterThan(0);
  const reason = name === "published" ? "The original observation did not retain its publication identity."
    : name === "rework_reserved" ? "The estimate is unavailable for this unit." : "This observed unit is outside the frozen estimate cohort.";
  expect(host.textContent).toContain(reason);
  expect(calls.every(command => command === "teams_observed")).toBe(true);
});
it("clears retained numeric estimates on the actual pane's failed phase read", async () => {
  const value = structuredClone(frames.published) as unknown as TeamView;
  const { host, setView } = await mount(value);
  expect(host.querySelector("[data-estimate-minutes]")).not.toBeNull();
  setView({ ...value, plan: { ...value.plan!, draft_failure: { code: "unavailable", message: "Selected read failed" } } });
  expect(host.querySelector("[data-estimate-minutes]")).toBeNull();
});
it("a synthetic stale derived lease cannot display a retained estimate", async () => {
  const value = structuredClone(frames.published) as unknown as TeamView;
  value.plan!.estimates!.epoch += 1;
  const { host } = await mount(value);
  expect(host.querySelector("[data-estimate-minutes]")).toBeNull();
  expect(host.textContent).toContain("Estimates are unavailable for the selected scope.");
});
it("closing and reopening requires the fresh observed lease, not the retained frame", async () => {
  const value = structuredClone(frames.published) as unknown as TeamView;
  const { host } = await mount(value);
  const toggle = host.querySelector("[data-observed-panel] > button") as HTMLButtonElement;
  toggle.click(); expect(host.querySelector("[data-phase-estimates]")).toBeNull();
  await Promise.resolve(); await Promise.resolve(); lease += 1;
  toggle.click(); await Promise.resolve(); await Promise.resolve();
  expect(host.querySelector("[data-estimate-minutes]")).toBeNull();
});
