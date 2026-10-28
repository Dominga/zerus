// Fails when a dot-like separator appears in the design's own text or in the
// built output: Zerus never separates values with dots (owner rule,
// 2026-10-27). Values sit apart through layout (spacing, columns, labelled
// fields, chips) or line breaks instead.
//
// Usage: node scripts/check-separators.ts [extra paths...]
// Scans src/, review/, skins/, the HTML entry points and dist/ when present.
// Licence files, node_modules and third-party-only build chunks are not scanned.

import { readdirSync, readFileSync, statSync, existsSync } from "node:fs";
import { join, relative } from "node:path";

// U+00B7 middle dot, U+2022 bullet, U+2219 bullet operator, U+22C5 dot operator,
// U+2027 hyphenation point, U+2981 z notation spot, U+30FB katakana middle dot.
const SEPARATORS = /[\u00B7\u2022\u2219\u22C5\u2027\u2981\u30FB]/g;
const NAMES: Record<string, string> = {
  "\u00B7": "U+00B7", "\u2022": "U+2022", "\u2219": "U+2219", "\u22C5": "U+22C5", "\u2027": "U+2027", "\u2981": "U+2981", "\u30FB": "U+30FB",
};
const TEXT = /\.(tsx?|jsx?|mjs|css|html|json|md|svg|txt)$/;
const SKIP = new Set(["node_modules", "licenses", ".git"]);
// Built chunks that hold only third-party code: xterm.js maps "~" to a middle dot
// in its VT100 line-drawing character set, which is terminal data, not UI text.
const THIRD_PARTY = /[\\/]assets[\\/](xterm|addon-fit)-[^\\/]+\.js$/;

const root = new URL("..", import.meta.url).pathname;
const targets = [...["src", "review", "skins", "index.html", "app.html", "dist"].map((p) => join(root, p)), ...process.argv.slice(2)];

function* files(path: string): Generator<string> {
  if (!existsSync(path)) return;
  if (statSync(path).isDirectory()) {
    for (const name of readdirSync(path)) if (!SKIP.has(name)) yield* files(join(path, name));
  } else if (TEXT.test(path) && !THIRD_PARTY.test(path)) yield path;
}

let total = 0;
const report: string[] = [];
for (const target of targets) {
  for (const file of files(target)) {
    const lines = readFileSync(file, "utf8").split("\n");
    lines.forEach((line, i) => {
      const found = line.match(SEPARATORS);
      if (!found) return;
      total += found.length;
      const at = line.search(SEPARATORS);
      const excerpt = line.slice(Math.max(0, at - 50), at + 50).replace(/\s+/g, " ").trim();
      report.push(`${relative(root, file)}:${i + 1}: ${[...new Set(found)].map((c) => NAMES[c]).join(", ")}: ${excerpt}`);
    });
  }
}

if (total) {
  console.error(report.join("\n"));
  console.error(`\n${total} separator dot${total === 1 ? "" : "s"} found. Separate values with layout or line breaks, never dots.`);
  process.exit(1);
}
console.log("No separator dots in src, review, skins, entry pages or dist.");
