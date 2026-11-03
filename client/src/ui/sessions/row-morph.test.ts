import { describe, expect, it } from "vitest";

// zerus-bkbb: the compact-row morph never lets an accelerated animation end by
// itself. Vblank-paced WebKitGTK at 144 Hz showed one frame of the pre-morph
// (expanded) layout when the rows' transform transition finished on its own;
// SessionList's endMorph removes it from the main thread instead. The painted
// check is client/scripts/row-collapse-smoke.html (menu-smoke.py --paint-column).

// Read from disk: Vitest hands stylesheet imports over empty, even with `?raw`.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const css = fs.readFileSync(new URL("../styles/sessions.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const list = fs.readFileSync(new URL("./SessionList.tsx", import.meta.url), "utf8");

describe("compact-row morph", () => {
  const rule = /\.sp-scroll\.morphing \.vrow\s*\{\s*transition:\s*transform\s+(\d+)ms\s+linear\(([^)]*)\)\s*;?\s*\}/.exec(css);
  const morph = Number(/const MORPH = \{ duration: (\d+),/.exec(list)?.[1]);
  const end = /morphTimer = window\.setTimeout\(endMorph, MORPH\.duration \+ (\d+)\)/.exec(list);

  it("glides the rows within the morph and holds the final transform past endMorph", () => {
    expect(rule, "the rows' transition must be a held linear() curve").not.toBeNull();
    expect(end, "the morph must end through endMorph").not.toBeNull();
    const duration = Number(rule![1]), endAt = morph + Number(end![1]);
    const points = rule![2].split(",").map((p) => p.trim().split(/\s+/)).map(([v, at]) => ({ v: Number(v), at: at === undefined ? undefined : Number(at.replace("%", "")) }));
    const done = points.find((p) => p.v === 1 && p.at !== undefined)!;
    expect(points[0].v).toBe(0);
    expect(points.every((p, i) => i === 0 || p.v >= points[i - 1].v), "the glide never moves back").toBe(true);
    // The visible motion ends with the 220 ms morph...
    expect((done.at! / 100) * duration).toBeLessThanOrEqual(morph);
    // ...and the transition still holds when endMorph removes it, with margin
    // for a late timer, so it never finishes on its own.
    expect(duration).toBeGreaterThanOrEqual(endAt * 3);
  });

  it("holds every morph animation and cancels them together", () => {
    expect(list).toMatch(/const HELD = \{ \.\.\.MORPH, fill: "forwards" \}/);
    expect(list).not.toMatch(/\.animate\([^)]*\], MORPH\)/);
    expect(list).toMatch(/function endMorph\(\) \{\s*for \(const a of held\) a\.cancel\(\);/);
    expect(list).toMatch(/fill: "both" \}\)\)\)/);
  });
});
