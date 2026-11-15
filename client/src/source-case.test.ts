import { readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";

// macOS file systems ignore letter case, so an extensionless import such as
// "./PlanToolbar" can resolve to planToolbar.ts there while Linux picks
// PlanToolbar.tsx. Two module files in one folder must not differ only by case.
const MODULE = /\.(ts|tsx|js|jsx|mts|mjs|json)$/;

function collisions(dir: string): string[] {
  const found: string[] = [];
  const seen = new Map<string, string>();
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules") continue;
    if (entry.isDirectory()) {
      found.push(...collisions(join(dir, entry.name)));
      continue;
    }
    if (!MODULE.test(entry.name)) continue;
    const stem = entry.name.replace(MODULE, "");
    const key = stem.toLowerCase();
    const earlier = seen.get(key);
    if (earlier !== undefined && earlier.replace(MODULE, "") !== stem) {
      found.push(`${join(dir, earlier)} <-> ${join(dir, entry.name)}`);
    }
    seen.set(key, entry.name);
  }
  return found;
}

it("no two module files in one folder differ only by letter case", () => {
  const root = dirname(fileURLToPath(import.meta.url));
  expect(collisions(root)).toEqual([]);
});
