import { afterEach, beforeEach, expect, it, vi } from "vitest";
import dark from "./skins/zerus-dark.json";
import light from "./skins/zerus-light.json";

const port = vi.hoisted(() => ({
  invoke: vi.fn().mockResolvedValue(undefined),
  ready: Promise.resolve(),
  selected: { appearance: "dark", tokens: { color: { page: "#000000" } } },
  applied: "",
  effects: [] as (() => void)[],
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: port.invoke }));
vi.mock("../store/settings", () => ({ get settingsReady() { return port.ready; } }));
vi.mock("./runtime", () => ({
  skin: () => port.selected,
  applySkin: (skin: typeof port.selected) => { port.applied = skin.tokens.color.page; },
}));
vi.mock("solid-js", () => ({
  createRoot: (run: () => void) => run(),
  createEffect: (run: () => void) => { port.effects.push(run); run(); },
}));

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  port.effects = [];
  port.ready = Promise.resolve();
  port.selected = dark;
  port.applied = "";
  vi.stubGlobal("window", { __ZERUS_WINDOW_THEME__: "original-generation" });
  vi.stubGlobal("getComputedStyle", () => ({ backgroundColor: port.applied }));
  vi.stubGlobal("document", {
    body: {},
    createElement: () => ({ getContext: () => ({
      fillStyle: "", fillRect: () => {},
      getImageData: () => ({ data: [...port.applied.slice(1).match(/../g)!.map(n => parseInt(n, 16)), 255] }),
    }) }),
  });
});
afterEach(() => { vi.useRealTimers(); });

it("does not release first show until both the persisted theme and imported library are ready", async () => {
  let settings!: () => void, library!: () => void;
  port.ready = new Promise<void>(resolve => { settings = resolve; });
  const loaded = new Promise<void>(resolve => { library = resolve; });
  const { startWindowTheme } = await import("./window");
  const started = startWindowTheme(loaded);
  settings();
  await Promise.resolve();
  expect(port.invoke).not.toHaveBeenCalled();
  // The selected imported page color becomes available only with the library.
  port.selected = { appearance: "dark", tokens: { color: { page: "#152637" } } };
  library();
  await started;
  expect(port.invoke).toHaveBeenCalledExactlyOnceWith("window_theme", {
    generation: "original-generation", revision: 1, background: [21, 38, 55],
  });
});

it("updates exact dark and light page colors with increasing revisions", async () => {
  const { startWindowTheme } = await import("./window");
  await startWindowTheme(Promise.resolve());
  expect(port.invoke.mock.calls[0][1].background).toEqual([22, 27, 33]);
  port.selected = light;
  port.effects[0](); // a saved or system appearance change
  expect(port.invoke.mock.calls[1][1]).toEqual({
    generation: "original-generation", revision: 2,
    background: light.tokens.color.page.slice(1).match(/../g)!.map(n => parseInt(n, 16)),
  });
});

it("browser pages do not send native presentation commands", async () => {
  vi.stubGlobal("window", {});
  const { startWindowTheme } = await import("./window");
  await startWindowTheme(Promise.resolve());
  expect(port.invoke).not.toHaveBeenCalled();
});

it.each(["settings", "library", "both"])("bounds a hung %s input and applies the current theme hint", async (hung) => {
  vi.useFakeTimers();
  const pending = new Promise<void>(() => {});
  port.ready = hung === "library" ? Promise.resolve() : pending;
  const { startWindowTheme } = await import("./window");
  const started = startWindowTheme(hung === "settings" ? Promise.resolve() : pending);
  await vi.advanceTimersByTimeAsync(799);
  expect(port.invoke).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  await started;
  expect(port.invoke).toHaveBeenCalledExactlyOnceWith("window_theme", {
    generation: "original-generation", revision: 1, background: [22, 27, 33],
  });
});

it("settles rejected settings and skin input without stranding the window", async () => {
  let reject!: (reason: Error) => void;
  port.ready = new Promise<void>((_, failed) => { reject = failed; });
  const { startWindowTheme } = await import("./window");
  const started = startWindowTheme(Promise.reject(new Error("skin read failed")));
  reject(new Error("settings read failed"));
  await started;
  expect(port.invoke).toHaveBeenCalledTimes(1);
});

it("uses a light fallback when the engine has no 2d color context", async () => {
  port.selected = light;
  vi.stubGlobal("document", { body: {}, createElement: () => ({ getContext: () => null }) });
  const { startWindowTheme } = await import("./window");
  await startWindowTheme(Promise.resolve());
  expect(port.invoke.mock.calls[0][1].background).toEqual([245, 247, 249]);
});

it("settles a failed native invoke so the native show deadline can take over", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  port.invoke.mockRejectedValueOnce(new Error("native invoke unavailable"));
  const { startWindowTheme } = await import("./window");
  await expect(startWindowTheme(Promise.resolve())).resolves.toBeUndefined();
  expect(port.invoke).toHaveBeenCalledTimes(1);
  expect(error).toHaveBeenCalled();
  error.mockRestore();
});
