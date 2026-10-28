import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { SETTINGS, STORAGE_KEY, createSettings, defaultSettings, readSettings, writeSettings, type SettingsStorage } from "./settings";

/** An in-memory storage that records every write. */
function memory(initial?: string) {
  const items = new Map<string, string>();
  if (initial !== undefined) items.set(STORAGE_KEY, initial);
  const writes: string[] = [];
  const storage: SettingsStorage = {
    getItem: (key) => items.get(key) ?? null,
    setItem: (key, value) => { items.set(key, value); writes.push(value); },
  };
  return { storage, writes, stored: () => items.get(STORAGE_KEY) };
}

describe("settings keys and defaults", () => {
  it("uses the Qt keys and defaults: Animate working sessions on, Reduce motion off", () => {
    expect(SETTINGS.animateActivity).toEqual({ key: "workspace/animateActivity", fallback: true });
    expect(SETTINGS.reduceMotion).toEqual({ key: "workspace/reduceMotion", fallback: false });
    expect(defaultSettings()).toEqual({ animateActivity: true, reduceMotion: false });
  });
});

describe("readSettings", () => {
  it("falls back to the defaults for missing, malformed or non-object storage", () => {
    for (const raw of [null, undefined, "", "not json", "[]", "null", "42", '"text"']) expect(readSettings(raw)).toEqual(defaultSettings());
  });

  it("reads stored booleans by their QSettings keys and ignores values of another type", () => {
    expect(readSettings(JSON.stringify({ "workspace/animateActivity": false, "workspace/reduceMotion": true }))).toEqual({ animateActivity: false, reduceMotion: true });
    expect(readSettings(JSON.stringify({ "workspace/animateActivity": "false", "workspace/reduceMotion": 1 }))).toEqual(defaultSettings());
    expect(readSettings(JSON.stringify({ animateActivity: false }))).toEqual(defaultSettings());
  });
});

describe("writeSettings", () => {
  it("writes every setting under its key and keeps keys it does not know", () => {
    const raw = JSON.stringify({ "workspace/theme": "dark", "workspace/reduceMotion": false });
    expect(JSON.parse(writeSettings(raw, { animateActivity: false, reduceMotion: true }))).toEqual({
      "workspace/theme": "dark", "workspace/animateActivity": false, "workspace/reduceMotion": true,
    });
    expect(JSON.parse(writeSettings("garbage", defaultSettings()))).toEqual({ "workspace/animateActivity": true, "workspace/reduceMotion": false });
  });
});

describe("createSettings", () => {
  it("starts from storage and stores each change", () => {
    createRoot((dispose) => {
      const m = memory(JSON.stringify({ "workspace/reduceMotion": true, "workspace/compact": true }));
      const { settings, set } = createSettings(m.storage);
      expect(settings.reduceMotion).toBe(true);
      expect(settings.animateActivity).toBe(true);
      set("animateActivity", false);
      expect(settings.animateActivity).toBe(false);
      expect(JSON.parse(m.stored()!)).toEqual({ "workspace/reduceMotion": true, "workspace/compact": true, "workspace/animateActivity": false });
      // A later instance (the next start) reads what was stored.
      expect(createSettings(m.storage).settings.animateActivity).toBe(false);
      dispose();
    });
  });

  it("tries a value without storing it and restores the chosen one", () => {
    createRoot((dispose) => {
      const m = memory();
      const { settings, set, restore } = createSettings(m.storage);
      set("reduceMotion", true, { persist: false });
      expect(settings.reduceMotion).toBe(true);
      expect(m.writes).toEqual([]);
      // A stored change meanwhile does not carry the tried value along.
      set("animateActivity", false);
      expect(JSON.parse(m.stored()!)).toEqual({ "workspace/animateActivity": false, "workspace/reduceMotion": false });
      restore();
      expect({ ...settings }).toEqual({ animateActivity: false, reduceMotion: false });
      dispose();
    });
  });

  it("works without storage or when storage throws", () => {
    createRoot((dispose) => {
      const none = createSettings(undefined);
      expect({ ...none.settings }).toEqual(defaultSettings());
      none.set("reduceMotion", true);
      expect(none.settings.reduceMotion).toBe(true);
      const broken: SettingsStorage = { getItem: () => { throw new Error("denied"); }, setItem: () => { throw new Error("full"); } };
      const failing = createSettings(broken);
      expect({ ...failing.settings }).toEqual(defaultSettings());
      expect(() => failing.set("animateActivity", false)).not.toThrow();
      expect(failing.settings.animateActivity).toBe(false);
      dispose();
    });
  });
});
