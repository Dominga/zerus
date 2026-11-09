import { expect, it } from "vitest";

const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
  readdirSync(path: URL): string[];
};
const sheet = (name: string) => fs.readFileSync(new URL(name, import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const css = sheet("./sessions.css");
const declarations = (selector: string, from = css) => {
  const rule = [...from.matchAll(/([^{}]+)\{([^{}]*)\}/g)].find((m) => m[1].trim() === selector);
  if (!rule) throw Error(`Missing rule: ${selector}`);
  return Object.fromEntries(rule[2].split(";").filter((s) => s.includes(":")).map((s) => s.split(":").map((v) => v.trim())));
};
const skins = () => fs.readdirSync(new URL("../../skin/skins/", import.meta.url)).filter((name) => name.endsWith(".json"));

// Task #457: session cards get a subtle card surface and frame from the skin's
// tokens, a project is a frame in its own colour, and a card's state badge
// never leaves the card.
it("draws every session card from the skin's card and card-border tokens (task #457)", () => {
  const card = declarations(".scard");
  expect(card.background).toBe("var(--z-color-card)");
  expect(card.border).toBe("1px solid var(--z-color-card-border)");
  // No hard-coded colour in the card's surface or frame.
  expect(card.background + card.border).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(/i);
  // Hover and selection stay distinguishable through their own tokens.
  expect(declarations(".rowhit:hover .scard").background).toBe("var(--z-color-card-hover)");
  const selected = declarations(".scard.selected, .rowhit:hover .scard.selected");
  expect(selected.background).toBe("var(--z-color-card-selected)");
  expect(selected["border-color"]).toContain("var(--z-color-card-selected-border)");
});

it("frames a project in its own colour around its header row, with the folds and the pointer safe (task #457)", () => {
  const frame = declarations(".group-frame");
  expect(frame.position).toBe("absolute");
  // Task 468: the folder's colour at half, never a solid line (the neutral fallback included).
  expect(frame.border).toBe("1px solid color-mix(in srgb, var(--frame) 50%, transparent)");
  expect(frame["border-radius"]).toBe("8px");
  // The frame's layer must never take the pointer from a card or header.
  expect(frame["pointer-events"]).toBe("none");
});

it("keeps a card's status badge inside the card, cut by an ellipsis, never by the card (task #457)", () => {
  const badge = declarations(".sbadge");
  expect(badge["max-width"]).toBe("100%");
  expect(badge["white-space"]).toBe("nowrap");
  expect(badge.overflow).toBe("hidden");
  const label = declarations(".sbadge .sb-label");
  expect(label["text-overflow"]).toBe("ellipsis");
  expect(label["min-width"]).toBe("0");
  expect(label.overflow).toBe("hidden");
  // The badge keeps the width its caption needs; the title gives way first.
  expect(declarations(".sc-l1 > .sbadge")["flex"]).toBe("0 0 auto");
  expect(declarations(".scard.compact .sbadge")["flex"]).toBe("0 0 auto");
  // The old rule let a badge collapse to a clipped sliver.
  expect(css).not.toContain("flex-shrink: .001");
  // The cooling badge keeps its bar inside the frame, clear of the caption.
  const cooling = declarations(".sbadge.k-cooling");
  expect(cooling["padding-right"]).toBe("31px");
  expect(declarations(".sbadge .sb-heat").right).toBe("8px");
});

it("keeps a card's surface a couple of tones apart from the list's pane in every skin, light and dark (task #457)", () => {
  const dir = new URL("../../skin/skins/", import.meta.url);
  const missing: string[] = [];
  for (const name of skins()) {
    const skin = JSON.parse(fs.readFileSync(new URL(`./${name}`, dir), "utf8")) as { appearance: string; tokens: { color: Record<string, string> } };
    const color = skin.tokens.color;
    if (color.card === color.pane) missing.push(`${name}: card equals pane`);
    if (color["card-border"] === color.card) missing.push(`${name}: border equals card`);
    if (!/^#[0-9a-f]{6}$/i.test(color.card)) missing.push(`${name}: card is not a colour`);
    if (!/^#[0-9a-f]{6}$/i.test(color["card-border"])) missing.push(`${name}: border is not a colour`);
  }
  expect(missing).toEqual([]);
});

