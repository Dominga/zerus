// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { batch, createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import type { ActivityEvent } from "../../types";
import type { ActivitySource } from "../../store/activitySource";

// Exercise Activity's navigation and placement with deterministic row geometry.
// The virtualizer's own measurement/anchor behavior has separate coverage.
vi.mock("@tanstack/solid-virtual", async original => ({
  ...await original<object>(),
  createVirtualizer: (options: { count: number; getItemKey(index: number): string }) => ({
    itemSizeCache: new Map(), scrollOffset: 0,
    measure() {}, measureElement() {}, resizeItem() {},
    getTotalSize: () => options.count * 110,
    get measurementsCache() { return Array.from({ length: options.count }, (_, index) => ({ start: index * 110 })); },
    getVirtualItems: () => Array.from({ length: options.count }, (_, index) => ({ key: options.getItemKey(index), index, start: index * 110, end: index * 110 + 100 })),
  }),
}));

import { OwnMessageNavigation } from "./OwnMessageNavigation";
import { ActivityView, following, jumpToLatest } from "./Activity";
import { loadMarkdown } from "./markdown";

let dispose: (() => void) | undefined;
let frames: Map<number, FrameRequestCallback>, serial = 0;
const settle = async () => {
  for (let n = 0; n < 12; n++) {
    await Promise.resolve();
    const pending = [...frames.values()]; frames.clear();
    pending.forEach(run => run(n * 16));
  }
};
beforeEach(async () => {
  frames = new Map(); vi.useFakeTimers();
  vi.stubGlobal("requestAnimationFrame", (run: FrameRequestCallback) => { frames.set(++serial, run); return serial; });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function(this: HTMLElement) {
    return this.querySelectorAll(".act-virtual-row").length * 110 + 40;
  });
  vi.spyOn(HTMLElement.prototype, "scrollTo").mockImplementation(function(this: HTMLElement, options?: ScrollToOptions | number) {
    const top = typeof options === "object" ? options.top ?? 0 : options ?? 0;
    this.scrollTop = Math.max(0, Math.min(top, this.scrollHeight - this.clientHeight));
  });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function(this: HTMLElement) {
    const scroller = this.closest<HTMLElement>(".act-scroll");
    const row = this.closest<HTMLElement>(".act-virtual-row");
    const start = row ? Number(row.dataset.index) * 110 : 0;
    const top = this.classList.contains("act-scroll") ? 0 : start + 40 - (scroller?.scrollTop ?? 0);
    return { top, bottom: top + 100, left: 0, right: 900, width: 900, height: 100, x: 0, y: top, toJSON() {} };
  });
  await loadMarkdown();
});
afterEach(() => {
  dispose?.(); dispose = undefined; document.body.replaceChildren();
  vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});

const events = (from: number, count = 20): ActivityEvent[] => Array.from({ length: count }, (_, i) => ({ seq: from + i, type: "AgentMessage", source: "codex_transcript", detail: `Reply ${from + i}`, at: 1_793_000_000 + from + i }));
function mount() {
  const [target, setTarget] = createSignal("first-" + serial);
  const [rows, setRows] = createSignal(events(1));
  const [loaded, setLoaded] = createSignal(true);
  const [loadingOlder, setLoadingOlder] = createSignal(false);
  const [olderDone, setOlderDone] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const loadOlder = vi.fn();
  const source: ActivitySource = {
    target: () => ({ id: target() }), events: rows, loaded, loadOlder,
    summary: () => undefined, loadingOlder, olderDone, ended: () => false, error,
  };
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => [createComponent(ActivityView, { source }), createComponent(OwnMessageNavigation, {})], host);
  return { scroller: host.querySelector<HTMLElement>(".act-scroll")!, source, target, setTarget, setRows, setLoaded, setLoadingOlder, setOlderDone, setError, loadOlder, host };
}
async function read(scroller: HTMLElement, top: number) {
  scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -1, bubbles: true }));
  scroller.scrollTop = top; scroller.dispatchEvent(new Event("scroll"));
  await vi.advanceTimersByTimeAsync(250); await settle();
}

