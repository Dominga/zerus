import { expect, it } from "vitest";

const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const sheet = (name: string) => fs.readFileSync(new URL(name, import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const css = sheet("./inspector.css");
const declarations = (selector: string, from = css) => {
  const rule = [...from.matchAll(/([^{}]+)\{([^{}]*)\}/g)].find((m) => m[1].trim() === selector);
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

it("keeps the header's keep-warm mark inside the chip row's height and unwrapped (zerus-yacns)", () => {
  const mark = declarations(".dh-warm", sheet("./detail.css"));
  // The agent badge is always on the row at this height, so the mark never makes the block taller.
  expect(mark.height).toBe(declarations(".agent-badge", sheet("./sessions.css")).height);
  expect([mark.flex, mark["white-space"]]).toEqual(["none", "nowrap"]);
  expect(mark["line-height"] ?? "").not.toMatch(/^(2|[3-9])/);
});
