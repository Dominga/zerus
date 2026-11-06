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

describe("Long unbroken lines in Activity", () => {
  it.each([".act-msg", ".act-thinking", ".act-tools", ".act-notice"])("%s wraps them inside its card", (row) => {
    // Not anywhere: that also shrinks min-content, which would squeeze header
    // labels and let table columns fall to one character.
    expect(value(row, "overflow-wrap")).toBe("break-word");
  });
  it("a tool's output lines and a notice's body stay within their row", () => {
    // Both are sized by min-content, which break-word leaves at the whole run.
    expect(value(".act-tool-lines", "grid-template-columns")).toBe("minmax(0, 1fr)");
    expect(value(".act-notice-body", "max-width")).toBe("100%");
  });
  it("code blocks keep their own horizontal scrolling", () => {
    expect(value(".act-code", "white-space")).toBe("pre");
    expect(value(".act-code", "overflow-x")).toBe("auto");
  });
  it("the transcript itself still never scrolls sideways", () => {
    expect(value(".act-scroll", "overflow-x")).toBe("hidden");
  });
  it("no row gets a fixed height or a clip that would break measured row heights", () => {
    for (const row of [".act-msg", ".act-thinking", ".act-tools", ".act-notice", ".act-virtual-row"]) {
      expect(value(row, "height")).toBeUndefined();
      expect(value(row, "max-height")).toBeUndefined();
      expect(value(row, "overflow")).toBeUndefined();
    }
  });
});
