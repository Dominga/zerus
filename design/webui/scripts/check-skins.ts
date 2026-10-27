// Validate, resolve and compile every built-in skin (base skin first).
// Run with `npm run skins:check`; exits non-zero on the first invalid skin.
import { readFileSync } from "node:fs";
import { resolveSkins, compileSkin, type SkinFile } from "../src/skin/skin.ts";

const order = ["zerus-dark", "zerus-light", "fjord", "paper"];
const files = order.map((id) => JSON.parse(readFileSync(new URL(`../skins/${id}.json`, import.meta.url), "utf8")) as SkinFile);
try {
  for (const skin of resolveSkins(files)) {
    const css = compileSkin(skin);
    console.log(`${skin.id}: ${new Set(css.match(/--z-[a-z0-9-]+(?=:)/g)).size} properties`);
  }
} catch (error) {
  console.error(String(error));
  process.exit(1);
}
