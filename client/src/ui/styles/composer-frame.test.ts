import { describe, expect, it } from "vitest";

// Task 453 (owner, screenshot of package 168: "Я кому говорил убрать эту
// границу?"): the composer is one frame. The field inside it draws no frame of
// its own — no border, no outline, focused or not — and the composer's own
// border shows the focus. The rule must be unlayered: the base layer's
// `:focus-visible` ring is what drew the second, inner frame, and a
// component-layer `outline: none` loses to it whenever the document's cascade
// layers arrive in the other order (development CSS is injected per module, so
// a component stylesheet may open the components layer before base.css declares
// reset, base, components).

// Read from disk, as popups.test.ts does: the specifier is built so TypeScript
// (no Node types here) leaves it alone.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readdirSync(path: URL): string[];
  readFileSync(path: URL, encoding: "utf8"): string;
};
const here = new URL("./", import.meta.url);
const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, "");
const sheets = fs.readdirSync(here).filter((file) => file.endsWith(".css"))
  .map((file) => ({ file, css: strip(fs.readFileSync(new URL(file, here), "utf8")) }));

/** Leaf rules of a stylesheet, with the at-rules that enclose them. */
function rules(css: string): { prelude: string; body: string; inside: string[] }[] {
  const out: { prelude: string; body: string; inside: string[] }[] = [];
  const stack: { prelude: string; start: number; nested: boolean }[] = [];
  let last = 0;
  for (let i = 0; i < css.length; i++) {
    if (css[i] === "{") {
      if (stack.length) stack[stack.length - 1].nested = true;
      stack.push({ prelude: css.slice(last, i).trim(), start: i + 1, nested: false });
      last = i + 1;
    } else if (css[i] === "}") {
      const open = stack.pop()!;
      if (!open.nested) out.push({ prelude: open.prelude, body: css.slice(open.start, i).trim(), inside: stack.map((frame) => frame.prelude) });
      last = i + 1;
    } else if (css[i] === ";") last = i + 1;
  }
  return out;
}

const all = sheets.flatMap((sheet) => rules(sheet.css).map((rule) => ({ ...rule, file: sheet.file })));
const declares = (body: string, property: string) => new RegExp(`(^|;)\\s*${property}\\s*:\\s*([^;]+)`, "m").exec(body)?.[2].trim();
const selector = (rule: { prelude: string }) => rule.prelude.replace(/\s+/g, " ");
/** A value that draws nothing: 0, none, 0 solid transparent... */
const draws = (value: string | undefined) => !!value && !/^(0|none|0\s+\w+\s+transparent)\b/.test(value);
const layered = (inside: string[]) => inside.some((frame) => frame.startsWith("@layer"));

/** One selector part that applies to the focused field inside the composer:
 *  the composer's and the field's own classes, only the focus pseudo-classes,
 *  or a bare `:focus-visible` ring, which matches every element. */
