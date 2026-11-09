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
