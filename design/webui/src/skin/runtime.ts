import { createSignal, createMemo } from "solid-js";
import { resolveSkins, compileSkin, type ResolvedSkin, type SkinFile } from "./skin";
import zerusDark from "../../skins/zerus-dark.json";
import zerusLight from "../../skins/zerus-light.json";
import fjord from "../../skins/fjord.json";
import paper from "../../skins/paper.json";

// The base skin must come first: it defines the token set others may override.
export const skins: ResolvedSkin[] = resolveSkins([zerusDark, zerusLight, fjord, paper] as SkinFile[]);

const sheet = document.createElement("style");
sheet.id = "zerus-skins";
sheet.textContent = skins.map(compileSkin).join("\n");
document.head.prepend(sheet);

function initialSkin(): string {
  const fromUrl = new URLSearchParams(location.search).get("skin");
  if (fromUrl && skins.some((s) => s.id === fromUrl)) return fromUrl;
  try {
    const saved = localStorage.getItem("zerus-mock-skin");
    if (saved && skins.some((s) => s.id === saved)) return saved;
  } catch {
    /* storage may be unavailable */
  }
  return matchMedia("(prefers-color-scheme: light)").matches ? "zerus-light" : "zerus-dark";
}

const [skinId, setSkinIdRaw] = createSignal(initialSkin());
export { skinId };

/** Reactive list of built-in plus imported skins. */
const [skinListSignal, setSkinList] = createSignal<ResolvedSkin[]>([...skins]);
export const skinList = skinListSignal;
export const skin = createMemo(() => skinListSignal().find((s) => s.id === skinId()) ?? skins[0]);

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

export function setSkin(id: string) {
  setSkinIdRaw(id);
  try {
    localStorage.setItem("zerus-mock-skin", id);
  } catch {
    /* ignore */
  }
}

/** Reflect the active skin's tokens and L1 variants onto the root element. */
export function applySkin(s: ResolvedSkin) {
  const root = document.documentElement;
  root.dataset.skin = s.id;
  root.dataset.appearance = s.appearance;
  for (const [name, value] of Object.entries(s.components)) root.dataset[camel(name)] = value;
}

const camel = (name: string) => name.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase());
