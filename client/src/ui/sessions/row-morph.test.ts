import { describe, expect, it } from "vitest";

// zerus-bkbb: no compact-row morph animation leaves its active phase before
// endMorph cancels it. On vblank-paced WebKitGTK at 144 Hz an accelerated
// animation that finished by itself (a CSS transition, or WAAPI filling
// forwards after 220 ms) presented one frame of the pre-morph, expanded layout.
// Offset keyframes with a cubic-bezier easing keep to the Safari 16 / macOS
// 14.0 WKWebView floor; CSS linear() would need Safari 17.2. The painted check
// is client/scripts/row-collapse-smoke.html (menu-smoke.py --paint-column).

// Read from disk: Vitest hands stylesheet imports over empty, even with `?raw`.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const css = fs.readFileSync(new URL("../styles/sessions.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const list = fs.readFileSync(new URL("./SessionList.tsx", import.meta.url), "utf8");
const morph = list.slice(list.indexOf("const MORPH ="), list.indexOf("type ClearRead"));

describe("compact-row morph", () => {
  it("has no CSS row transition and no linear() easing", () => {
    expect(css).not.toMatch(/\.vrow[^{]*\{[^}]*transition/);
    expect(css).not.toMatch(/linear\(/);
  });

  it("keeps every animation active past endMorph and cancels them together", () => {
    const duration = Number(/const MORPH = \{ duration: (\d+), easing: "cubic-bezier\([^"]+\)" \}/.exec(morph)?.[1]);
    const active = Number(/const HOLD = (\d+);/.exec(morph)?.[1]);
    const endAfter = Number(/morphTimer = window\.setTimeout\(endMorph, MORPH\.duration \+ (\d+)\)/.exec(morph)?.[1]);
    expect(duration).toBe(220);
    // Still active, with margin for a late timer, when endMorph cancels it.
    expect(active).toBeGreaterThanOrEqual(3 * (duration + endAfter));
    // The single animation call: offset keyframes over the whole active time.
    expect(morph.match(/\.animate\(/g)).toHaveLength(1);
    expect(morph).toMatch(/el\.animate\(\[\.\.\.head, \{ \.\.\.to, offset: \(start \+ span\) \/ HOLD \}, \{ \.\.\.to, offset: 1 \}\], \{ duration: HOLD, fill: "both" \}\)/);
    // Every morph motion uses it and ends within the 220 ms morph.
    for (const call of ["glides.push(hold(el, { translate: `0 ${dy}px` }, { translate: \"0 0\" }))",
      "held.push(hold(next, { height: `${from}px` }, { height: `${to}px` }))",
      "held.push(hold(g, { opacity: 1 }, { opacity: 0 }, 0, 120, \"ease-out\"))",
      "held.push(hold(n, { opacity: 0 }, { opacity: 1 }, 60, 160, \"ease-out\"))"]) expect(morph).toContain(call);
    expect(morph).toMatch(/held\.push\(hold\(el, \{ transform: `translate\(/);
    expect(morph).toMatch(/function endMorph\(\) \{\s*for \(const a of \[\.\.\.held, \.\.\.glides\]\) a\.cancel\(\);\s*for \(const g of fading\) g\.remove\(\);/);
  });
});
