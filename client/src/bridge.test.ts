import { describe, expect, it } from "vitest";
import { attentionMarks, pendingReadMarks, pump, queueReadMarks, updateAttentionMarks, type PumpHost, type Source } from "./bridge";
import { emptyBatch, type Batch } from "./types";

/** A source whose `next` answers are resolved by the test. */
function source() {
  const waiting: ((b: Batch) => void)[] = [];
  const s: Source = {
    next: () => new Promise<Batch>((resolve) => waiting.push(resolve)),
    follow: async () => {},
    unfollow: async () => {},
  };
  return { s, calls: () => waiting.length, answer: (b: Batch) => waiting.shift()!(b) };
}

/** Frames, timers and visibility under the test's control. */
function host(hidden = false) {
  let frames: (() => void)[] = [];
  let timers: { callback: () => void; ms: number; live: boolean }[] = [];
  const listeners: (() => void)[] = [];
  const h: PumpHost = {
    frame: (callback) => { frames.push(callback); },
    later: (callback, ms) => { const t = { callback, ms, live: true }; timers.push(t); return () => { t.live = false; }; },
    visibility: { hidden: () => hidden, onChange: (l) => { listeners.push(l); return () => {}; } },
  };
  return {
    h,
    frames: () => frames.length,
    runFrames: () => { const now = frames; frames = []; now.forEach((f) => f()); },
    liveTimers: () => timers.filter((t) => t.live).length,
    runTimers: () => { const now = timers.filter((t) => t.live); timers = []; now.forEach((t) => { t.live = false; t.callback(); }); },
    setHidden: (value: boolean) => { hidden = value; listeners.forEach((l) => l()); },
  };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const batch = (key: string): Batch => ({ ...emptyBatch(), removes: [{ computer: "", key, reason: null }] });

describe("pump", () => {
  it("keeps machine cards and counts a batch of only cards as content", async () => {
    const src = source();
    const h = host();
    const got: Batch[] = [];
    pump(src.s, (b) => got.push(b), h.h);
    src.answer({ machines: [{ computer: "", machine: { host: "workstation" } }] } as unknown as Batch);
    await settle();
    h.runFrames();
    expect(got).toHaveLength(1);
    expect(got[0].machines).toHaveLength(1);
    expect(got[0].upserts).toEqual([]);
  });

  it("applies a batch in the next frame and asks again only after it", async () => {
    const src = source();
    const page = host();
    const applied: Batch[] = [];
    pump(src.s, (b) => applied.push(b), page.h);
    expect(src.calls()).toBe(1);
    src.answer(batch("a"));
    await settle();
    expect(page.frames()).toBe(1);
    expect(src.calls()).toBe(0);
    page.runFrames();
    expect(applied.map((b) => b.removes[0].key)).toEqual(["a"]);
    expect(page.liveTimers()).toBe(0); // the 250 ms fallback is cancelled
    expect(src.calls()).toBe(1);
  });

  it("asks again at once, without a frame, after an empty answer", async () => {
    const src = source();
    const page = host();
    pump(src.s, () => {}, page.h);
    src.answer(emptyBatch());
    await settle();
    expect(page.frames()).toBe(0);
    expect(src.calls()).toBe(1);
  });

  it("asks for nothing and schedules no frame while the window is hidden", async () => {
    const src = source();
    const page = host(true);
    const applied: Batch[] = [];
    pump(src.s, (b) => applied.push(b), page.h);
    await settle();
    expect(src.calls()).toBe(0);
    expect(page.frames()).toBe(0);
    expect(page.liveTimers()).toBe(0);
    page.setHidden(false);
    expect(src.calls()).toBe(1);
  });

  it("holds a batch that arrives while hidden and applies it in the first frame after", async () => {
    const src = source();
    const page = host();
    const applied: Batch[] = [];
    pump(src.s, (b) => applied.push(b), page.h);
    page.setHidden(true); // the call is already in flight
    src.answer(batch("held"));
    await settle();
    expect(page.frames()).toBe(0);
    expect(page.liveTimers()).toBe(0);
    expect(src.calls()).toBe(0);
    expect(applied).toEqual([]);
    page.setHidden(false);
    expect(page.frames()).toBe(1);
    page.runFrames();
    expect(applied.map((b) => b.removes[0].key)).toEqual(["held"]);
    expect(src.calls()).toBe(1);
  });

  it("holds a batch whose frame did not come before the window was hidden", async () => {
    const src = source();
    const page = host();
    const applied: Batch[] = [];
    pump(src.s, (b) => applied.push(b), page.h);
    src.answer(batch("late"));
    await settle();
    page.setHidden(true); // rAF stops; the 250 ms fallback fires instead
    page.runTimers();
    expect(applied).toEqual([]);
    expect(src.calls()).toBe(0);
    page.setHidden(false);
    page.runFrames(); // the stale frame callback of the first attempt is ignored
    page.runFrames();
    expect(applied.map((b) => b.removes[0].key)).toEqual(["late"]);
    expect(src.calls()).toBe(1);
  });

  it("an occluded window without frames still applies within the fallback", async () => {
    const src = source();
    const page = host();
    const applied: Batch[] = [];
    pump(src.s, (b) => applied.push(b), page.h);
    src.answer(batch("occluded"));
    await settle();
    page.runTimers();
    expect(applied.length).toBe(1);
    page.runFrames(); // the frame that finally comes does not apply it twice
    expect(applied.length).toBe(1);
    expect(src.calls()).toBe(1);
  });
});

describe("marks outside the app", () => {
  it("falls back to no marks, ignores updates and has no pending list, so the store keeps its own", async () => {
    expect(await attentionMarks()).toEqual({ marks: {}, replies: {}, order: { marks: [], replies: [] } });
    await expect(updateAttentionMarks({ k: { review: true, acknowledged: "" } }, { k: "r1" })).resolves.toBeUndefined();
    expect(await queueReadMarks([{ computer: "", name: "claude/p/a", run_id: "r", conversation_id: "c", reply_id: "x", attention_signature: null }])).toBeUndefined();
    expect(await pendingReadMarks()).toBeUndefined();
  });
});
