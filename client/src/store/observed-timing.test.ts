import { describe, expect, it } from "vitest";
import { observedFollower, type ObservedFollow } from "./observed-timing";
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };
describe("observed page lease", () => {
  it("retires local eligibility immediately and closes only its own serialized lease", async () => {
    const calls: [number, boolean, number | null][] = []; const waiting: ((n: number) => void)[] = [];
    const port: ObservedFollow = (_team, phase, on, expected) => { calls.push([phase, on, expected]); return new Promise((resolve) => waiting.push(resolve)); };
    const follower = observedFollower("fixture-team", port);
    follower.select(1, true); follower.select(2, true);
    expect(follower.epoch()).toBeNull(); expect(calls).toEqual([[1, true, null]]);
    waiting.shift()!(11); await flush(); expect(calls).toEqual([[1, true, null], [2, true, null]]);
    follower.close(); waiting.shift()!(12); await flush(); expect(calls.at(-1)).toEqual([2, false, 12]);
    waiting.shift()!(13); await flush(); expect(follower.epoch()).toBeNull();
  });
  it("keeps an unsuccessful read unknown without another call or a guessed lease", async () => {
    let calls = 0; const follower = observedFollower("fixture-team", async () => { calls++; throw new Error("source unavailable"); });
    follower.select(1, true); await flush(); expect(calls).toBe(1); expect(follower.epoch()).toBeNull(); expect(follower.error()).toContain("source unavailable");
    follower.close(); await flush(); expect(calls).toBe(1);
  });
});
