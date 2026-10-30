import { describe, expect, it } from "vitest";
import { resolveIcon } from "./icons";

// An unknown name renders an empty glyph, which only shows up as a gap in the
// running app (the File reference title, the Tasks session filter).

// Read from disk, as styles/motion.test.ts does: the specifier is built so
// TypeScript (no Node types here) leaves it alone.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
  readdirSync(path: URL, options: { recursive: true }): string[];
};
const src = new URL("../../", import.meta.url);

describe("icons", () => {
  it("every literal Icon name in the client resolves to a glyph", () => {
    const missing: string[] = [];
    let seen = 0;
    for (const file of fs.readdirSync(src, { recursive: true })) {
      if (!/\.tsx?$/.test(file) || /\.test\.tsx?$/.test(file)) continue;
      for (const [, name] of fs.readFileSync(new URL(file, src), "utf8").matchAll(/<Icon\b[^>]*?\bname="([^"]+)"/g)) {
        seen++;
        if (!resolveIcon(name).html) missing.push(`${file}: ${name}`);
      }
    }
    expect(seen).toBeGreaterThan(100);
    expect(missing).toEqual([]);
  });
});