it("restores each session's reading row after switching, including new events while away", async () => {
  const view = mount(); await settle();
  const first = view.target();
  expect(view.scroller.querySelector<HTMLElement>(".act-col")?.style.visibility).toBe("visible");
  expect(view.scroller.querySelectorAll(".act-virtual-row")).toHaveLength(20);
  expect(following()).toBe(true);
  await read(view.scroller, 595);
  expect(following()).toBe(false);
  batch(() => { view.setTarget("second-" + serial); view.setRows(events(100)); }); await settle();
  expect(following()).toBe(true);
  await read(view.scroller, 815);
  const second = view.target();
  batch(() => { view.setTarget(first); view.setRows(events(1, 25)); }); await settle();
  expect(view.scroller.scrollTop).toBe(595);
  expect(following()).toBe(false);
  view.setRows(events(1, 26)); await settle();
  expect(view.scroller.scrollTop).toBe(595);
  batch(() => { view.setTarget(second); view.setRows(events(100)); }); await settle();
  expect(view.scroller.scrollTop).toBe(815);
  jumpToLatest(); await settle();
  expect(following()).toBe(true);
  expect(view.scroller.scrollTop).toBe(view.scroller.scrollHeight - 400);
  batch(() => { view.setTarget(first); view.setRows(events(1)); }); await settle();
  batch(() => { view.setTarget(second); view.setRows(events(100, 30)); }); await settle();
  expect(following()).toBe(true);
  expect(view.scroller.scrollTop).toBe(view.scroller.scrollHeight - 400);
});

it("restores after the Activity tab remounts and anchors by message rather than absolute pixels", async () => {
  const view = mount(); await settle();
  await read(view.scroller, 595);
  dispose!(); dispose = undefined;
  view.setRows(events(1, 25));
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(ActivityView, { source: view.source }), host);
  await settle();
  const scroller = host.querySelector<HTMLElement>(".act-scroll")!;
  expect(scroller.scrollTop).toBe(595);
  expect(following()).toBe(false);
  const first = view.target();
  batch(() => { view.setTarget("away-" + serial); view.setRows(events(100)); }); await settle();
  batch(() => { view.setTarget(first); view.setRows([...events(90, 3), ...events(1, 25)]); }); await settle();
  expect(scroller.scrollTop).toBe(595 + 3 * 110);
});

it("waits for a delayed first page and requests the saved window once if it left the tail", async () => {
  const view = mount(); await settle();
  const first = view.target();
  await read(view.scroller, 595);
  batch(() => { view.setTarget("other-" + serial); view.setRows(events(100)); }); await settle();
  batch(() => { view.setTarget(first); view.setLoaded(false); view.setRows([]); }); await settle();
  expect(view.loadOlder).not.toHaveBeenCalled();
  batch(() => { view.setRows(events(1000)); view.setLoaded(true); }); await settle();
  expect(view.loadOlder).toHaveBeenCalledExactlyOnceWith(5);
  view.setRows([...events(1), ...events(1000)]); await settle();
  expect(view.scroller.scrollTop).toBe(595);
  expect(following()).toBe(false);
});

it("an explicit Latest request cancels restoration while the first page is pending", async () => {
  const view = mount(); await settle();
  const first = view.target();
  await read(view.scroller, 595);
  batch(() => { view.setTarget("away-" + serial); view.setRows(events(100)); }); await settle();
  batch(() => { view.setTarget(first); view.setLoaded(false); view.setRows([]); }); await settle();
  jumpToLatest();
  batch(() => { view.setLoaded(true); view.setRows(events(1, 30)); }); await settle();
  expect(following()).toBe(true);
  expect(view.scroller.scrollTop).toBe(view.scroller.scrollHeight - 400);
  expect(view.loadOlder).not.toHaveBeenCalled();
});

