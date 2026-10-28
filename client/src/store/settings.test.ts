import { describe, expect, it, vi } from "vitest";
import {
  ACTIVITY_WIDTH_KEY, DEFAULTS, OVERVIEW_WIDTH_KEY, STORAGE_KEY, createSettings, sameType, storageBackend, takeLegacy,
  type SettingValue, type Settings, type SettingsBackend, type SettingsStorage,
} from "./settings";

function memoryStorage(initial: Record<string, string> = {}): SettingsStorage & { data: Record<string, string> } {
  const data = { ...initial };
  return {
    data,
    getItem: (key) => (key in data ? data[key] : null),
    setItem: (key, value) => { data[key] = value; },
    removeItem: (key) => { delete data[key]; },
  };
}

/** A core store stand-in: clamps like the core and records each save. */
function fakeCore(values: Settings = {}) {
  const saved: [string, SettingValue | null][] = [];
  let listener: ((key: string, value: SettingValue) => void) | undefined;
  let refuse = false;
  const backend: SettingsBackend = {
    load: async () => ({ ...DEFAULTS, ...values }),
    save: async (key, value) => {
      saved.push([key, value]);
      if (refuse) throw "Settings are not readable";
      if (key === "processes/activeSeconds" && typeof value === "number") return Math.max(1, Math.min(60, value));
      return value ?? DEFAULTS[key];
    },
    listen: (onChange) => { listener = onChange; },
  };
  return { backend, saved, emit: (key: string, value: SettingValue) => listener?.(key, value), refuse: () => { refuse = true; } };
}

describe("defaults", () => {
  it("are Qt's, by QSettings name, from the core's table", () => {
    expect(DEFAULTS["workspace/animateActivity"]).toBe(true);
    expect(DEFAULTS["workspace/reduceMotion"]).toBe(false);
    expect(DEFAULTS["workspace/theme"]).toBe("system");
    expect(DEFAULTS["workspace/contentScale"]).toBe(1);
    expect(DEFAULTS["processes/activeSeconds"]).toBe(2.5);
    expect(DEFAULTS["client/darkSkin"]).toBe("zerus-dark");
    expect(DEFAULTS["client/lightSkin"]).toBe("zerus-light");
    expect(DEFAULTS["client/overviewWidth"]).toBe(1200);
    expect("automaticApprovals/enabled" in DEFAULTS).toBe(false);
  });

  it("accept only values of the default's type", () => {
    expect(sameType("workspace/compact", true)).toBe(true);
    expect(sameType("workspace/compact", "true")).toBeUndefined();
    expect(sameType("workspace/contentScale", 1.5)).toBe(1.5);
    expect(sameType("no/such", true)).toBeUndefined();
  });
});

describe("the browser dev mode", () => {
  it("keeps typed values in local storage and drops the rest", async () => {
    const storage = memoryStorage({ [STORAGE_KEY]: JSON.stringify({ "workspace/theme": "dark", "workspace/compact": "yes", other: 1 }) });
    const backend = storageBackend(storage);
    expect(await backend.load()).toEqual({ "workspace/theme": "dark" });
    expect(await backend.save("workspace/compact", true)).toBe(true);
    expect(JSON.parse(storage.data[STORAGE_KEY])).toEqual({ "workspace/theme": "dark", "workspace/compact": true, other: 1 });
    await backend.save("workspace/theme", null);
    expect(JSON.parse(storage.data[STORAGE_KEY])["workspace/theme"]).toBeUndefined();
  });

  it("works without storage or when storage throws", async () => {
    expect(await storageBackend(undefined).load()).toEqual({});
    const throwing: SettingsStorage = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); }, removeItem: () => {} };
    expect(await storageBackend(throwing).load()).toEqual({});
    expect(await storageBackend(throwing).save("workspace/compact", true)).toBe(true);
  });
});

describe("the one-time move from local storage", () => {
  it("takes the old settings and the Overview and Activity widths once", () => {
    const storage = memoryStorage({
      [STORAGE_KEY]: JSON.stringify({ "workspace/animateActivity": false, "workspace/reduceMotion": true }),
      [OVERVIEW_WIDTH_KEY]: "1480",
      [ACTIVITY_WIDTH_KEY]: "1100",
    });
    expect(takeLegacy(storage)).toEqual({
      "workspace/animateActivity": false, "workspace/reduceMotion": true, "client/overviewWidth": 1480, "workspace/activityWidth": 1100,
    });
    expect(storage.data).toEqual({});
    expect(takeLegacy(storage)).toEqual({});
  });
});

describe("createSettings", () => {
  it("starts at the defaults, writes the moves first, then loads the store", async () => {
    const core = fakeCore({ "workspace/theme": "light" });
    const store = createSettings(core.backend, { "workspace/reduceMotion": true });
    expect(store.settings["workspace/theme"]).toBe("system");
    expect(store.loaded()).toBe(false);
    await store.ready;
    expect(core.saved).toEqual([["workspace/reduceMotion", true]]);
    expect(store.settings["workspace/theme"]).toBe("light");
    expect(store.loaded()).toBe(true);
  });

  it("shows a change at once and keeps the value the store put in effect", async () => {
    const core = fakeCore();
    const store = createSettings(core.backend);
    await store.ready;
    const saving = store.set("processes/activeSeconds", 0.2);
    expect(store.settings["processes/activeSeconds"]).toBe(0.2);
    await saving;
    expect(store.settings["processes/activeSeconds"]).toBe(1);
  });

  it("keeps the stored value when the store refuses a change", async () => {
    const core = fakeCore({ "workspace/theme": "dark" });
    const store = createSettings(core.backend);
    await store.ready;
    core.refuse();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    await store.set("workspace/theme", "light");
    warn.mockRestore();
    expect(store.settings["workspace/theme"]).toBe("dark");
  });

  it("tries a value without storing it and restores the chosen one", async () => {
    const core = fakeCore();
    const store = createSettings(core.backend);
    await store.ready;
    await store.set("workspace/animateActivity", false, { persist: false });
    expect(store.settings["workspace/animateActivity"]).toBe(false);
    expect(core.saved).toEqual([]);
    // A change from elsewhere waits while a value is tried.
    core.emit("workspace/animateActivity", true);
    expect(store.settings["workspace/animateActivity"]).toBe(false);
    store.restore();
    expect(store.settings["workspace/animateActivity"]).toBe(true);
  });

  it("follows changes made elsewhere", async () => {
    const core = fakeCore();
    const store = createSettings(core.backend);
    await store.ready;
    core.emit("client/overviewWidth", 1600);
    expect(store.settings["client/overviewWidth"]).toBe(1600);
  });
});
