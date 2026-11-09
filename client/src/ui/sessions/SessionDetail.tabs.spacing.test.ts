import { describe, expect, it } from "vitest";

// Task 454 (owner, package 168, D:/Tools/zerus-build/owner-tabs-168.png): the
// session tabs "Activity Terminal Team" ran together in the packaged app although
// #426 gave them padding and a gap. The build links the lazy chunks' stylesheets
// before the entry one (client/dist/index.html: SessionList, ProjectMemoryPanel,
// sections, WslDialogs, FolderBrowser, then index-*.css which carries base.css),
// and the minifier drops a file's `@layer a, b, c;` statement once its blocks
// follow in that order. So the reset layer can be opened *after* components, and
// the reset's `button { padding: 0 }` -- a later layer beats an earlier one
// whatever the specificity -- stripped every tab's padding: the labels sat only
// the row's gap apart.
//
// This test loads the real stylesheets in that packaged order (not one file) and
// computes the cascade for the tab row, so the spacing the owner sees is checked
// here. The fix is in the stylesheet itself (unlayered, as task 449's card), not a
// layer-order declaration in index.html: Tauri hashes an inline <style> into
// style-src, and a hash there makes `'unsafe-inline'` stop applying, which would
// block the skin's runtime-created style element and leave every colour undefined.

const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const read = (url: URL) => fs.readFileSync(url, "utf8");
const here = new URL("./", import.meta.url);
const indexHtml = read(new URL("../../../index.html", here));
const sheet = (name: string) => read(new URL(`../styles/${name}`, here));

// client/dist/index.html's order: the chunks' stylesheets (each opens
// @layer components) first, then the entry stylesheet, which is main.tsx's own
// import order and therefore has base.css before detail.css.
const CHUNK_SHEETS = ["sessions.css", "tasks.css", "inspector.css", "newsession.css"];
const ENTRY_SHEETS = ["base.css", "app.css", "sessions.css", "detail.css", "client.css", "goto.css", "screens.css", "machines.css", "motion.css", "settings.css", "teams.css"];
const PACKAGED_ORDER = [...CHUNK_SHEETS, ...ENTRY_SHEETS];
const sheets: Record<string, string> = Object.fromEntries(PACKAGED_ORDER.map((name) => [name, sheet(name)]));

interface Rule { prelude: string; body: string; parents: string[] }

/** Leaf rules (no nested blocks) of a stylesheet, with their enclosing preludes. */
function rules(css: string): Rule[] {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: Rule[] = [];
  const stack: { prelude: string; start: number; nested: boolean }[] = [];
  let last = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "{") {
      if (stack.length) stack[stack.length - 1].nested = true;
      stack.push({ prelude: text.slice(last, i).trim(), start: i + 1, nested: false });
      last = i + 1;
    } else if (c === "}") {
      const open = stack.pop()!;
      if (!open.nested) out.push({ prelude: open.prelude, body: text.slice(open.start, i), parents: stack.map((s) => s.prelude) });
      last = i + 1;
    } else if (c === ";") {
      last = i + 1;
    }
  }
  return out;
}

/** The layer a rule sits in: the innermost enclosing `@layer name`, or null. */
function layerOf(rule: Rule): string | null {
  for (let i = rule.parents.length - 1; i >= 0; i--) {
    const match = /^@layer\s+([\w-]+)$/.exec(rule.parents[i]);
    if (match) return match[1];
  }
  return null;
}

interface Element { type: string; classes: string[] }
const ROW: Element = { type: "div", classes: ["detail-tabs", "tabs"] };
const TAB: Element = { type: "button", classes: ["tab"] };

/** One compound ("button.tab", ".tab") against one element; null when it uses
 *  something this test does not model (:hover, [attr], *). */
