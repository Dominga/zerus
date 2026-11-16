// @vitest-environment happy-dom
// Rows: root product writers -> strict core projector -> model/batch fixture.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { ObservedPanel } from "./ObservedPanel";
import type { ObservedPane } from "../../store/observed-timing";
import fixture from "../../../core/src/teams/fixtures/observed-pane.json";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
const disposers: (() => void)[] = [];
afterEach(() => { disposers.splice(0).forEach((dispose) => dispose()); document.body.replaceChildren(); });
const settle = async () => { for (let i = 0; i < 16; i++) await Promise.resolve(); };
function mount() {
  const pane = structuredClone(fixture) as unknown as ObservedPane;
  const [current, setCurrent] = createSignal<ObservedPane | null>(pane);
  const follow = vi.fn().mockResolvedValue(pane.epoch);
  const host = document.createElement("div"); document.body.append(host);
  disposers.push(render(() => createComponent(ObservedPanel, { team: pane.page.scope.team, phase: pane.page.scope.phase, get pane() { return current(); }, follow }), host));
  return { host, setCurrent, pane, follow };
}
it("lazily renders the finite product page without measured work or lifecycle actions", async () => {
  const { host, follow } = mount(); await settle(); expect(follow).not.toHaveBeenCalled();
  (host.querySelector("[aria-expanded]") as HTMLButtonElement).click(); await settle();
  expect(follow).toHaveBeenCalledTimes(1); expect(host.textContent).toContain("Implementation time is unavailable");
  expect(host.querySelectorAll("[data-observed-fact]")).toHaveLength(1);
  expect(host.textContent).toContain("Partial observation window");
  expect(host.querySelector("input,textarea,select")).toBeNull();
  const members = [...host.querySelectorAll("button")].find((b) => b.textContent === "Members")!;
  members.click(); await settle(); expect(host.textContent).toContain("Unattributed"); expect(follow).toHaveBeenCalledTimes(1);
});
it("drops prior coverage when actual core/store state retires it and rejects an old lease", async () => {
  const { host, setCurrent, pane } = mount(); (host.querySelector("[aria-expanded]") as HTMLButtonElement).click(); await settle();
  expect(host.querySelectorAll("[data-observed-fact]")).toHaveLength(1);
  setCurrent(null); await settle(); expect(host.querySelector("[data-observed-fact]")).toBeNull(); expect(host.textContent).toContain("coverage is unavailable");
  setCurrent({ ...pane, epoch: pane.epoch + 1 }); await settle(); expect(host.querySelector("[data-observed-fact]")).toBeNull();
});
it("labelled synthetic gap overlay never renders a guessed duration", async () => {
  const { host, setCurrent, pane } = mount();
  const gap = structuredClone(pane); gap.page.reasons.push("clock_discontinuity"); gap.page.rows[0].category = "gap";
  setCurrent(gap); (host.querySelector("[aria-expanded]") as HTMLButtonElement).click(); await settle();
  expect(host.textContent).toContain("No duration is inferred across the gap"); expect(host.textContent).toContain("Observation gap");
});