it("reads an expired saved window once, then returns to Latest when no earlier message is left", async () => {
  const view = mount(); await settle();
  const first = view.target();
  await read(view.scroller, 595);
  batch(() => { view.setTarget("away-" + serial); view.setRows(events(100)); }); await settle();
  view.loadOlder.mockImplementation(() => view.setLoadingOlder(true));
  batch(() => { view.setTarget(first); view.setRows(events(1000)); }); await settle();
  expect(view.loadOlder).toHaveBeenCalledExactlyOnceWith(5);
  // The journal before the new tail expired: each settled read returns no rows.
  for (let n = 0; n < 3; n++) { view.setLoadingOlder(false); await vi.advanceTimersByTimeAsync(10_000); await settle(); }
  expect(view.loadOlder).toHaveBeenCalledTimes(1);
  expect(following()).toBe(true);
  expect(view.scroller.scrollTop).toBe(view.scroller.scrollHeight - 400);
});

it("places the nearest earlier surviving message at the top when no saved row returns, never the old pixel offset", async () => {
  const view = mount(); await settle();
  const first = view.target();
  await read(view.scroller, 595);
  batch(() => { view.setTarget("away-" + serial); view.setRows(events(100)); }); await settle();
  // The saved rows s6 to s9 were pruned; s4 is the nearest earlier message.
  batch(() => { view.setTarget(first); view.setRows([...events(1, 4), ...events(15, 10)]); }); await settle();
  expect(view.loadOlder).not.toHaveBeenCalled();
  expect(view.scroller.querySelector("#item-s4")!.getBoundingClientRect().top).toBe(0);
  expect(view.scroller.scrollTop).toBe(3 * 110 + 40);
  expect(following()).toBe(false);
  view.setRows([...events(1, 4), ...events(15, 11)]); await settle();
  expect(view.scroller.scrollTop).toBe(3 * 110 + 40);
  expect(following()).toBe(false);
});

it("returns to Latest and follows when the saved session's transcript was cleared", async () => {
  const view = mount(); await settle();
  const first = view.target();
  await read(view.scroller, 595);
  batch(() => { view.setTarget("away-" + serial); view.setRows(events(100)); }); await settle();
  batch(() => { view.setTarget(first); view.setRows([]); }); await settle();
  expect(view.loadOlder).not.toHaveBeenCalled();
  expect(following()).toBe(true);
  view.setRows(events(30, 10)); await settle();
  expect(following()).toBe(true);
  expect(view.scroller.scrollTop).toBe(view.scroller.scrollHeight - 400);
});

const own = (seq: number, more: Partial<ActivityEvent> = {}): ActivityEvent => ({ seq, type: "UserPromptSubmit", source: "desktop", detail: `My message ${seq}`, ...more });
const nav = (host: HTMLElement, direction: "Previous" | "Next") => host.querySelector<HTMLButtonElement>(`button[aria-label="${direction} message from you"]`)!;
const highlighted = (host: HTMLElement) => host.querySelector(".act-own-highlight [id^='item-']")?.id;

it("navigates only this session's person rows in both directions and highlights briefly", async () => {
  const view = mount();
  view.setOlderDone(true);
  view.setRows([own(1), ...events(2, 2), own(4, { origin: "peer_message", sender: "codex/example/peer" }),
    own(5, { inherited_from: "another session" }), own(6, { agent_id: "child" }),
    { seq: 7, type: "PreToolUse", detail: "Tool" }, own(8, { source: "phone" }), ...events(9, 10)]);
  await settle();
  const previous = nav(view.host, "Previous"), next = nav(view.host, "Next");
  expect(previous).not.toBeNull(); expect(next.disabled).toBe(true);
  previous.click(); await settle();
  expect(highlighted(view.host)).toBe("item-s8");
  expect(view.host.querySelector("#item-s8")!.getBoundingClientRect().top).toBe(0);
  expect(following()).toBe(false); expect(next.disabled).toBe(true);
  previous.click(); await settle();
  expect(highlighted(view.host)).toBe("item-s1"); expect(previous.disabled).toBe(true); expect(next.disabled).toBe(false);
  next.click(); await settle(); expect(highlighted(view.host)).toBe("item-s8");
  await vi.advanceTimersByTimeAsync(2000); expect(highlighted(view.host)).toBeUndefined();
  jumpToLatest(); await settle(); expect(next.disabled).toBe(true); expect(previous.disabled).toBe(false);
});

