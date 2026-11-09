// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";

const fixture = vi.hoisted(() => ({ openClear: vi.fn(), openInspector: vi.fn(), warning: "" }));
vi.mock("../../store/composer", async () => {
  const { createSignal: signal } = await import("solid-js");
  const [warning, setWarning] = signal("");
  fixture.warning = "";
  (fixture as unknown as { setWarning: (value: string) => void }).setWarning = setWarning;
  return { composer: {
    view: () => ({ cache: { visible: true, tone: "warning", label: "Cache 4 min left", short_label: "4m" }, cache_warning: warning(),
      cache_details: "Cached prompt expires in 4 min.", clear: { cache_enabled: true, cache_reason: "" } }),
    openClear: fixture.openClear,
  } };
});
vi.mock("../../store/inspector", () => ({ openInspector: fixture.openInspector }));

import { CacheChip } from "./CacheChip";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.clearAllMocks(); });

function mount() {
  const host = document.createElement("div"); document.body.append(host);
  const [compact] = createSignal(false);
  dispose = render(() => createComponent(CacheChip, { get compact() { return compact(); } }), host);
  const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label);
  return { host, button };
}

it("Usage details closes the popover and opens the side panel's Details tab (Qt SessionsWindow.cpp:993-1000)", () => {
  const { host, button } = mount();
  host.querySelector<HTMLButtonElement>(".tchip")!.click();
  expect(host.querySelector(".cache-pop")).not.toBeNull();
  button("Usage details")!.click();
  expect(fixture.openInspector).toHaveBeenCalledExactlyOnceWith("details");
  expect(host.querySelector(".cache-pop")).toBeNull();
  expect(fixture.openClear).not.toHaveBeenCalled();
});

it("shows Usage details without a warning, and Clear context only with one", () => {
  const { host, button } = mount();
  host.querySelector<HTMLButtonElement>(".tchip")!.click();
  expect(button("Usage details")).toBeDefined();
  expect(button("Clear context")).toBeUndefined();
  (fixture as unknown as { setWarning: (value: string) => void }).setWarning("The cache expires soon.");
  expect(button("Clear context")).toBeDefined();
  button("Clear context")!.click();
  expect(fixture.openClear).toHaveBeenCalledTimes(1);
  expect(fixture.openInspector).not.toHaveBeenCalled();
});

it("returns keyboard focus to the chip after Usage details or Clear context (zerus-5wmh.13.7.1.1)", async () => {
  const { host, button } = mount();
  const chip = host.querySelector<HTMLButtonElement>(".tchip")!;
  // click() without a pointer is Enter or Space on the chip: the popover focuses its first action.
  chip.focus(); chip.click();
  await Promise.resolve();
  expect(document.activeElement).toBe(button("Usage details"));
  button("Usage details")!.click();
  expect(fixture.openInspector).toHaveBeenCalledExactlyOnceWith("details");
  expect(document.activeElement).toBe(chip);
  (fixture as unknown as { setWarning: (value: string) => void }).setWarning("The cache expires soon.");
  chip.click();
  await Promise.resolve();
  button("Clear context")!.focus();
  button("Clear context")!.click();
  expect(fixture.openClear).toHaveBeenCalledTimes(1);
  expect(host.querySelector(".cache-pop")).toBeNull();
  expect(document.activeElement).toBe(chip);
});
