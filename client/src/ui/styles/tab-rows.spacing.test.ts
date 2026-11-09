import { describe, expect, it } from "vitest";

// Task 463 (the owner's dev build, D:/Tools/zerus-build/verify-strip-inspector.png):
// the inspector's tabs "Plan & agents  Details  Worktrees  Tasks" ran together like
// the session tabs did before 454. The packaged document links the lazy chunks'
// stylesheets first (the inspector's among them) and the entry stylesheet last, and
// the minifier drops a file's `@layer a, b, c;` statement once its blocks follow in
// that order: the reset layer is then opened after components, and its
// `button { padding: 0 }` -- a later layer beats an earlier one whatever the
// specificity -- strips every tab's padding. Rows that never stated their own
// spacing (the inspector, the catalog, the plan modes) lose it; rows with an
// unlayered statement (the session tabs since 454) keep it.
//
// This test loads the real stylesheets in the packaged order and computes the
// cascade for those rows, so the spacing the owner sees is checked here. The fix
// is unlayered CSS in base.css (any tab row) and detail.css (the inspector's row),
// never an inline <style> in index.html: Tauri hashes an inline style into
// style-src, and a hash there stops `'unsafe-inline'` applying, which would block
// the skin's runtime-created style element and leave every colour undefined.

const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const read = (url: URL) => fs.readFileSync(url, "utf8");
const here = new URL("./", import.meta.url);
const indexHtml = read(new URL("../../../index.html", here));
const sheet = (name: string) => read(new URL(`./${name}`, here));

// client/dist/index.html's order: the chunks' stylesheets first (each opens
// @layer components), then the entry stylesheet, which is main.tsx's order and
// therefore has base.css before detail.css.
const CHUNK_SHEETS = ["sessions.css", "tasks.css", "inspector.css", "newsession.css"];
const ENTRY_SHEETS = ["base.css", "app.css", "sessions.css", "detail.css", "catalog.css", "client.css", "goto.css", "screens.css", "machines.css", "motion.css", "settings.css", "teams.css"];
const PACKAGED_ORDER = [...CHUNK_SHEETS, ...ENTRY_SHEETS];
const sheets: Record<string, string> = Object.fromEntries(PACKAGED_ORDER.map((name) => [name, sheet(name)]));

/** The same stylesheets without task 463's own unlayered blocks (the owner's app). */
const withoutOwnBlocks = (css: string) => {
  const at = css.indexOf("Task 463 (owner's dev build");
  return at < 0 ? css : css.slice(0, at);
};
const before = { ...sheets, "base.css": withoutOwnBlocks(sheets["base.css"]), "detail.css": withoutOwnBlocks(sheets["detail.css"]) };

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

/** A compound (".tab", "button.tab") against an element; null when the test does
 *  not model something it uses (:hover, [hidden], *). */