function matchesFocusedField(part: string) {
  if (/[>+~]|:not\(|\[/.test(part)) return false;
  const compounds = part.trim().split(/\s+/);
  let field = false;
  for (const compound of compounds) {
    const clean = compound.replace(/::[\w-]+/g, "");
    const classes = [...clean.matchAll(/\.([\w-]+)/g)].map((match) => match[1]);
    const pseudos = [...clean.matchAll(/:([\w-]+)/g)].map((match) => match[1]);
    if (!pseudos.every((name) => name === "focus" || name === "focus-visible")) return false;
    if (classes.some((name) => name !== "cmp-editor" && name !== "composer")) return false;
    if (classes.includes("cmp-editor")) field = true;
  }
  return field || (compounds.length === 1 && compounds[0].startsWith(":"));
}

const weight = (part: string) => [...part.replace(/::[\w-]+/g, "").matchAll(/\.[\w-]+|:[\w-]+/g)].length;
const layerOf = (inside: string[]) => inside.find((frame) => frame.startsWith("@layer"))?.slice("@layer".length).trim() ?? "";

/** The winning declared value of `property` for the focused field under the
 *  given layer order: the layer's rank first (unlayered is last), then the
 *  selector's weight, then source order. */
function winning(property: string, order: string[]) {
  let best: { rank: number; weight: number; value: string } | undefined;
  for (const rule of all) {
    const part = rule.prelude.split(",").map((text) => text.trim()).find(matchesFocusedField);
    const value = part === undefined ? undefined : declares(rule.body, property);
    if (value === undefined) continue;
    const layer = layerOf(rule.inside);
    const rank = layer === "" ? order.length : order.indexOf(layer);
    const node = { rank, weight: weight(part!), value };
    if (!best || node.rank > best.rank || (node.rank === best.rank && node.weight >= best.weight)) best = node;
  }
  return best?.value;
}

describe("the composer's frame (task 453)", () => {
  it("gives the field inside the composer no border and no outline, in any state, in every stylesheet", () => {
    const field = all.filter((rule) => rule.prelude.includes(".cmp-editor"));
    expect(field.length).toBeGreaterThan(0);
    for (const rule of field) {
      expect(draws(declares(rule.body, "border"))).toBe(false);
      expect(draws(declares(rule.body, "outline"))).toBe(false);
    }
  });

  it("suppresses the focus ring unlayered, so it holds whichever order the document's layers arrive in", () => {
    const focused = all.filter((rule) => rule.prelude.includes(".cmp-editor:focus") || rule.prelude.includes(".cmp-editor:focus-visible"));
    expect(focused.length).toBeGreaterThan(0);
    const unlayered = focused.filter((rule) => !layered(rule.inside));
    expect(unlayered.length).toBeGreaterThan(0);
    // One rule carries both states (focus and focus-visible) and draws nothing.
    const suppression = unlayered.find((rule) =>
      rule.prelude.includes(".cmp-editor:focus") && rule.prelude.includes(".cmp-editor:focus-visible")
      && declares(rule.body, "outline") === "none" && declares(rule.body, "border") === "0");
    expect(suppression, "an unlayered rule silencing the field's border and focus outline").toBeDefined();
    // No colour of its own: it cannot differ between skins, light or dark.
    expect(suppression!.body).not.toContain("var(--z-color");
  });

  it("wins the cascade for the focused field in both layer orders, which is the regression", () => {
    // Task 453 and #426/#449: the ring came back because the base layer's
    // `:focus-visible` outranked the field's component-layer `outline: none`
    // when the document's layers arrived in the other order. The winner is
    // computed here from the stylesheets themselves, for the declared order and
    // for the inverted one, so a rule that only wins in one order fails.
    const declared = ["reset", "base", "components", "variants", "utilities"];
    const inverted = ["reset", "components", "base", "variants", "utilities"];
    for (const order of [declared, inverted]) {
      expect(winning("outline", order), `outline wins 'none' with layers ${order.join(",")}`).toBe("none");
      expect(winning("border", order), `border wins '0' with layers ${order.join(",")}`).toBe("0");
    }
  });

  it("draws exactly one frame, the composer's own, and the composer shows the focus", () => {
    // The one frame and its focused colour.
    const frame = all.filter((rule) => selector(rule) === ".composer" && declares(rule.body, "border"));
    expect(frame).toHaveLength(1);
    expect(declares(frame[0].body, "border")).toBe("1px solid var(--z-color-border)");
    const focused = all.find((rule) => selector(rule) === ".composer:focus-within");
    expect(focused, "the composer shows focus on its own frame").toBeDefined();
    expect(declares(focused!.body, "border-color")).toBe("var(--z-color-accent-muted)");
    expect(declares(focused!.body, "box-shadow")).toBe("var(--z-shadow-focus-ring)");
    // Nothing inside the composer draws a frame of its own: the toolbar row, the
    // field and the bottom row all sit inside the one border.
    for (const row of [".cmp-toolbar", ".cmp-editor", ".cmp-actions"]) {
      for (const rule of all.filter((found) => selector(found) === row)) {
        expect(draws(declares(rule.body, "border")), `${row} draws no border`).toBe(false);
        expect(draws(declares(rule.body, "outline")), `${row} draws no outline`).toBe(false);
      }
    }
  });

  it("keeps the height drag on the composer's outer top edge", () => {
    const edge = all.find((rule) => selector(rule) === ".cmp-resize.edge");
    expect(edge, "the drag handle is an overlay, not a row of the composer").toBeDefined();
    expect(edge!.body).toContain("position: absolute");
    expect(declares(edge!.body, "top")).toBe("-3px");
    expect(declares(edge!.body, "left")).toBe("12px");
    expect(declares(edge!.body, "right")).toBe("12px");
  });
});