it("reads one bounded older page at a time and stops as soon as an own message is found", async () => {
  const view = mount(); view.setRows(events(40));
  view.loadOlder.mockImplementation(() => view.setLoadingOlder(true)); await settle();
  const previous = nav(view.host, "Previous"); previous.click(); await settle();
  expect(view.loadOlder).toHaveBeenCalledExactlyOnceWith();
  expect(previous.disabled).toBe(true); expect(previous.getAttribute("aria-busy")).toBe("true");
  expect(previous.title).toContain("Reading earlier messages"); expect(view.host.querySelector("[role=status]")?.textContent).toContain("Reading earlier messages");
  batch(() => { view.setRows([...events(20), ...events(40)]); view.setLoadingOlder(false); }); await settle();
  expect(view.loadOlder).toHaveBeenCalledTimes(2);
  batch(() => { view.setRows([own(19), ...events(20), ...events(40)]); view.setLoadingOlder(false); }); await settle();
  expect(highlighted(view.host)).toBe("item-s19"); expect(view.loadOlder).toHaveBeenCalledTimes(2);
  expect(previous.getAttribute("aria-busy")).toBe("false");
});

it("disables exhausted directions and cancels an older search on session change, error or Latest", async () => {
  const view = mount(); view.setRows(events(40));
  view.loadOlder.mockImplementation(() => view.setLoadingOlder(true)); await settle();
  nav(view.host, "Previous").click(); await settle();
  batch(() => { view.setOlderDone(true); view.setLoadingOlder(false); }); await settle();
  expect(nav(view.host, "Previous").disabled).toBe(true); expect(nav(view.host, "Next").disabled).toBe(true);
  view.setOlderDone(false); nav(view.host, "Previous").click(); await settle();
  batch(() => { view.setTarget("replacement-" + serial); view.setRows([own(1), ...events(2)]); view.setLoadingOlder(false); }); await settle();
  expect(highlighted(view.host)).toBeUndefined(); expect(view.loadOlder).toHaveBeenCalledTimes(2);
  view.setRows(events(40)); nav(view.host, "Previous").click(); await settle();
  batch(() => { view.setError("Unavailable"); view.setLoadingOlder(false); }); await settle();
  expect(nav(view.host, "Previous").getAttribute("aria-busy")).toBe("false"); expect(view.loadOlder).toHaveBeenCalledTimes(3);
  view.setError(undefined); nav(view.host, "Previous").click(); await settle(); jumpToLatest();
  batch(() => { view.setRows([own(1), ...events(40)]); view.setLoadingOlder(false); }); await settle();
  expect(highlighted(view.host)).toBeUndefined(); expect(following()).toBe(true);
});

it("includes the person's phone recording and rebases navigation after manual reading", async () => {
  const view = mount(); view.setOlderDone(true);
  view.setRows([own(1), ...events(2, 4), own(6, { source: "hgs_voice", voice: { state: "submitted", duration_ms: 1200 } }), ...events(7, 15)]);
  await settle();
  nav(view.host, "Previous").click(); await settle(); expect(highlighted(view.host)).toBe("item-s6");
  await read(view.scroller, 4 * 110 + 40);
  nav(view.host, "Previous").click(); await settle(); expect(highlighted(view.host)).toBe("item-s1");
});