function compoundMatches(compound: string, classes: string[]): boolean | null {
  if (/[:\[]/.test(compound) || compound === "*") return null;
  let rest = compound;
  let ok = rest.startsWith(".") || rest.startsWith("button");
  rest = rest.replace(/^button/, "");
  for (const [, name] of rest.matchAll(/\.([\w-]+)/g)) ok = ok && classes.includes(name);
  rest = rest.replace(/\.[\w-]+/g, "");
  if (rest !== "") return null;
  return ok;
}

/** Whether a selector matches a tab inside the given row, and its specificity. */
function selectorMatches(selector: string, row: string[], tab: string[]): { matched: boolean; specificity: number } | null {
  const parts = selector.trim().split(/\s+/).filter((part) => !["", ">", "+", "~"].includes(part));
  if (!parts.length) return null;
  const last = compoundMatches(parts[parts.length - 1], tab);
  if (last === null) return null;
  const ancestors = parts.slice(0, -1);
  const rowHit = ancestors.length ? compoundMatches(ancestors[ancestors.length - 1], row) : null;
  const specificity = parts.reduce((total, part) => total + (part.match(/\.[\w-]+/g) ?? []).length, 0);
  return { matched: last && (rowHit === null || rowHit), specificity };
}

/** The winning declaration of `prop` on a tab inside the row, in file order. */
function resolve(prop: string, row: string[], source: Record<string, string>): string | null {
  const ranks = new Map<string, number>();
  let nextLayer = 0;
  // No inline style sits in index.html, so each file's own order statement has been
  // dropped by the minifier: a layer is ranked by the first rule that opens it.
  const rank = (name: string | null) => {
    if (name === null) return Number.MAX_SAFE_INTEGER; // unlayered wins over every layer
    if (!ranks.has(name)) ranks.set(name, nextLayer++);
    return ranks.get(name)!;
  };
  for (const file of PACKAGED_ORDER) for (const rule of rules(source[file] ?? "")) rank(layerOf(rule));
  let winner: { value: string; layer: number; specificity: number; order: number } | null = null;
  let order = 0;
  for (const file of PACKAGED_ORDER) {
    for (const rule of rules(source[file] ?? "")) {
      for (const selector of rule.prelude.split(",")) {
        const hit = selectorMatches(selector, row, ["tab"]);
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

/** The row's gap: its own, else the base `.tabs`. */
function rowGap(row: string[], source: Record<string, string>): number {
  const resolveRow = (prop: string) => {
    let winner: { value: string; layer: number; specificity: number; order: number } | null = null;
    let order = 0;
    let nextLayer = 0;
    const ranks = new Map<string, number>();
    const rank = (name: string | null) => {
      if (name === null) return Number.MAX_SAFE_INTEGER;
      if (!ranks.has(name)) ranks.set(name, nextLayer++);
      return ranks.get(name)!;
    };
    for (const file of PACKAGED_ORDER) for (const rule of rules(source[file] ?? "")) rank(layerOf(rule));
    for (const file of PACKAGED_ORDER) {
      for (const rule of rules(source[file] ?? "")) {
        for (const selector of rule.prelude.split(",")) {
          const parts = selector.trim().split(/\s+/).filter(Boolean);
          const hit = parts.length ? compoundMatches(parts[parts.length - 1], row) : null;
          if (hit !== true) continue;
          const match = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, "i").exec(rule.body);
          if (!match) continue;
          const candidate = { value: match[1].trim(), layer: rank(layerOf(rule)), specificity: parts.reduce((t, p) => t + (p.match(/\.[\w-]+/g) ?? []).length, 0), order: order++ };
          const better = !winner || candidate.layer > winner.layer
            || (candidate.layer === winner.layer && (candidate.specificity > winner.specificity
              || (candidate.specificity === winner.specificity && candidate.order > winner.order)));
          if (better) winner = candidate;
        }
      }
    }
    return winner?.value ?? null;
  };
  return parseFloat(resolveRow("column-gap") ?? resolveRow("gap") ?? "");
}

/** CSS pixels of a one-, two-, three- or four-value shorthand. */
function side(value: string | null, which: "left" | "right"): number {
  const parts = (value ?? "").split(/\s+/).filter(Boolean).map(parseFloat);
  if (!parts.length) return NaN;
  if (parts.length === 1) return parts[0];
  if (parts.length === 2 || parts.length === 3) return parts[1];
  return which === "left" ? parts[3] : parts[1];
}

/** The labels' distance in a tab row: left padding, the row's gap, right padding. */
function labelSpacing(row: string[], source: Record<string, string>) {
  const padding = [side(resolve("padding-left", row, source) ?? resolve("padding", row, source), "left"),
    side(resolve("padding-right", row, source) ?? resolve("padding", row, source), "right")] as const;
  const gap = rowGap(row, source);
  return { padding: [...padding], gap, px: padding[0] + gap + padding[1] };
}

describe("every tab row keeps its spacing in the packaged app (task 463)", () => {
  it("keeps the document free of an inline style, which Tauri's CSP would turn into a blocked skin", () => {
    // Tauri hashes an inline <style> into style-src (tauri::manager::set_csp) and a
    // hash there stops `'unsafe-inline'` applying: the skin's runtime-created
    // <style> would be blocked and every colour left undefined (task 454).
    expect(indexHtml).not.toMatch(/<style[\s>]/i);
    expect(indexHtml).not.toMatch(/@layer/);
  });

  it("is what the owner's dev build shows: the inspector's tabs lose their padding", () => {
    const broken = labelSpacing(["insp-tabs", "tabs"], before);
    expect(broken.padding).toEqual([0, 0]);
    expect(broken.gap).toBe(2);
    expect(broken.px).toBeLessThan(16);
  });

  it("gives the inspector row the session tabs' spacing: 6 px gap and 0 10 px per tab", () => {
    const fixed = labelSpacing(["insp-tabs", "tabs"], sheets);
    expect(fixed.padding).toEqual([10, 10]);
    expect(fixed.gap).toBe(6);
    expect(fixed.px).toBe(26);
    expect(sheets["detail.css"]).toMatch(/^\.insp-tabs \{ gap: 6px; \}$/m);
    expect(sheets["detail.css"]).toMatch(/^\.insp-tabs \.tab \{ padding: 0 10px; font-size: var\(--fs-xs\); \}$/m);
  });

  it("keeps every other row's tabs real: the base tab is unlayered, so a row without its own rules holds", () => {
    // The catalog's and the plan modes' rows state only their container: their tabs
    // take the base .tab, which must survive the packaged order.
    for (const row of [["cat-tabs", "tabs"], ["pm-tabs"]]) {
      const spacing = labelSpacing(row, sheets);
      expect(spacing.padding).toEqual([10, 10]);
      expect(spacing.px).toBeGreaterThanOrEqual(16);
    }
    // Before the unlayered base, those tabs had no padding at all.
    expect(labelSpacing(["cat-tabs", "tabs"], before).padding).toEqual([0, 0]);
    expect(sheets["base.css"]).toMatch(/^\.tabs \{ display: flex; gap: 2px; align-items: flex-end; \}$/m);
    expect(sheets["base.css"]).toMatch(/^\.tab \{\n  position: relative; height: 30px; padding: 0 10px;/m);
    // The selected tab keeps its own colour and weight, or the unlayered .tab
    // would mute it.
    expect(sheets["base.css"]).toMatch(/^\.tab\.on \{ color: var\(--z-color-text\); font-weight: 600; \}$/m);
  });
});
