import { createSignal, createMemo } from "solid-js";
import { resolveSkins, compileSkin, type ResolvedSkin, type SkinFile } from "./skin";
import { DEFAULTS, setSetting, settingsReady, text } from "../store/settings";
import zerusDark from "./skins/zerus-dark.json";
import zerusLight from "./skins/zerus-light.json";
import fjord from "./skins/fjord.json";
import paper from "./skins/paper.json";

// The base skin must come first: it defines the token set others may override.
export const skins: ResolvedSkin[] = resolveSkins([zerusDark, zerusLight, fjord, paper] as SkinFile[]);

const sheet = document.createElement("style");
sheet.id = "zerus-skins";
sheet.textContent = skins.map(compileSkin).join("\n");
document.head.prepend(sheet);

// Settings > Appearance: the Qt theme (workspace/theme: follow the system,
// dark or light) picks one of two skins, one per appearance
// (client/darkSkin, client/lightSkin). `?skin=` overrides both for a run.
const fromUrl = new URLSearchParams(location.search).get("skin");
const urlSkin = fromUrl && skins.some((s) => s.id === fromUrl) ? fromUrl : undefined;

export type Theme = "system" | "dark" | "light";
export type Appearance = "dark" | "light";

const media = typeof matchMedia === "undefined" ? undefined : matchMedia("(prefers-color-scheme: light)");
const [systemLight, setSystemLight] = createSignal(media?.matches ?? false);
media?.addEventListener("change", (e) => setSystemLight(e.matches));
export { systemLight };

export const theme = (): Theme => text("workspace/theme") as Theme;
const slot = (appearance: Appearance) => (appearance === "dark" ? "client/darkSkin" : "client/lightSkin");

/** Reactive list of built-in plus imported skins. */
const [skinListSignal, setSkinList] = createSignal<ResolvedSkin[]>([...skins]);
export const skinList = skinListSignal;

/** The skin chosen for `appearance`; an unknown or mismatched id falls back to the slot's default. */
export function chosenSkin(appearance: Appearance): string {
  const id = text(slot(appearance));
  return skinListSignal().some((s) => s.id === id && s.appearance === appearance) ? id : String(DEFAULTS[slot(appearance)]);
}

/** The appearance a theme shows now. */
export const appearanceFor = (choice: Theme, light: boolean): Appearance =>
  choice === "dark" ? "dark" : choice === "light" ? "light" : light ? "light" : "dark";

/** The skin in effect: the theme's appearance picks one of the pair. */
export const skinId = createMemo(() => urlSkin ?? chosenSkin(appearanceFor(theme(), systemLight())));
export const skin = createMemo(() => skinListSignal().find((s) => s.id === skinId()) ?? skins[0]);

/** Settings > Appearance > Theme. */
export function chooseTheme(choice: Theme) {
  void setSetting("workspace/theme", choice);
}

/**
 * Chooses a skin for its appearance. A fixed theme of the other appearance
 * moves to this one, as the mockup does; whether the skin is in use now.
 */
export function chooseSkin(id: string): boolean {
  const chosen = skinListSignal().find((s) => s.id === id);
  if (!chosen) return false;
  void setSetting(slot(chosen.appearance), id);
  if (theme() !== "system" && theme() !== chosen.appearance) void setSetting("workspace/theme", chosen.appearance);
  return appearanceFor(theme(), systemLight()) === chosen.appearance;
}

/** The skin picked before the settings store (local storage), moved once into its slot. */
const LEGACY_SKIN = "zerus-client-skin";
void settingsReady.then(() => {
  try {
    const id = localStorage.getItem(LEGACY_SKIN);
    const legacy = id ? skins.find((s) => s.id === id) : undefined;
    if (legacy) void setSetting(slot(legacy.appearance), legacy.id);
    localStorage.removeItem(LEGACY_SKIN);
  } catch {
    /* storage may be unavailable */
  }
});

/** Register a validated, resolved user skin and its compiled CSS rule. */
export function registerSkin(resolved: ResolvedSkin) {
  let imported = document.getElementById("zerus-imported-skins");
  if (!imported) {
    imported = document.createElement("style");
    imported.id = "zerus-imported-skins";
    document.head.append(imported);
  }
  imported.textContent += `\n${compileSkin(resolved)}`;
  const list = skinListSignal().filter((s) => s.id !== resolved.id);
  setSkinList([...list, resolved]);
}

/** Reflect the active skin's tokens and L1 variants onto the root element. */
export function applySkin(s: ResolvedSkin) {
  const root = document.documentElement;
  root.dataset.skin = s.id;
  root.dataset.appearance = s.appearance;
  for (const [name, value] of Object.entries(s.components)) root.dataset[camel(name)] = value;
}

const camel = (name: string) => name.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase());
