// @vitest-environment happy-dom
// Handwritten read controls exercise the form; root tests use Git and product writers.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { TeamAdvanceBase } from "./TeamAdvanceBase";
import type { TeamBaseFacts } from "../../store/teams";

const old = "1".repeat(40), next = "2".repeat(40), fetched = "3".repeat(40);
const facts = (): TeamBaseFacts => ({ team: "fixture-team", revision: 7, base_commit: old, refs: [
  { reference: "refs/heads/main", commit: next }, { reference: "refs/remotes/origin/main", commit: fetched },
] });
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const mount = (props: Parameters<typeof TeamAdvanceBase>[0]) => {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(TeamAdvanceBase, props), host); return host;
};
it("handwritten base read shows both local main refs and sends only the chosen hash at the read revision", async () => {
  const advance = vi.fn(), load = vi.fn(async () => facts());
  const host = mount({ team: "fixture-team", busy: false, load, onAdvance: advance, onCancel: vi.fn() });
  await settle();
  expect(load).toHaveBeenCalledTimes(1);
  expect(host.textContent).toContain(old); expect(host.textContent).toContain(next); expect(host.textContent).toContain(fetched);
  expect(host.textContent).toContain("Earlier hand-outs keep their assigned base");
  expect(advance).not.toHaveBeenCalled();
  const input = host.querySelector("input")!;
  expect(input.value).toBe(fetched);
  input.value = next; input.dispatchEvent(new Event("input", { bubbles: true }));
  host.querySelector<HTMLButtonElement>("[data-team-base-continue]")!.click();
  expect(advance).toHaveBeenCalledWith(next, 7);
});
it("handwritten base read cannot submit the old base, short hash, or symbolic ref", async () => {
  const advance = vi.fn();
  const host = mount({ team: "fixture-team", busy: false, load: async () => facts(), onAdvance: advance, onCancel: vi.fn() });
  await settle();
  const input = host.querySelector("input")!;
  for (const value of [old, "main", "HEAD", "0123456"]) {
    input.value = value; input.dispatchEvent(new Event("input", { bubbles: true }));
    expect(host.querySelector<HTMLButtonElement>("[data-team-base-continue]")!.disabled).toBe(true);
  }
  expect(advance).not.toHaveBeenCalled();
});
it("a refused read displays its cause and closing never sends", async () => {
  const advance = vi.fn(), cancel = vi.fn();
  const host = mount({ team: "fixture-team", busy: false, load: async () => { throw new Error("No main refs are available"); }, onAdvance: advance, onCancel: cancel });
  await settle();
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("No main refs are available");
  expect(host.querySelector("[data-team-base-continue]")).toBeNull();
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Cancel")!.click();
  expect(cancel).toHaveBeenCalledOnce(); expect(advance).not.toHaveBeenCalled();
});
