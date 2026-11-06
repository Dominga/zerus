import { describe, expect, it } from "vitest";
import { KIND_STYLES, contrast, distance, keyName, kindKey, labelOn, lossKey, swatchBackground } from "./profile-palette";
import { resolveSkins, type SkinFile } from "../../skin/skin";
import zerusDark from "../../skin/skins/zerus-dark.json";
import zerusLight from "../../skin/skins/zerus-light.json";
import fjord from "../../skin/skins/fjord.json";
import paper from "../../skin/skins/paper.json";

// Task #79: every span kind of the team profile readable and distinct, in Qt
// and here, from one palette.

// Read from disk, as styles/motion.test.ts does (no Node types here).
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const repo = new URL("../../../../", import.meta.url);
// Every shipped skin, resolved as the app does (skin/runtime.ts).
const skins = resolveSkins([zerusDark, zerusLight, fjord, paper] as SkinFile[]);
const PATTERNS: Record<string, string> = {
  SolidPattern: "solid", Dense5Pattern: "dots", BDiagPattern: "bdiag", FDiagPattern: "fdiag", DiagCrossPattern: "diagcross",
  HorPattern: "hor", VerPattern: "ver", CrossPattern: "cross",
};

describe("the profile palette", () => {
  it("is Qt's table, kind for kind", () => {
    const qt = fs.readFileSync(new URL("tray/src/TeamProfile.cpp", repo), "utf8");
    const rows = [...qt.matchAll(/\{"([a-z_]+)", "[a-z_]*", "[a-z]*", QColor\("(#[0-9a-f]{6})"\), QColor\("(#[0-9a-f]{6})"\), Qt::(\w+)\}/g)]
      .map((m) => ({ key: m[1], dark: m[2], light: m[3], pattern: PATTERNS[m[4]] }));
    expect(rows).toEqual(KIND_STYLES.map((style) => ({ ...style })));
  });

  it("no two kinds share a colour in any shipped skin, light or dark", () => {
    expect(skins.length).toBeGreaterThan(0);
    // The Qt skins' lanes too (WorkspaceSkin designTokens card).
    const lanes = [...skins.map((s) => ({ id: s.id, dark: s.appearance === "dark", card: String(s.tokens.color?.card ?? "") })),
      { id: "qt-dark", dark: true, card: "#181c22" }, { id: "qt-light", dark: false, card: "#ffffff" }];
    for (const lane of lanes) {
      expect(lane.card).toMatch(/^#[0-9a-f]{6}$/i);
      const colours = KIND_STYLES.map((style) => ({ key: style.key, hex: lane.dark ? style.dark : style.light }));
      for (const [i, a] of colours.entries()) {
        expect(contrast(a.hex, lane.card), `${lane.id} ${a.key} on its lane`).toBeGreaterThanOrEqual(3);
        expect(contrast(labelOn(a.hex), a.hex), `${a.key} label`).toBeGreaterThanOrEqual(4.5);
        for (const b of colours.slice(i + 1)) expect(distance(a.hex, b.hex), `${lane.id}: ${a.key} and ${b.key}`).toBeGreaterThanOrEqual(20);
      }
    }
  });

  it("tells every wait by its own pattern, not only by hue", () => {
    const waits = KIND_STYLES.filter((style) => style.pattern !== "solid");
    expect(new Set(waits.map((style) => style.pattern)).size).toBe(waits.length);
    expect(waits.map((style) => style.key)).toEqual(["delivery", "member", "deps", "lead", "folder_held", "hold", "stage_wait"]);
    for (const style of waits) expect(swatchBackground(style, true)).not.toBe(style.dark);
  });

  it("groups the span kinds as Qt does and names each in full", () => {
    expect(kindKey("permission")).toBe("blocked");
    expect(kindKey("start_delay")).toBe("delivery");
    expect(kindKey("stage", "review")).toBe("review");
    expect(kindKey("stage", "lint")).toBe("stage");
    expect(lossKey("member_wait")).toBe("member");
    expect(lossKey("idle_member")).toBeUndefined();
    const names = KIND_STYLES.map((style) => keyName(style.key));
    expect(new Set(names).size).toBe(names.length);
    for (const name of names) expect(name.endsWith("…")).toBe(false);
  });
});
