import { describe, expect, it } from "vitest";
import { frameBudget, passes, recordFrames, stats, worstRun } from "./pacing";

const T120 = frameBudget(120);

// Deterministic jitter so a failure reproduces exactly.
function jitter(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 0x100000000 - 0.5;
  };
}

function series(count: number, interval: number, spread = 0, seed = 1): number[] {
  const next = jitter(seed);
  return Array.from({ length: count }, () => interval + spread * next());
}

describe("frameBudget", () => {
  it("is 1000 / Hz", () => {
    expect(frameBudget(120)).toBeCloseTo(8.3333, 4);
    expect(frameBudget(60)).toBeCloseTo(16.6667, 4);
  });
});

describe("stats", () => {
  it("summarises a perfect 120 Hz series", () => {
    const s = stats(series(1200, T120), T120);
    expect(s.count).toBe(1200);
    expect(s.mean).toBeCloseTo(T120, 6);
    expect(s.median).toBeCloseTo(T120, 6);
    expect(s.p95).toBeCloseTo(T120, 6);
    expect(s.p99).toBeCloseTo(T120, 6);
    expect(s.over1_5T).toBe(0);
    expect(s.over2_5T).toBe(0);
    expect(s.band60).toBe(0);
  });

  it("uses the middle pair for an even median and nearest rank for percentiles", () => {
    const s = stats([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 5);
    expect(s.median).toBe(5.5);
    expect(s.p95).toBe(10);
    expect(stats([...Array(100).keys()].map((i) => i + 1), 5).p95).toBe(95);
    expect(stats([...Array(100).keys()].map((i) => i + 1), 5).p99).toBe(99);
  });

  it("counts intervals strictly above 1.5 T and 2.5 T and the inclusive 15.0-18.5 ms band", () => {
    const s = stats([10, 1.5 * 10, 15.0001, 25, 25.0001, 18.5, 18.5001, 14.9999], 10);
    expect(s.over1_5T).toBeCloseTo(5 / 8, 9); // 15.0001, 25, 25.0001, 18.5, 18.5001
    expect(s.over2_5T).toBeCloseTo(1 / 8, 9); // 25.0001
    expect(s.band60).toBeCloseTo(3 / 8, 9); // 15 (=1.5T), 15.0001, 18.5
  });

  it("returns NaN figures for an empty series", () => {
    const s = stats([], T120);
    expect(s.count).toBe(0);
    expect(Number.isNaN(s.mean)).toBe(true);
    expect(passes(s, T120).all).toBe(false);
  });
});

describe("passes", () => {
  it("passes a perfect 120 Hz series", () => {
    const verdict = passes(stats(series(1200, T120), T120), T120);
    expect(verdict).toEqual({ mean: true, median: true, within1_5T: true, over2_5T: true, band60: true, all: true });
  });

  it("passes a 120 Hz series with sub-millisecond jitter", () => {
    const verdict = passes(stats(series(1200, T120, 0.8, 7), T120), T120);
    expect(verdict.all).toBe(true);
  });

  it("fails a 60 Hz series on a 120 Hz display on the mean and the 60 Hz band", () => {
    const verdict = passes(stats(series(600, frameBudget(60)), T120), T120);
    expect(verdict.mean).toBe(false);
    expect(verdict.band60).toBe(false);
    expect(verdict.all).toBe(false);
  });

  it("fails 0.3% of intervals above 2.5 T and only that criterion", () => {
    const deltas = series(997, T120).concat([3 * T120, 3 * T120, 3 * T120]);
    const verdict = passes(stats(deltas, T120), T120);
    expect(verdict).toEqual({ mean: true, median: true, within1_5T: true, over2_5T: false, band60: true, all: false });
  });

  it("allows at most 0.2% above 2.5 T", () => {
    const deltas = series(998, T120).concat([3 * T120, 3 * T120]);
    expect(passes(stats(deltas, T120), T120).over2_5T).toBe(true);
  });

  it("fails more than 1% above 1.5 T", () => {
    // 2% of frames are doubled; the extra time also shifts the mean beyond 1%.
    const deltas = series(980, T120).concat(series(20, 2 * T120));
    const verdict = passes(stats(deltas, T120), T120);
    expect(verdict.within1_5T).toBe(false);
    expect(verdict.over2_5T).toBe(true);
  });

  it("checks the median within 0.5 ms and the mean within 1%", () => {
    expect(passes(stats(series(100, T120 + 0.09), T120), T120).mean).toBe(false);
    expect(passes(stats(series(100, T120 + 0.08), T120), T120).mean).toBe(true);
    const skewed = series(51, T120 + 0.6).concat(series(49, T120 - 0.6));
    expect(passes(stats(skewed, T120), T120).median).toBe(false);
  });

  it("leaves the 60 Hz band out when the display itself runs near 60 Hz", () => {
    const T60 = frameBudget(60);
    const verdict = passes(stats(series(600, T60), T60), T60);
    expect(verdict.band60).toBeNull();
    expect(verdict.all).toBe(true);
  });
});

describe("worstRun", () => {
  it("picks the run that fails the most criteria, then the one with more slow frames", () => {
    const good = stats(series(1200, T120, 0.4, 3), T120);
    const slowTail = stats(series(1190, T120).concat(series(10, 2 * T120)), T120);
    const sixty = stats(series(600, frameBudget(60)), T120);
    expect(worstRun([good, slowTail, sixty], T120)).toBe(2);
    expect(worstRun([good, slowTail], T120)).toBe(1);
    expect(worstRun([good], T120)).toBe(0);
  });
});

describe("recordFrames", () => {
  it("returns the intervals between animation frames for the requested duration", async () => {
    let clock = 0;
    const raf = (callback: FrameRequestCallback) => {
      clock += 8;
      queueMicrotask(() => callback(clock));
      return 0;
    };
    const deltas = await recordFrames(100, { raf, now: () => clock });
    expect(deltas.length).toBeGreaterThanOrEqual(11);
    expect(deltas.every((d) => d === 8)).toBe(true);
  });
});