it("runs the registered shortcut from the composer without editing or moving its focus", async () => {
  const { installShortcutRouter, platform, route } = await import("../../store/shortcuts");
  const view = mount(); view.setOlderDone(true); view.setRows([own(1), ...events(2)]); await settle();
  const field = document.createElement("textarea"); field.dataset.shortcutContext = "composer"; field.value = "Unsent draft"; view.host.append(field); field.focus();
  const stop = installShortcutRouter();
  try {
    const key = new KeyboardEvent("keydown", { key: "ArrowUp", code: "ArrowUp", ctrlKey: platform !== "macos", metaKey: platform === "macos", altKey: true, shiftKey: true, bubbles: true, cancelable: true });
    // happy-dom aliases AltGraph to Alt; this is a physical Ctrl+Alt chord.
    Object.defineProperty(key, "getModifierState", { value: () => false });
    expect(route(key)).toEqual({ run: "activity.previousOwn", consume: true });
    field.dispatchEvent(key); await settle();
    expect(key.defaultPrevented).toBe(true); expect(highlighted(view.host)).toBe("item-s1");
    expect(field.value).toBe("Unsent draft"); expect(document.activeElement).toBe(field);
  } finally { stop(); }
});

it("cancels an unpainted jump when a resumed run retains the same display window", async () => {
  const view = mount(); view.setOlderDone(true); view.setRows([own(1), ...events(2)]);
  const display = "retained-display-" + serial;
  view.source.displayKey = () => display;
  view.setTarget("before-resume-" + serial); await settle();
  const top = view.scroller.scrollTop;
  nav(view.host, "Previous").click();
  view.setTarget("after-resume-" + serial); await settle();
  expect(highlighted(view.host)).toBeUndefined(); expect(view.scroller.scrollTop).toBe(top);
});

it("keeps both direction states through a scroll burst and recomputes only after idle", async () => {
  const view = mount(); view.setOlderDone(true);
  view.setRows([own(1), ...events(2, 6), own(8), ...events(9, 20)]); await settle();
  const previous = nav(view.host, "Previous"), next = nav(view.host, "Next");
  const disabled = () => [previous.disabled, next.disabled];
  expect(disabled()).toEqual([false, true]);
  const flips: MutationRecord[] = [];
  const observer = new MutationObserver(records => flips.push(...records));
  observer.observe(previous, { attributes: true, attributeFilter: ["disabled"] });
  observer.observe(next, { attributes: true, attributeFilter: ["disabled"] });
  try {
    for (const top of [700, 300, 0]) {
      view.scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -1, bubbles: true }));
      view.scroller.scrollTop = top; view.scroller.dispatchEvent(new Event("scroll"));
      await vi.advanceTimersByTimeAsync(50); await settle();
      expect(disabled()).toEqual([false, true]); expect(flips).toHaveLength(0);
    }
    await vi.advanceTimersByTimeAsync(199); await settle();
    expect(disabled()).toEqual([false, true]); expect(flips).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1); await settle();
    expect(disabled()).toEqual([true, false]);
  } finally { observer.disconnect(); }
});

it("honors an enabled navigation click once the existing scroll gesture settles", async () => {
  const view = mount(); view.setOlderDone(true);
  view.setRows([own(1), ...events(2, 6), own(8), ...events(9, 20)]); await settle();
  view.scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -1, bubbles: true }));
  view.scroller.scrollTop = 500; view.scroller.dispatchEvent(new Event("scroll"));
  nav(view.host, "Previous").click();
  // The browser's remaining scroll events do not cancel the explicit click.
  view.scroller.scrollTop = 450; view.scroller.dispatchEvent(new Event("scroll"));
  await settle(); expect(highlighted(view.host)).toBeUndefined();
  await vi.advanceTimersByTimeAsync(250); await settle();
  expect(highlighted(view.host)).toBe("item-s1");
  expect(view.host.querySelector("#item-s1")!.getBoundingClientRect().top).toBe(0);
});
