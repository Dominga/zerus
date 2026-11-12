import { describe, expect, it } from "vitest";

const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const css = fs.readFileSync(new URL("./detail.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
  selectors: m[1].split(",").map((s) => s.trim()),
  body: Object.fromEntries(m[2].split(";").filter((s) => s.includes(":")).map((s) => {
    const at = s.indexOf(":");
    return [s.slice(0, at).trim(), s.slice(at + 1).trim()];
  })),
}));
const value = (selector: string, property: string) =>
  rules.filter((r) => r.selectors.includes(selector) && property in r.body).map((r) => r.body[property]).at(-1);

describe("DeepSeek account defaults notice (zerus-knlxe)", () => {
  it("wraps a long unbroken reason inside the notice instead of widening the dialog", () => {
    // A flex child keeps its min-content width unless it may shrink; anywhere
    // also lets a 600-character word or a long path break at any point.
    expect(value(".cmp-settings-defaults-text", "min-width")).toBe("0");
    expect(value(".cmp-settings-defaults-text", "overflow-wrap")).toBe("anywhere");
  });
});
