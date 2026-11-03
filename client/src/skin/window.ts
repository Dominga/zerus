import { createEffect, createRoot } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { settingsReady } from "../store/settings";
import { applySkin, skin } from "./runtime";

type Rgb = [number, number, number];
const HANDSHAKE_MS = 800;

/** Resolve CSS colors through the engine, including imported skins and color-mix. */
function background(): Rgb {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 1;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Window theme color is unavailable");
  context.fillStyle = getComputedStyle(document.body).backgroundColor;
  context.fillRect(0, 0, 1, 1);
  const [r, g, b] = context.getImageData(0, 0, 1, 1).data;
  return [r, g, b];
}

/** Allow up to 800 ms for the saved selection and skin library.
 * Later theme/system/skin changes repaint the native surfaces without showing
 * a hidden window. No network or fleet readiness is part of this handshake. */
export async function startWindowTheme(library: Promise<unknown>): Promise<void> {
  const generation = (window as Window & { __ZERUS_WINDOW_THEME__?: string }).__ZERUS_WINDOW_THEME__;
  if (!generation) return;
  let timer: ReturnType<typeof setTimeout> | undefined;
  await Promise.race([
    Promise.allSettled([settingsReady, library]),
    new Promise<void>(resolve => { timer = setTimeout(resolve, HANDSHAKE_MS); }),
  ]);
  clearTimeout(timer);
  let revision = 0;
  createRoot(() => createEffect(() => {
    const selected = skin();
    let color: Rgb;
    try {
      applySkin(selected);
      color = background();
    } catch {
      color = selected.appearance === "light" ? [245, 247, 249] : [22, 27, 33];
    }
    void invoke("window_theme", { generation, revision: ++revision, background: color })
      .catch(error => console.error("Zerus Next: window theme could not be applied", error));
  }));
}
