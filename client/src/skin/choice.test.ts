import { describe, expect, it } from "vitest";
import { resolveSkins, type SkinFile } from "./skin";
import { SLOT, choiceWrites, previewLayout, slotSkin, type Appearance } from "./choice";
import zerusDark from "./skins/zerus-dark.json";
import zerusLight from "./skins/zerus-light.json";
import fjord from "./skins/fjord.json";
import paper from "./skins/paper.json";
import whitewhDark from "./skins/whitewh-dark.json";
import whitewhLight from "./skins/whitewh-light.json";

// Task 348: Settings > Appearance > Skin. Every built-in skin can be chosen for
// its appearance, back from Compact too, and each preview draws its layout.
const skins = resolveSkins([zerusDark, zerusLight, fjord, paper, whitewhDark, whitewhLight] as SkinFile[]);
const DEFAULTS = { "client/darkSkin": "zerus-dark", "client/lightSkin": "zerus-light", "workspace/skin": "zerus", "workspace/theme": "system" };

/** A settings map; applying a choice's writes in order, as chooseSkin does. */
function store(start: Record<string, string>) {
  const values: Record<string, string> = { ...DEFAULTS, ...start };
  const read = (key: string) => values[key] ?? "";
  const choose = (id: string) => {
    const chosen = skins.find((skin) => skin.id === id)!;
    for (const [key, value] of choiceWrites(chosen, read, DEFAULTS, skins)) values[key] = value;
  };
  const shown = (appearance: Appearance) => slotSkin(appearance, read, DEFAULTS, skins);
  return { values, choose, shown };
}

describe("choosing a skin (task 348)", () => {
  // The fork's start: Qt's WhiteWh skin ported, both slots still at their defaults.
  const ported = { "workspace/skin": "whitewh" };

  it("shows Compact for both appearances while the WhiteWh port is on", () => {
    const { shown } = store(ported);
    expect([shown("dark"), shown("light")]).toEqual(["whitewh-dark", "whitewh-light"]);
  });

  for (const start of [{ name: "from the ported Compact", values: ported }, { name: "with no port", values: {} }]) {
    for (const skin of skins) {
      it(`selects ${skin.name} for ${skin.appearance} ${start.name}, the other appearance unchanged`, () => {
        const { choose, shown } = store(start.values);
        const other: Appearance = skin.appearance === "dark" ? "light" : "dark";
        const before = shown(other);
        choose(skin.id);
        expect(shown(skin.appearance)).toBe(skin.id);
        expect(shown(other)).toBe(before);
      });
    }
  }

  it("goes back and forth between Compact and the standard skins", () => {
    const { choose, shown } = store(ported);
    choose("zerus-dark");
    expect([shown("dark"), shown("light")]).toEqual(["zerus-dark", "whitewh-light"]);
    choose("zerus-light");
    expect([shown("dark"), shown("light")]).toEqual(["zerus-dark", "zerus-light"]);
    choose("whitewh-dark");
    expect(shown("dark")).toBe("whitewh-dark");
    choose("zerus-dark");
    expect(shown("dark")).toBe("zerus-dark");
  });

  it("ends the port once and keeps the other slot's Compact written first", () => {
    const { choose, values } = store(ported);
    const chosen = skins.find((skin) => skin.id === "zerus-dark")!;
    const read = (key: string) => values[key] ?? "";
    expect(choiceWrites(chosen, read, DEFAULTS, skins)).toEqual([[SLOT.light, "whitewh-light"], ["workspace/skin", "zerus"], [SLOT.dark, "zerus-dark"]]);
    choose("zerus-dark");
    expect(values["workspace/skin"]).toBe("zerus");
  });

  it("moves a fixed theme of the other appearance to the chosen skin's", () => {
    const { choose, values } = store({ "workspace/theme": "dark" });
    choose("paper");
    expect(values["workspace/theme"]).toBe("light");
  });
});

describe("each skin's preview draws its layout (task 348)", () => {
  const layout = (id: string) => previewLayout(skins.find((skin) => skin.id === id)!);

  it("draws Compact's one title bar and side panel of sections, unlike Zerus", () => {
    expect(layout("whitewh-dark")).toMatchObject({ chrome: "unified", sidePanel: "sections" });
    expect(layout("whitewh-light")).toMatchObject({ chrome: "unified", sidePanel: "sections" });
    expect(layout("zerus-dark")).toMatchObject({ chrome: "system", sidePanel: "tabs" });
    expect(layout("whitewh-dark")).not.toEqual(layout("zerus-dark"));
    expect(layout("whitewh-light")).not.toEqual(layout("zerus-light"));
  });

  it("draws Fjord's labeled rail and full Activity and Paper's sessions on the right, collapsed panel and indented subagents", () => {
    expect(layout("fjord")).toMatchObject({ rail: "labeled", activity: "full", sessions: "left" });
    expect(layout("paper")).toMatchObject({ sessions: "right", inspector: "collapsed", tree: "indent" });
  });

  it("gives every built-in skin of one appearance its own preview", () => {
    for (const appearance of ["dark", "light"] as const) {
      const shown = skins.filter((skin) => skin.appearance === appearance).map((skin) => JSON.stringify(previewLayout(skin)));
      expect(new Set(shown).size).toBe(shown.length);
    }
  });

  it("marks each variant on the preview for its styles", async () => {
    const fs = await import(/* @vite-ignore */ ["node", "fs"].join(":")) as { readFileSync(path: URL, encoding: "utf8"): string };
    const source = fs.readFileSync(new URL("../ui/screens/settings/Appearance.tsx", import.meta.url), "utf8");
    const css = fs.readFileSync(new URL("../ui/styles/settings.css", import.meta.url), "utf8");
    for (const attribute of ["data-chrome", "data-rail", "data-sessions", "data-inspector", "data-side-panel", "data-activity", "data-tree"]) expect(source).toContain(`${attribute}={`);
    for (const rule of ['.skin-prev[data-rail="labeled"]', '.skin-prev[data-sessions="right"]', '.skin-prev[data-tree="indent"]', '.skin-prev[data-activity="full"]', ".skp-title", ".skp-section"]) expect(css).toContain(rule);
  });
});
