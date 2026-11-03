import { describe, expect, it } from "vitest";

// zerus-attm: the session panel's slide (dock to strip, peek, strip to docked)
// never leaves its active phase by itself. On vblank-paced WebKitGTK at 144 Hz
// a slide that finished by itself presented one frame of its pre-slide layer:
// a closed panel flashed back open, an opened one jumped back. Like the row
// morph (row-morph.test.ts) it holds its end value and is cancelled together
// with `done` in one task. The painted check is
// client/scripts/panel-slide-smoke.html (menu-smoke.py --paint-row).

// Read from disk, as row-morph.test.ts does.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const list = fs.readFileSync(new URL("./SessionList.tsx", import.meta.url), "utf8");
const block = list.slice(list.indexOf("let sliding:"), list.indexOf("export function SessionList"));

describe("panel slide", () => {
  it("holds its end through the shared hold() keyframes, never ending by itself", () => {
    const open = Number(/const OPEN_MS = (\d+);/.exec(list)?.[1]);
    const active = Number(/const HOLD = (\d+);/.exec(list)?.[1]);
    const endAfter = Number(/\}, OPEN_MS \+ (\d+)\);/.exec(block)?.[1]);
    expect(open).toBe(220);
    // Still active, with margin for a late timer, when the timer cancels it.
    expect(active).toBeGreaterThanOrEqual(3 * (open + endAfter));
    expect(block).not.toMatch(/\.animate\(/);
    expect(block).not.toMatch(/onfinish/);
    expect(block).toContain("opening ? hold(el, hidden, shown, 0, OPEN_MS, OUT_CUBIC) : hold(el, shown, hidden, 0, OPEN_MS, OUT_CUBIC)");
  });

  it("ends in one task with done, and a new slide replaces the one in flight", () => {
    expect(block).toMatch(/if \(sliding\?\.animation !== animation\) return;\s*sliding = undefined;\s*animation\.cancel\(\);\s*done\?\.\(\);/);
    expect(block).toMatch(/if \(sliding\) \{\s*clearTimeout\(sliding\.timer\);\s*sliding\.animation\.cancel\(\);\s*sliding = undefined;\s*\}/);
    // Reduced motion and a missing panel still settle at once.
    expect(block).toMatch(/if \(!el \|\| motionReduced\(\)\) \{ done\?\.\(\); return; \}/);
  });

  it("never runs a queued opening that a newer slide request superseded", () => {
    // #1158 review: a close in the same turn (or 5 ms later) used to be
    // cancelled by the opening queued before it, leaving the panel open.
    expect(block).toMatch(/function slide\(el: HTMLElement \| undefined, width: number, opening: boolean, done\?: \(\) => void\) \{\s*slideRequest\+\+;/);
    expect(block).toMatch(/const request = \+\+slideRequest;\s*requestAnimationFrame\(\(\) => \{ if \(request === slideRequest\) slide\(el\(\), width\(\), true\); \}\);/);
    // Every queued opening goes through it.
    expect(list).not.toMatch(/requestAnimationFrame\(\(\) => slide\(/);
    expect(list.match(/slideOpenLater\(\(\) => panel, sessionsWidth\)/g)).toHaveLength(2);
  });
});
