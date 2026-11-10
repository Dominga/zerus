import { describe, expect, it } from "vitest";
import { CHROMA_CAP, KIND_STYLES, chroma, contrast, distance, keyName, kindKey, labelOn, lossKey, swatchBackground } from "./profile-palette";
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

describe("the profile palette", () => {
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

  it("keeps every colour calm: no kind above the chroma cap in any skin (task 93)", () => {
    expect(CHROMA_CAP).toEqual({ dark: 45, light: 55 });
    // The palette is one table for every skin; each skin uses its theme's half.
    for (const skin of skins) {
      const dark = skin.appearance === "dark";
      for (const style of KIND_STYLES) {
        const hex = dark ? style.dark : style.light;
        expect(chroma(hex), `${skin.id} ${style.key} ${hex}`).toBeLessThanOrEqual(dark ? CHROMA_CAP.dark : CHROMA_CAP.light);
      }
    }
    // The old toxic green is gone; work is the skin's liked mint.
    expect(KIND_STYLES.find((style) => style.key === "work")).toMatchObject({ dark: "#7dd3b0", light: "#3f9f7b" });
    expect(chroma("#07ab2d")).toBeGreaterThan(CHROMA_CAP.dark);
  });

  it("tells every wait by its own pattern, not only by hue", () => {
    const waits = KIND_STYLES.filter((style) => style.pattern !== "solid");
    expect(new Set(waits.map((style) => style.pattern)).size).toBe(waits.length);
    expect(waits.map((style) => style.key)).toEqual(["delivery", "member", "deps", "lead", "person", "folder_held", "hold", "stage_wait"]);
    for (const style of waits) expect(swatchBackground(style, true)).not.toBe(style.dark);
  });

  it("gives waiting for a person's answer its own calm colour and sparse hatch (task 118)", () => {
    const person = KIND_STYLES.find((style) => style.key === "person");
    expect(person).toMatchObject({ dark: "#d8eea5", light: "#8a9269", pattern: "sparse" });
    // Apart from every other kind by the plain distance in each theme...
    for (const theme of ["dark", "light"] as const) {
      for (const other of KIND_STYLES.filter((style) => style.key !== "person")) {
        expect(distance(person![theme], other[theme]), `${theme}: person and ${other.key}`).toBeGreaterThanOrEqual(24);
      }
    }
    // ...and under deuteranopia and protanopia (Machado et al. 2009, full
    // severity): the new colour never falls onto another kind's.
    const SIM = {
      deuteranopia: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
      protanopia: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
    };
    const lin = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
    const gam = (v: number) => { const c = Math.min(1, Math.max(0, v)); return c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055; };
    const seen = (hex: string, m: number[][]) => {
      const rgb = [1, 3, 5].map((at) => lin(parseInt(hex.slice(at, at + 2), 16) / 255));
      const out = m.map((row) => Math.round(255 * gam(row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2])));
      return `#${out.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
    };
    for (const [name, matrix] of Object.entries(SIM)) {
      for (const theme of ["dark", "light"] as const) {
        for (const other of KIND_STYLES.filter((style) => style.key !== "person")) {
          expect(distance(seen(person![theme], matrix), seen(other[theme], matrix)), `${name} ${theme}: person and ${other.key}`).toBeGreaterThanOrEqual(6);
        }
      }
    }
    // Named in full, in the legend and for the loss; the lead's wait keeps its own.
    expect(keyName("person")).toBe("Waiting for a person's answer");
    expect(keyName("lead")).toBe("Waited for the lead");
    expect(lossKey("person_wait")).toBe("person");
    expect(kindKey("person")).toBe("person");
  });

  it("wraps the legend: every label whole, on as many lines as the width needs (task 118)", () => {
    // The owner's installed build drew the legend on one line and cut 'Rework (',
    // 'Build and', 'Waited fo'. The rules that keep it whole: a wrapping flex row
    // of items that never shrink or ellipsize their names, in a column that is
    // as wide as the page and scrolls vertically (checked in Edge at 300 to 1200
    // px: 12 down to 3 lines, no label cut).
    const css = fs.readFileSync(new URL("client/src/ui/styles/plan.css", repo), "utf8");
    const rule = (selector: string) => css.split("\n").find((line) => line.startsWith(`${selector} {`)) ?? "";
    const legend = rule(".pm-legend"), item = rule(".pm-legend-item");
    expect(legend).toMatch(/display: flex;/);
    expect(legend).toMatch(/flex-wrap: wrap;/);
    expect(item).toMatch(/white-space: nowrap;/);
    for (const text of [legend, item, rule(".pm-legend-title")]) expect(text).not.toMatch(/overflow|text-overflow|max-width|width:/);
    // Task #339: the page scrolls vertically and the panel never clips the
    // legend; the lanes, their legend and the estimates share one column.
    expect(rule(".pm-profile-scroll")).toMatch(/overflow: auto;/);
    expect(rule(".pm-profile-details")).toMatch(/overflow: visible;/);
    // One item per palette kind, named in full, so a new kind is in it too.
    const panels = fs.readFileSync(new URL("client/src/ui/teams/plan-panels.tsx", repo), "utf8");
    expect(panels).toContain("<For each={KIND_STYLES}>{(style) => <span class=\"pm-legend-item\"");
    expect(panels).toContain("{keyName(style.key)}");
    expect(KIND_STYLES.map((style) => keyName(style.key))).toContain("Waiting for a person's answer");
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
