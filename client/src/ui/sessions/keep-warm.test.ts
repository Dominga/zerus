import { describe, expect, it } from "vitest";
import { ALWAYS, choiceAt, durationLabel, durationMark, headerKeepWarm, keepWarmOf, mergeKeepWarm, positionOf, remainingText, statusLine, tooltipLines } from "./keep-warm";
import type { KeepWarmState } from "../../types";

const NOW = 1_794_000_000;
const state = (extra: Partial<KeepWarmState> = {}): KeepWarmState => ({
  enabled: true, until: NOW + 3 * 3600, revision: 2, state: "waiting", next_at: null, reason: "", turn_active: false, ...extra,
});

describe("keep cache warm presentation", () => {
  it("accepts only a well-formed host state", () => {
    expect(keepWarmOf({ enabled: true, until: null, revision: 1, state: "waiting", next_at: null, reason: "", turn_active: false })?.until).toBeNull();
    for (const bad of [undefined, null, "on", { enabled: "yes", revision: 1 }, { enabled: true }, { enabled: true, revision: -1 }])
      expect(keepWarmOf(bad)).toBeUndefined();
  });

  it("keeps the newest revision and the details only the Details read reports", () => {
    const listing = state({ revision: 3, enabled: false, until: null, state: "off" });
    const inspected = state({ revision: 2, supported: true, ttl_seconds: 3600, interval_seconds: 2880, reason_code: "" });
    const merged = mergeKeepWarm(listing, inspected, undefined)!;
    expect([merged.revision, merged.enabled, merged.ttl_seconds, merged.interval_seconds]).toEqual([3, false, 3600, 2880]);
    // A confirmed local change wins until the host reports a newer revision.
    const confirmed = state({ revision: 4, enabled: true, until: null });
    expect(mergeKeepWarm(listing, inspected, confirmed)!.until).toBeNull();
    expect(mergeKeepWarm(undefined, undefined, undefined)).toBeUndefined();
  });

  it("keeps the Details read's eligibility reason over the listing's empty one", () => {
    const reason = "DeepSeek keeps its context cache for hours without pings.";
    const merged = mergeKeepWarm(state({ enabled: false, state: "off", reason: "" }), state({ enabled: false, state: "off", supported: false, reason_code: "deepseek", reason }))!;
    expect([merged.supported, merged.reason]).toEqual([false, reason]);
  });

  it("maps 1 to 7 hours and Always onto the slider", () => {
    expect(positionOf(state({ until: null }), NOW)).toBe(8);
    expect(positionOf(state({ until: NOW + 2.5 * 3600 }), NOW)).toBe(3);
    expect(positionOf(state({ until: NOW + 60 }), NOW)).toBe(1);
    expect(positionOf(state({ until: NOW + 20 * 3600 }), NOW)).toBe(7);
    expect(positionOf(undefined, NOW)).toBe(2);
    expect([durationLabel(1), durationLabel(7), durationLabel(8)]).toEqual(["1 h", "7 h", "Always"]);
  });

  it("keeps a fresh choice at its hours with a clock read up to 30 s old or a host clock a few minutes ahead (zerus-ua4bi)", () => {
    for (let hours = 1; hours <= 7; hours++) {
      expect(positionOf(state({ until: NOW + hours * 3600 }), NOW - 30)).toBe(hours);
      expect(positionOf(state({ until: NOW + hours * 3600 + 120 }), NOW)).toBe(hours);
    }
    // Past those first minutes the hours left still round up.
    expect(positionOf(state({ until: NOW + 3600 + 20 * 60 }), NOW)).toBe(2);
  });

  it("offers exactly the choices the host accepts: 1 to 7 hours or Always (zerus-ua4bi)", () => {
    const positions = Array.from({ length: ALWAYS }, (_, i) => i + 1);
    expect(positions.map(choiceAt)).toEqual([1, 2, 3, 4, 5, 6, 7].map((hours) => ({ kind: "hours", hours })).concat([{ kind: "always" } as never]));
  });

  it("marks Always with an infinity sign, keeping Always as its name (zerus-ua4bi)", () => {
    expect([durationMark(1), durationMark(7), durationMark(ALWAYS)]).toEqual(["1 h", "7 h", "\u221e"]);
    expect(durationLabel(ALWAYS)).toBe("Always");
  });

  it("sums the header mark up: nothing while off, an infinity sign for Always, else the time left (zerus-yacns)", () => {
    expect(headerKeepWarm(undefined, NOW, 0)).toBeUndefined();
    expect(headerKeepWarm(state({ enabled: false, until: null, state: "off" }), NOW, 0)).toBeUndefined();
    const always = headerKeepWarm(state({ until: null }), NOW, 0)!;
    expect([always.mark, always.label]).toEqual(["\u221e", "Keep cache warm: Always"]);
    const timed = headerKeepWarm(state({ until: NOW + 2 * 3600 + 14 * 60 + 5, next_at: NOW + 600 }), NOW, 0)!;
    expect([timed.mark, timed.label]).toEqual(["2 h 14 m", "Keep cache warm: 2 h 14 m left"]);
    expect(timed.tooltip.split("\n")).toEqual(["Keep cache warm: 2 h 14 m left", "Next ping in 10 m", "Each ping reads the whole cached context from this account's usage.", "Click to change it in Details."]);
    expect(headerKeepWarm(state({ until: NOW + 3 * 3600 }), NOW, 0)!.mark).toBe("3 h");
    expect(headerKeepWarm(state({ until: NOW + 45 * 60 + 30 }), NOW, 0)!.mark).toBe("45 m");
    expect(headerKeepWarm(state({ until: NOW + 30 }), NOW, 0)!.mark).toBe("<1 m");
  });

  it("says how long is left without dot separators", () => {
    expect(remainingText(state({ until: NOW + 2 * 3600 + 14 * 60 + 5 }), NOW)).toBe("2 h 14 m left");
    expect(remainingText(state({ until: NOW + 45 * 60 }), NOW)).toBe("45 m left");
    expect(remainingText(state({ until: NOW + 20 }), NOW)).toBe("Less than a minute left");
    expect(remainingText(state({ until: null }), NOW)).toBe("Always");
    expect(remainingText(state({ enabled: false }), NOW)).toBe("");
  });

  it("explains the state, the reason and ineligible agents in one line", () => {
    expect(statusLine(state({ supported: false, reason_code: "claude_short_ttl", reason: "Claude uses a 5-minute cache on this account. Keeping it warm would cost about as much as rebuilding it." }), NOW))
      .toBe("Claude uses a 5-minute cache on this account. Keeping it warm would cost about as much as rebuilding it.");
    expect(statusLine(state({ state: "cold", reason: "The cache went cold. Keeping it warm resumes after your next message." }), NOW))
      .toBe("The cache went cold. Keeping it warm resumes after your next message.");
    expect(statusLine(state({ next_at: NOW + 600 }), NOW)).toBe("Next ping in 10 m");
    expect(statusLine(state({ turn_active: true }), NOW)).toBe("Warming the cache now");
    expect(statusLine(state({ enabled: false, state: "off" }), NOW)).toBe("");
  });

  it("states the interval, pings per hour and what each ping costs", () => {
    const lines = tooltipLines(state({ ttl_seconds: 3600, interval_seconds: 2880, ttl_source: "native_usage" }), 331_000);
    expect(lines).toEqual([
      "The cache lasts about 1 h, as the agent reported.",
      "Pings every 48 m while idle, about 1.3 an hour.",
      "Each ping reads the whole cached context (about 331k tokens) from this account's usage.",
    ]);
    expect(tooltipLines(state({ ttl_seconds: 1800, interval_seconds: 1440, ttl_source: "provider_default" }), 0)[0])
      .toBe("The cache lasts about 30 m, the provider's documented default.");
    expect(tooltipLines(state({}), 0)).toEqual(["Each ping reads the whole cached context from this account's usage."]);
  });
});
