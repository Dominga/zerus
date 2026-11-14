// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createActivityEnd } from "../ui/sessions/activity-end";
import { DEFAULT_PLAN, runScenario } from "./scenarios";

afterEach(() => { vi.useRealTimers(); document.body.replaceChildren(); });

it("releases the real end-follow gate and traverses the Activity instead of oscillating at its bottom", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance", "requestAnimationFrame", "cancelAnimationFrame"] });
  const element = document.createElement("div"); document.body.append(element);
  let top = 1000;
  const positions: number[] = [];
  Object.defineProperties(element, {
    scrollHeight: { get: () => 1200 }, clientHeight: { get: () => 200 },
    scrollTop: { get: () => top, set: (value: number) => { top = Math.max(0, Math.min(1000, value)); positions.push(top); element.dispatchEvent(new Event("scroll")); } },
  });
  element.scrollTo = ((options: ScrollToOptions) => { element.scrollTop = options.top ?? top; }) as typeof element.scrollTo;
  const end = createActivityEnd(() => ({ top, height: 1200, viewport: 200 }), () => element.scrollTo({ top: 1000, behavior: "instant" }));
  element.addEventListener("scroll", () => end.scrolled());
  element.addEventListener("wheel", () => end.release());
  end.pin();
  try {
    const pending = runScenario("activity-scroll", { scrollActivity: () => element, scrollList: () => null, openSession: async () => {} }, null as never,
      { ...DEFAULT_PLAN, hz: 144, hzSource: "native", warmupMs: 0, runMs: 320, runs: 1 });
    await vi.advanceTimersByTimeAsync(600);
    const result = await pending;
    expect(end.active()).toBe(false);
    expect(Math.max(...positions) - Math.min(...positions)).toBeGreaterThan(200);
    expect(result.runs[0]).toMatchObject({ scroll_travel: { span_px: expect.any(Number) } });
  } finally { end.dispose(); }
});

it("measures the advertised step even when the container's normal writes are smooth", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance", "requestAnimationFrame", "cancelAnimationFrame"] });
  const element = document.createElement("div"); element.style.scrollBehavior = "smooth";
  let top = 400;
  Object.defineProperties(element, {
    scrollHeight: { get: () => 2000 }, clientHeight: { get: () => 200 },
    // Model a smooth write's partial progress, not the method used by the driver.
    scrollTop: { get: () => top, set: (value: number) => { top += (value - top) / 2; } },
  });
  element.scrollTo = ((options: ScrollToOptions) => {
    if (options.behavior === "instant") top = options.top ?? top;
    else element.scrollTop = options.top ?? top;
  }) as typeof element.scrollTo;
  const pending = runScenario("list-scroll", { scrollList: () => element, scrollActivity: () => null, openSession: async () => {} }, null as never,
    { ...DEFAULT_PLAN, hz: 144, hzSource: "manual", warmupMs: 0, runMs: 160, runs: 1 });
  await vi.advanceTimersByTimeAsync(400);
  const result = await pending;
  const run = result.runs[0] as { frames: number; scroll_travel: { total_px: number } };
  expect(run.scroll_travel.total_px / (run.frames + 1)).toBe(16);
});

it("retains the observed evidence and gives no passing verdict when scrolling is ignored", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance", "requestAnimationFrame", "cancelAnimationFrame"] });
  const element = document.createElement("div");
  Object.defineProperties(element, { scrollHeight: { get: () => 1200 }, clientHeight: { get: () => 200 }, scrollTop: { get: () => 400 } });
  element.scrollTo = () => {};
  const pending = runScenario("activity-scroll", { scrollList: () => null, scrollActivity: () => element, openSession: async () => {} }, null as never,
    { ...DEFAULT_PLAN, hz: 144, hzSource: "native", warmupMs: 0, runMs: 160, runs: 1 });
  await vi.advanceTimersByTimeAsync(400);
  const result = await pending;
  expect(result.skipped).toBe("scrolling did not traverse the container");
  expect(result.runs[0]).toMatchObject({ scroll_travel: { span_px: 0, total_px: 0 } });
  expect(result.verdict).toBeUndefined();
});
