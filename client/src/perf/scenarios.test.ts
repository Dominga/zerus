import { describe, expect, it } from "vitest";
import { SCENARIOS, summarize, toJson, type PerfReport } from "./scenarios";

describe("summarize", () => {
  it("gives the count, mean, median, nearest-rank percentiles and max", () => {
    const s = summarize([5, 1, 4, 2, 3]);
    expect(s).toEqual({ count: 5, mean: 3, median: 3, p95: 5, p99: 5, max: 5 });
    expect(summarize([...Array(100).keys()].map((i) => i + 1)).p95).toBe(95);
  });

  it("is NaN for no samples", () => {
    const s = summarize([]);
    expect(s.count).toBe(0);
    expect(Number.isNaN(s.p95)).toBe(true);
  });
});

describe("report JSON", () => {
  it("writes NaN and infinities as null", () => {
    const report = { schema: "zerus-perf-v1", scenarios: [{ worst: { mean: NaN, max: Infinity, p95: 1.5 } }] } as unknown as PerfReport;
    expect(JSON.parse(toJson(report)).scenarios[0].worst).toEqual({ mean: null, max: null, p95: 1.5 });
  });

  it("covers every scenario of the spec", () => {
    expect(SCENARIOS.map((s) => s.id)).toEqual([
      "raf-light",
      "transform-animation",
      "left-animation",
      "list-scroll",
      "activity-scroll",
      "session-open",
      "input-latency",
    ]);
  });
});
