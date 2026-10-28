import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import defaults from "./settings-defaults.json";

// The client's local preferences (Settings, phase 3b), under Qt's QSettings
// names with Qt's defaults. In the app they live in the core's store
// (<data_dir>/settings.json): read with `settings_get`, written with
// `settings_set`, which answers with the value in effect (clamped as Qt
// clamps it), and announced as `settings-changed`. The defaults come from the
// core's key table (settings-defaults.json, kept equal by a core test).
// Outside Tauri (the browser dev mode) they fall back to local storage.
//
// A measurement run may try a value without storing it (`persist: false`);
// `restoreSettings` returns to what the user chose.

export type SettingValue = boolean | number | string | string[];
export type Settings = Record<string, SettingValue>;

/** Every setting's default, by its QSettings name. */
export const DEFAULTS: Readonly<Settings> = defaults;

/** Local storage of the browser dev mode (and of builds before the core store). */
export const STORAGE_KEY = "zerus-client-settings";

export interface SettingsStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** Where settings live: the core store in the app, local storage outside it. */
export interface SettingsBackend {
  /** Every setting as the store has it. */
  load(): Promise<Settings>;
  /** Stores one setting; the value in effect. */
  save(key: string, value: SettingValue | null): Promise<SettingValue>;
  /** Changes made elsewhere (another window, the Rust side). */
  listen(onChange: (key: string, value: SettingValue) => void): void;
}

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

/** A value of the same type as the setting's default, or undefined. */
export function sameType(key: string, value: unknown): SettingValue | undefined {
  const fallback = DEFAULTS[key];
  if (fallback === undefined || value === null || value === undefined) return undefined;
  if (Array.isArray(fallback)) return Array.isArray(value) && value.every((item) => typeof item === "string") ? value as string[] : undefined;
  return typeof value === typeof fallback ? value as SettingValue : undefined;
}

/** The settings of the browser dev mode, kept in local storage. */
export function storageBackend(storage: SettingsStorage | undefined): SettingsBackend {
  const read = () => { try { return storedObject(storage?.getItem(STORAGE_KEY)); } catch { return {}; } };
  return {
    load: async () => {
      const stored = read();
      const values: Settings = {};
      for (const [key, value] of Object.entries(stored)) {
        const typed = sameType(key, value);
        if (typed !== undefined) values[key] = typed;
      }
      return values;
    },
    save: async (key, value) => {
      const stored = read();
      if (value === null) delete stored[key];
      else stored[key] = value;
      try { storage?.setItem(STORAGE_KEY, JSON.stringify(stored)); } catch { /* storage may be unavailable or full */ }
      return value ?? DEFAULTS[key];
    },
    listen: () => {},
  };
}

/**
 * The values local storage held before the core store, to move once: the
 * settings object and the Overview and Activity widths. They are removed
 * after the move.
 */
export function takeLegacy(storage: SettingsStorage | undefined): Settings {
  const moved: Settings = {};
  try {
    const stored = storedObject(storage?.getItem(STORAGE_KEY));
    for (const [key, value] of Object.entries(stored)) {
      const typed = sameType(key, value);
      if (typed !== undefined) moved[key] = typed;
    }
    const width = Number(storage?.getItem(OVERVIEW_WIDTH_KEY));
    if (Number.isFinite(width) && width > 0) moved["client/overviewWidth"] = Math.round(width);
    const column = Number(storage?.getItem(ACTIVITY_WIDTH_KEY));
    if (Number.isFinite(column) && column > 0) moved["workspace/activityWidth"] = Math.round(column);
    storage?.removeItem(STORAGE_KEY);
    storage?.removeItem(OVERVIEW_WIDTH_KEY);
    storage?.removeItem(ACTIVITY_WIDTH_KEY);
  } catch {
    /* storage may be unavailable */
  }
  return moved;
}

/** Overview's column width before the core store (Overview.tsx). */
export const OVERVIEW_WIDTH_KEY = "zerus-client-overview-width";
/** Activity's column width before the core store (Activity.tsx). */
export const ACTIVITY_WIDTH_KEY = "zerus-client-activity-width";

/**
 * A reactive settings store over `backend`, starting from the defaults until
 * the backend's values arrive. `moves` are written once before loading.
 */
export function createSettings(backend: SettingsBackend, moves: Settings = {}) {
  const [settings, setSettings] = createStore<Settings>({ ...DEFAULTS });
  const [loaded, setLoaded] = createSignal(false);
  // What the user chose, and the keys holding a value tried without storing.
  const chosen: Settings = { ...DEFAULTS };
  const tried = new Set<string>();

  const apply = (key: string, value: SettingValue) => {
    chosen[key] = value;
    if (!tried.has(key)) setSettings(key, value);
  };

  async function set(key: string, value: SettingValue | null, options: { persist?: boolean } = {}) {
    if (options.persist === false) {
      if (value === null) return;
      tried.add(key);
      setSettings(key, value);
      return;
    }
    tried.delete(key);
    const before = chosen[key];
    if (value !== null) setSettings(key, value);
    try {
      apply(key, await backend.save(key, value));
    } catch (error) {
      // The store refused it (an unreadable settings file): keep what it has.
      console.warn(`settings: ${key} not saved: ${String(error)}`);
      if (before !== undefined) setSettings(key, before);
    }
  }

  /** Back to what the user chose, after values tried with `persist: false`. */
  function restore() {
    for (const key of tried) setSettings(key, chosen[key]);
    tried.clear();
  }

  const ready = (async () => {
    backend.listen((key, value) => apply(key, value));
    for (const [key, value] of Object.entries(moves)) {
      try { await backend.save(key, value); } catch { /* keeps the store's value */ }
    }
    try {
      const values = await backend.load();
      for (const [key, value] of Object.entries(values)) apply(key, value);
    } catch (error) {
      console.warn(`settings: not loaded: ${String(error)}`);
    }
    setLoaded(true);
  })();

  return { settings, set, restore, loaded, ready };
}

function browserStorage(): SettingsStorage | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    return undefined; // storage can be disabled
  }
}

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** The core store through the Tauri commands. */
function tauriBackend(): SettingsBackend {
  return {
    load: () => invoke<Settings>("settings_get"),
    save: (key, value) => invoke<SettingValue>("settings_set", { key, value }),
    listen: (onChange) => {
      void listen<{ key: string; value: SettingValue }>("settings-changed", (e) => onChange(e.payload.key, e.payload.value));
    },
  };
}

const store = inTauri()
  ? createSettings(tauriBackend(), takeLegacy(browserStorage()))
  : createSettings(storageBackend(browserStorage()));

/** The client's settings by QSettings name, reactive. */
export const settings = store.settings;
/** Changes one setting and stores it (unless `persist: false`); `null` removes a shortcut override. */
export const setSetting = store.set;
/** Undoes values tried with `persist: false`. */
export const restoreSettings = store.restore;
/** Whether the stored values have arrived. */
export const settingsLoaded = store.loaded;
/** Resolves once the stored values have arrived. */
export const settingsReady = store.ready;

export const flag = (key: string): boolean => settings[key] === true;
export const num = (key: string): number => { const value = settings[key]; return typeof value === "number" ? value : Number(DEFAULTS[key] ?? 0); };
export const text = (key: string): string => { const value = settings[key]; return typeof value === "string" ? value : String(DEFAULTS[key] ?? ""); };
