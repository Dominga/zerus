import { createEffect, createRoot } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { settingsReady } from "../store/settings";
import { applySkin, skin } from "./runtime";

type Rgb = [number, number, number];

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

/** The first native show waits for both the saved selection and skin library.
 * Later theme/system/skin changes repaint the native surfaces without showing
 * a hidden window. No network or fleet readiness is part of this handshake. */
export async function startWindowTheme(library: Promise<unknown>): Promise<void> {
  const generation = (window as Window & { __ZERUS_WINDOW_THEME__?: string }).__ZERUS_WINDOW_THEME__;
  if (!generation) return;
  await Promise.all([settingsReady, library]);
  let revision = 0;
  createRoot(() => createEffect(() => {
    applySkin(skin());
    void invoke("window_theme", { generation, revision: ++revision, background: background() })
      .catch(error => console.error("Zerus Next: window theme could not be applied", error));
  }));
}
