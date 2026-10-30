import { describe, expect, it } from "vitest";
import source from "../../src-tauri/src/platform/kwin_window_layer.js?raw";
import { createWindowLayer, LAYER_UNAVAILABLE, type WindowLayer } from "./windowLayer";

const initial: WindowLayer = { supported: true, on: false, busy: false, hint: "normal", revision: 1 };
describe("one confirmed window layer", () => {
  it("follows native changes, ignores stale loads, and keeps controls stable during a request", async () => {
    let receive!: (view: WindowLayer) => void;
    let answer!: (ok: boolean) => void;
    const saves: boolean[] = [];
    const layer = createWindowLayer({ load: async () => initial, listen: async (next) => { receive = next; next({ ...initial, on: true, revision: 2 }); return () => {}; },
      save: (on) => { saves.push(on); return new Promise((resolve) => { answer = resolve; }); }, error: () => { throw Error("unexpected refusal"); } });
    await layer.ready;
    expect(layer.view().on).toBe(true);
    const pending = layer.set(false);
    await layer.set(false);
    expect(saves).toEqual([false]);
    expect(layer.view().supported).toBe(true);
    expect(layer.view().on).toBe(true); // no optimistic pin or toast
    receive({ ...initial, revision: 3 }); answer(true); await pending;
    expect(layer.view().on).toBe(false);
    receive({ ...initial, on: true, revision: 2 });
    expect(layer.view().on).toBe(false);
    layer.dispose();
  });
  it("explains unsupported layers and reports a native refusal with the confirmed hint", async () => {
    let receive!: (view: WindowLayer) => void;
    const errors: string[] = [];
    let writes = 0;
    const layer = createWindowLayer({ load: async () => ({ ...initial, supported: false, hint: LAYER_UNAVAILABLE }),
      listen: async (next) => { receive = next; return () => {}; }, save: async () => { writes++; receive({ ...initial, hint: "KWin refused", revision: 3 }); return false; }, error: (text) => errors.push(text) });
    await layer.ready; await layer.set(true); expect(writes).toBe(0);
    receive({ ...initial, revision: 2 }); await layer.set(true);
    expect(errors).toEqual(["KWin refused"]); expect(layer.view().on).toBe(false);
  });
});

const window = () => ({ pid: 123, resourceClass: "org.example.Zerus", caption: "Zerus test", normalWindow: true, dialog: false, deleted: false, internalId: "owned-id", keepAbove: false });
const config = { pid: 123, appId: "org.example.Zerus", caption: "Zerus test", id: "", desired: true, service: ":1.12", path: "/test", token: "nonce", plugin: "temporary", kwin: ":1.10" };
function run(windows: object[], extra = {}) {
  const calls: unknown[][] = [];
  new Function("workspace", "callDBus", `const config = ${JSON.stringify({ ...config, ...extra })};\n${source}`)({ windowList: () => windows }, (...args: unknown[]) => calls.push(args));
  expect(calls[1]).toEqual([":1.10", "/Scripting", "org.kde.kwin.Scripting", "unloadScript", "temporary"]);
  return calls[0].slice(5);
}
describe("Qt KWin window scope", () => {
  it.each([{ pid: 124 }, { resourceClass: "other" }, { dialog: true }, { deleted: true }, { normalWindow: false }, { caption: "other" }])("never changes a foreign or non-normal window %j", (change) => {
    const foreign = { ...window(), ...change };
    expect(run([foreign])).toEqual(["", false, "missing"]);
    expect(foreign.keepAbove).toBe(false);
  });
  it("refuses ambiguous matches and binds the confirmed UUID after the caption changes", () => {
    expect(run([window(), window()])).toEqual(["", false, "ambiguous"]);
    const owned = { ...window(), caption: "changed title" };
    expect(run([owned], { id: "owned-id" })).toEqual(["owned-id", true, ""]);
    expect(owned.keepAbove).toBe(true);
  });
  it("quotes captions literally, probes without changing state, and confirms a forced KDE rule", () => {
    const caption = "literal\"; throw Error('injection'); //\n";
    expect(run([{ ...window(), caption }], { caption, desired: null })).toEqual(["owned-id", false, ""]);
    const forced = window(); Object.defineProperty(forced, "keepAbove", { get: () => false, set: () => {} });
    expect(run([forced])).toEqual(["owned-id", false, "denied"]);
  });
});