function compoundMatches(compound: string, element: Element): boolean | null {
  if (/[:\[]/.test(compound) || compound === "*") return null;
  let rest = compound;
  let ok = true;
  const type = /^[a-zA-Z][\w-]*/.exec(rest);
  if (type) {
    ok = ok && type[0].toLowerCase() === element.type;
    rest = rest.slice(type[0].length);
  }
  for (const [, name] of rest.matchAll(/\.([\w-]+)/g)) ok = ok && element.classes.includes(name);
  rest = rest.replace(/\.[\w-]+/g, "");
  if (rest !== "") return null;
  return ok;
}

/** Whether a selector of the stylesheet's list matches `button.tab` in the row,
 *  and its specificity (the class count of the simple selectors it names). */
function selectorMatches(selector: string): { matched: boolean; specificity: number } | null {
  const parts = selector.trim().split(/\s+/).filter((part) => !["", ">", "+", "~"].includes(part));
  const last = compoundMatches(parts[parts.length - 1], TAB);
  if (last === null) return null;
  const ancestors = parts.slice(0, -1);
  const row = ancestors.length ? compoundMatches(ancestors[ancestors.length - 1], ROW) : null;
  // A wrapper this test does not model (.detail .work-main …) may match: keep it.
  const matched = last && (row === null || row);
  const specificity = parts.reduce((total, part) => total + (part.match(/\.[\w-]+/g) ?? []).length, 0);
  return { matched, specificity };
}

/** The winning declaration of `prop` for the tab, over the files in the order given. */
function resolve(prop: string, options: { files: string[]; sources?: Record<string, string> }): string | null {
  const sources = options.sources ?? sheets;
  const ranks = new Map<string, number>();
  let nextLayer = 0;
  // The document declares no order (no inline style may sit in index.html), so the
  // minifier has dropped each file's own statement: a layer is ranked by the first
  // rule that opens it -- the chunks' `components` long before base.css's `reset`.
  const rank = (name: string | null) => {
    if (name === null) return Number.MAX_SAFE_INTEGER; // unlayered wins over every layer
    if (!ranks.has(name)) ranks.set(name, nextLayer++);
    return ranks.get(name)!;
  };
  for (const file of options.files) for (const rule of rules(sources[file] ?? "")) rank(layerOf(rule));
  let winner: { value: string; layer: number; specificity: number; order: number } | null = null;
  let order = 0;
  for (const file of options.files) {
    for (const rule of rules(sources[file] ?? "")) {
      for (const selector of rule.prelude.split(",")) {
        const hit = selectorMatches(selector);
        if (!hit || !hit.matched) continue;
        const match = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, "i").exec(rule.body);
        if (!match) continue;
        const candidate = { value: match[1].trim(), layer: rank(layerOf(rule)), specificity: hit.specificity, order: order++ };
        const better = !winner || candidate.layer > winner.layer
          || (candidate.layer === winner.layer && (candidate.specificity > winner.specificity
            || (candidate.specificity === winner.specificity && candidate.order > winner.order)));
        if (better) winner = candidate;
      }
    }
  }
  return winner?.value ?? null;
}

/** CSS pixels of a one-, two-, three- or four-value shorthand, per side. */
function side(value: string | null, which: "left" | "right"): number {
  const parts = (value ?? "").split(/\s+/).filter(Boolean).map(parseFloat);
  if (!parts.length) return NaN;
  if (parts.length === 1) return parts[0];
  if (parts.length === 2 || parts.length === 3) return parts[1];
  return which === "left" ? parts[3] : parts[1];
}

/** The labels' distance in the tab row: left padding, the row's gap, right padding. */
function labelSpacing(options: { files: string[]; sources?: Record<string, string> }) {
  const padding = [side(resolve("padding-left", options) ?? resolve("padding", options), "left"),
    side(resolve("padding-right", options) ?? resolve("padding", options), "right")] as const;
  const gap = parseFloat(resolve("column-gap", options) ?? resolve("gap", options) ?? "");
  return { padding: [...padding], gap, px: padding[0] + gap + padding[1] };
}

// Task 454's own lines, taken out to rebuild the stylesheet the owner's app had
// (the checkout may hand these files back with CRLF endings).
const before = {
  ...sheets,
  "detail.css": sheets["detail.css"]
    .replace(/^\.detail-tabs \{ gap: 6px; \}(?:\r?\n|$)/m, "")
    .replace(/^\.detail-tabs \.tab \{ padding: 0 10px; \}(?:\r?\n|$)/m, ""),
};

describe("the session tabs' spacing in the packaged app (task 454)", () => {
  it("keeps the document free of an inline style, which Tauri's CSP would turn into a blocked skin", () => {
    // Tauri hashes an inline <style> into style-src (tauri::manager::set_csp), and a
    // hash in that directive makes `'unsafe-inline'` stop applying: the skin's
    // runtime-created <style> would be blocked and every colour left undefined.
    expect(indexHtml).not.toMatch(/<style[\s>]/i);
    expect(indexHtml).not.toMatch(/@layer/);
    // base.css still declares the order for the dev server and this test.
    expect(/^@layer reset, base, components, variants, utilities;/m.test(sheets["base.css"])).toBe(true);
  });

  it("is what the owner's build lost: the packaged order without the row's own rules strips the padding", () => {
    const broken = labelSpacing({ files: PACKAGED_ORDER, sources: before });
    // base.css's reset is appended after components, so `button { padding: 0 }`
    // wins over .detail-tabs .tab: only the row's gap stands between labels.
    expect(broken.padding).toEqual([0, 0]);
    expect(broken.gap).toBe(6);
    expect(broken.px).toBeLessThan(16);
    // The real stylesheets were read in that packaged order: the tab rule is in
    // detail.css and the reset in base.css.
    expect(sheets["detail.css"]).toMatch(/\.detail-tabs \.tab \{[^}]*padding: 0 10px;/);
    expect(sheets["base.css"]).toMatch(/@layer reset \{[\s\S]*?button \{[^}]*padding: 0;/);
  });

  it("gives the labels a clear distance from the row's own unlayered rules", () => {
    const fixed = labelSpacing({ files: PACKAGED_ORDER });
    expect(fixed.padding).toEqual([10, 10]);
    expect(fixed.gap).toBe(6);
    expect(fixed.px).toBe(26);
    expect(fixed.px).toBeGreaterThanOrEqual(16);
    // Pixels, not tokens: no skin or appearance can shrink the spacing.
    expect(sheets["detail.css"]).toMatch(/^\.detail-tabs \{ gap: 6px; \}$/m);
    expect(sheets["detail.css"]).toMatch(/^\.detail-tabs \.tab \{ padding: 0 10px; \}$/m);
  });
});

