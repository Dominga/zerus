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
  const loadOlder = vi.fn();
  const source: ActivitySource = {
    target: () => ({ id: target() }), events: rows, loaded, loadOlder,
    summary: () => undefined, loadingOlder: () => false, olderDone: () => false, ended: () => false, error: () => undefined,
  };
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(ActivityView, { source }), host);
  return { scroller: host.querySelector<HTMLElement>(".act-scroll")!, source, target, setTarget, setRows, setLoaded, loadOlder };
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
