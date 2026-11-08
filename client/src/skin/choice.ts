import type { ResolvedSkin } from "./skin";

// Settings > Appearance > Skin without the DOM: which skin a slot holds, what
// choosing one writes, and the layout its preview draws (task 348).

export type Appearance = "dark" | "light";
export type SettingWrite = [key: string, value: string];
type Listed = Pick<ResolvedSkin, "id" | "appearance">;

export const SLOT: Record<Appearance, string> = { dark: "client/darkSkin", light: "client/lightSkin" };
const other = (appearance: Appearance): Appearance => (appearance === "dark" ? "light" : "dark");

/**
 * The skin chosen for `appearance`; an unknown or mismatched id falls back to
 * the slot's default. While the fork's Qt skin is WhiteWh (workspace/skin), a
 * slot that still holds its default shows the ported Compact skin.
 */
export function slotSkin(appearance: Appearance, read: (key: string) => string, defaults: Readonly<Record<string, unknown>>, list: Listed[]): string {
  const id = read(SLOT[appearance]);
  const fallback = String(defaults[SLOT[appearance]]);
  if (id === fallback && read("workspace/skin") === "whitewh") {
    const ported = `whitewh-${appearance}`;
    if (list.some((s) => s.id === ported)) return ported;
  }
  return list.some((s) => s.id === id && s.appearance === appearance) ? id : fallback;
}

/**
 * What choosing `chosen` writes, in order. Task 348: a standard skin chosen
 * while the WhiteWh port is on ends the port, or the slot's default id would
 * read as Compact again and the choice could never be made; the other
 * appearance keeps the Compact it shows, written into its own slot first.
 * A fixed theme of the other appearance moves to this one, as before.
 */
export function choiceWrites(chosen: Listed, read: (key: string) => string, defaults: Readonly<Record<string, unknown>>, list: Listed[]): SettingWrite[] {
  const writes: SettingWrite[] = [];
  if (read("workspace/skin") === "whitewh" && !chosen.id.startsWith("whitewh-")) {
    const keep = slotSkin(other(chosen.appearance), read, defaults, list);
    if (keep !== read(SLOT[other(chosen.appearance)])) writes.push([SLOT[other(chosen.appearance)], keep]);
    writes.push(["workspace/skin", "zerus"]);
  }
  writes.push([SLOT[chosen.appearance], chosen.id]);
  const theme = read("workspace/theme");
  if (theme !== "system" && theme !== chosen.appearance) writes.push(["workspace/theme", chosen.appearance]);
  return writes;
}

/** The layout a skin's preview draws: one value per variant the skin chooses. */
export interface PreviewLayout {
  chrome: "system" | "unified";
  rail: "icons" | "labeled";
  sessions: "left" | "right";
  inspector: "docked" | "collapsed";
  sidePanel: "tabs" | "sections";
  activity: "centered" | "full";
  tree: "lines" | "indent";
}

const pick = <T extends string>(value: unknown, allowed: readonly T[], fallback: T): T =>
  (allowed as readonly unknown[]).includes(value) ? (value as T) : fallback;

export function previewLayout(skin: Pick<ResolvedSkin, "layout" | "components">): PreviewLayout {
  const layout = skin.layout as Record<string, unknown>;
  const components = skin.components as Record<string, unknown>;
  return {
    chrome: pick(layout.chrome, ["system", "unified"] as const, "system"),
    rail: pick(layout.rail, ["icons", "labeled"] as const, "icons"),
    sessions: pick(layout["sessions-pane"], ["left", "right"] as const, "left"),
    inspector: pick(layout.inspector, ["docked", "collapsed"] as const, "docked"),
    sidePanel: pick(layout["side-panel"], ["tabs", "sections"] as const, "tabs"),
    activity: pick(layout["activity-column"], ["centered", "full"] as const, "centered"),
    tree: pick(components["tree-lines"], ["lines", "indent"] as const, "lines"),
  };
}
