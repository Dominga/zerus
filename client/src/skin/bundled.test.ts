import { describe, expect, it } from "vitest";
import { resolveSkins, type SkinFile } from "./skin";
import zerusDark from "./skins/zerus-dark.json";
import zerusLight from "./skins/zerus-light.json";
import fjord from "./skins/fjord.json";
import paper from "./skins/paper.json";
import whitewhDark from "./skins/whitewh-dark.json";
import whitewhLight from "./skins/whitewh-light.json";

// runtime.ts resolves these six at import: a token the base skin does not
// define throws there and the client stops at start-up.
describe("bundled skins", () => {
  it("resolve without an error, each token defined by the base skin", () => {
    const files = [zerusDark, zerusLight, fjord, paper, whitewhDark, whitewhLight] as SkinFile[];
    expect(() => resolveSkins(files)).not.toThrow();
    expect(resolveSkins(files).map((skin) => skin.id)).toEqual(files.map((file) => file.id));
  });
});
