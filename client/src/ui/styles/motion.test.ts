import { describe, expect, it } from "vitest";

// Guards for the motion rules (motion.css): every keyframe animates only what
// the compositor runs without repainting, and every endless animation stops
// under Reduce motion.

// Read from disk: Vitest hands stylesheet imports over empty, even with `?raw`.
// The specifier is built so TypeScript (no Node types here) leaves it alone.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readdirSync(path: URL): string[];
  readFileSync(path: URL, encoding: "utf8"): string;
};
const here = new URL("./", import.meta.url);
const sheets: Record<string, string> = Object.fromEntries(fs.readdirSync(here).filter((f) => f.endsWith(".css")).map((f) => [f, fs.readFileSync(new URL(f, here), "utf8")]));

interface Rule { prelude: string; body: string; parents: string[]; file: string }

/** Leaf rules (no nested blocks) of a stylesheet, with the preludes of their enclosing blocks. */
function rules(file: string, css: string): Rule[] {
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
      if (!open.nested) out.push({ prelude: open.prelude, body: text.slice(open.start, i), parents: stack.map((s) => s.prelude), file });
      last = i + 1;
    } else if (c === ";") {
      last = i + 1;
    }
  }
  return out;
}

const all = Object.entries(sheets).flatMap(([file, css]) => rules(file, css));
const selectors = (prelude: string) => prelude.split(",").map((s) => s.trim().replace(/\s+/g, " ")).filter(Boolean);
const properties = (body: string) => body.split(";").map((d) => d.split(":")[0].trim()).filter(Boolean);

/** Selectors of motion.css rules under `attribute` whose body sets `declaration`, without the root prefix. */
function stopped(attribute: string, declaration: RegExp): string[] {
  const prefix = `:root[${attribute}]`;
  return all.filter((r) => r.file.endsWith("motion.css") && declaration.test(r.body))
    .flatMap((r) => selectors(r.prelude))
    .filter((s) => s.startsWith(prefix))
    .map((s) => s.slice(prefix.length).trim());
}

const COMPOSITED = new Set(["transform", "translate", "scale", "rotate", "opacity", "animation-timing-function"]);

describe("motion styles", () => {
  it("finds the stylesheets and their keyframes", () => {
    expect(Object.keys(sheets).some((f) => f.endsWith("motion.css"))).toBe(true);
    expect(all.filter((r) => r.parents.at(-1)?.startsWith("@keyframes")).length).toBeGreaterThan(10);
  });

  it("animates only transforms and opacity in every keyframe", () => {
    const offending = all
      .filter((r) => r.parents.at(-1)?.startsWith("@keyframes"))
      .flatMap((r) => properties(r.body).filter((p) => !COMPOSITED.has(p)).map((p) => `${r.parents.at(-1)} ${r.prelude}: ${p}`));
    expect(offending).toEqual([]);
  });

  it("stops every endless animation under Reduce motion", () => {
    const reduced = stopped('data-reduce-motion="1"', /animation:\s*none/);
    const endless = all
      .filter((r) => !r.file.endsWith("motion.css") && !r.parents.some((p) => p.startsWith("@keyframes")) && /animation(-iteration-count)?\s*:[^;]*\binfinite\b/.test(r.body))
      .flatMap((r) => selectors(r.prelude));
    expect(endless.length).toBeGreaterThan(10);
    const covered = (s: string) => reduced.some((r) => s === r || s.endsWith(` ${r}`) || (r === ".zlogo *" && s.startsWith(".zlogo")));
    expect(endless.filter((s) => !covered(s))).toEqual([]);
  });

  it("stops each working indicator under both settings", () => {
    const working = stopped('data-animate-activity="0"', /animation:\s*none/);
    expect(working).toEqual(expect.arrayContaining([".sbadge.k-working .sb-mark::after", ".gb-count.working", ".typing i"]));
    const reduced = stopped('data-reduce-motion="1"', /animation:\s*none/);
    expect(working.filter((s) => !reduced.includes(s))).toEqual([]);
  });

  it("pauses everything, pseudo-elements included, while the window is hidden", () => {
    expect(stopped('data-page-hidden="1"', /animation-play-state:\s*paused\s*!important/)).toEqual(["*", "*::before", "*::after"]);
  });
});
