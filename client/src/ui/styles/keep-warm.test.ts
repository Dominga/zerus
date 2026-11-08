import { expect, it } from "vitest";

const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const css = fs.readFileSync(new URL("./inspector.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const declarations = (selector: string) => {
  const rule = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].find((m) => m[1].trim() === selector);
  if (!rule) throw Error(`Missing rule: ${selector}`);
  return Object.fromEntries(rule[2].split(";").filter((s) => s.includes(":")).map((s) => s.split(":").map((v) => v.trim())));
};

it("gives the keep-warm duration a fixed width, so the slider never moves when it changes (zerus-ua4bi)", () => {
  const value = declarations(".kw-value");
  expect(value.width).toMatch(/^\d+(\.\d+)?(ch|em)$/);
  expect(value["min-width"]).toBeUndefined();
  expect(value.flex).toBe("none");
  expect(value["white-space"]).toBe("nowrap");
});
