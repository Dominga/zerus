import { createStore } from "solid-js/store";

// Client preferences the Qt GUI keeps in QSettings, with its keys and defaults.
// Until Settings and the QSettings migration move in (zerus-5wmh.8) they live
// per user in the webview's local storage: one JSON object keyed by the
// QSettings names, so the migration reads and writes the same keys.
//
//   workspace/animateActivity  "Animate working sessions" (Settings > Sessions,
//                              tray/src/SettingsPage.h), default on
//   workspace/reduceMotion     "Reduce motion", default off; Qt reads it
//                              (SessionsWindow.cpp) but offers no control.
//                              Qt does not consult the system preference;
//                              the client honours it separately (ui/motion.ts).

export interface Settings {
  animateActivity: boolean;
  reduceMotion: boolean;
}

export type SettingName = keyof Settings;

/** The QSettings key and default of each setting. */
export const SETTINGS: { readonly [K in SettingName]: { readonly key: string; readonly fallback: Settings[K] } } = {
  animateActivity: { key: "workspace/animateActivity", fallback: true },
  reduceMotion: { key: "workspace/reduceMotion", fallback: false },
};

/** The local storage entry holding every setting. */
export const STORAGE_KEY = "zerus-client-settings";

export interface SettingsStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const NAMES = Object.keys(SETTINGS) as SettingName[];

export const defaultSettings = (): Settings => ({ animateActivity: SETTINGS.animateActivity.fallback, reduceMotion: SETTINGS.reduceMotion.fallback });

/** The stored object, or an empty one when it is missing or not a JSON object. */
function storedObject(raw: string | null | undefined): Record<string, unknown> {
  if (!raw) return {};
  try {
    const value: unknown = JSON.parse(raw);
    return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

/** Settings from the stored text; a missing or malformed value takes its Qt default. */
export function readSettings(raw: string | null | undefined): Settings {
  const stored = storedObject(raw);
  const settings = defaultSettings();
  for (const name of NAMES) {
    const value = stored[SETTINGS[name].key];
    if (typeof value === "boolean") settings[name] = value;
  }
  return settings;
}

/** The stored text with `settings` written in; keys this build does not know are kept. */
export function writeSettings(raw: string | null | undefined, settings: Settings): string {
  const stored = storedObject(raw);
  for (const name of NAMES) stored[SETTINGS[name].key] = settings[name];
  return JSON.stringify(stored);
}

function browserStorage(): SettingsStorage | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    return undefined; // storage can be disabled
  }
}

/**
 * A reactive settings store over `storage`. `set` writes through unless
 * `persist` is false, which a measurement run uses to try a value for a while
 * without changing what the user chose.
 */
export function createSettings(storage: SettingsStorage | undefined = browserStorage()) {
  const load = () => { try { return storage?.getItem(STORAGE_KEY) ?? null; } catch { return null; } };
  const [settings, setSettings] = createStore<Settings>(readSettings(load()));
  // What the user chose; a value tried without persisting never reaches storage.
  const chosen = { ...settings };
  function set<K extends SettingName>(name: K, value: Settings[K], options: { persist?: boolean } = {}) {
    setSettings(name, value as Settings[SettingName]);
    if (options.persist === false) return;
    chosen[name] = value;
    try { storage?.setItem(STORAGE_KEY, writeSettings(load(), chosen)); } catch { /* storage may be unavailable or full */ }
  }
  /** Back to what the user chose, after values tried without persisting. */
  const restore = () => { for (const name of NAMES) setSettings(name, chosen[name]); };
  return { settings, set, restore };
}

const store = createSettings();

/** The client's settings, reactive. */
export const settings = store.settings;
/** Changes one setting and stores it (unless `persist: false`). */
export const setSetting = store.set;
/** Undoes values tried with `persist: false`. */
export const restoreSettings = store.restore;
