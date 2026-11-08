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
  it("give the Compact skins Qt's side panel of sections and keep tabs for the others (task 206)", () => {
    const files = [zerusDark, zerusLight, fjord, paper, whitewhDark, whitewhLight] as SkinFile[];
    expect(Object.fromEntries(resolveSkins(files).map((skin) => [skin.id, skin.layout["side-panel"]]))).toEqual({
      "zerus-dark": "tabs", "zerus-light": "tabs", fjord: "tabs", paper: "tabs", "whitewh-dark": "sections", "whitewh-light": "sections",
    });
    const odd = { ...(whitewhDark as SkinFile), id: "odd", layout: { "side-panel": "drawer" } } as unknown as SkinFile;
    expect(() => resolveSkins([zerusDark as SkinFile, odd])).toThrow(/side-panel/);
  });
  it("give the Compact skins Qt's one title bar and keep the system chrome for the others (task 243)", () => {
    const files = [zerusDark, zerusLight, fjord, paper, whitewhDark, whitewhLight] as SkinFile[];
    expect(Object.fromEntries(resolveSkins(files).map((skin) => [skin.id, skin.layout.chrome]))).toEqual({
      "zerus-dark": "system", "zerus-light": "system", fjord: "system", paper: "system", "whitewh-dark": "unified", "whitewh-light": "unified",
    });
    const odd = { ...(whitewhDark as SkinFile), id: "odd", layout: { chrome: "frameless" } } as unknown as SkinFile;
    expect(() => resolveSkins([zerusDark as SkinFile, odd])).toThrow(/chrome/);
  });
});
