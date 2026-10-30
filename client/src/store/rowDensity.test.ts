import { afterEach, describe, expect, it } from "vitest";
import { createComputed, createRoot, createSignal } from "solid-js";
import { createRowDensity, MAX_COMPACT_ROWS } from "./rowDensity";

const exact = "a".repeat(64), fallback = "b".repeat(64), unrelated = "c".repeat(64);
const settle = async () => { await Promise.resolve(); await Promise.resolve(); };
function port(initial: string[], ready = Promise.resolve()) {
  const [saved, setSaved] = createSignal(initial);
  const writes: string[][] = [];
  return { stored: saved, ready, keys: (row: string) => row === "unknown" ? undefined : [exact, fallback] as const,
    save: async (keys: string[]) => { setSaved(keys); writes.push(keys); return true; }, writes };
}
const disposers: (() => void)[] = [];
const density = (backend: ReturnType<typeof port>) => createRoot((dispose) => {
  disposers.push(dispose); return createRowDensity(backend);
});
afterEach(() => { for (const dispose of disposers.splice(0)) dispose(); });

describe("Qt row-density migration", () => {
  it("uses exact hashes and upgrades only a matching observed fallback", async () => {
    const backend = port([fallback, unrelated]);
    const rows = density(backend);
    expect(rows.get("row")).toBe(true);
    expect(rows.get("unknown")).toBe(false);
    await settle();
    expect(backend.writes.at(-1)).toEqual([exact, unrelated]);
    rows.set("row", false);
    expect(rows.get("row")).toBe(false);
    await settle();
    expect(backend.writes.at(-1)).toEqual([unrelated]);
  });
  it("waits for settings before saving and preserves all other imported keys", async () => {
    let loaded!: () => void;
    const backend = port([unrelated], new Promise<void>((resolve) => { loaded = resolve; }));
    const rows = density(backend);
    rows.set("row", true);
    expect(rows.get("row")).toBe(true);
    expect(backend.writes).toHaveLength(0);
    loaded(); await settle();
    expect(backend.writes.at(-1)).toEqual([exact, unrelated]);
  });
  it("expands hidden/unloaded hashes too and keeps later user choices", async () => {
    const backend = port([unrelated]);
    const rows = density(backend);
    expect(rows.any()).toBe(true);
    rows.expandAll();
    expect(rows.any()).toBe(false);
    await settle();
    expect(backend.writes.at(-1)).toEqual([]);
    rows.set("row", true); await settle();
    expect(backend.writes.at(-1)).toEqual([exact]);
  });
  it("retains a bounded ephemeral choice without inventing an identity", async () => {
    const backend = port([]);
    const rows = density(backend);
    rows.set("unknown", true); await settle();
    expect(rows.get("unknown")).toBe(true);
    expect(backend.writes).toHaveLength(0);
    rows.expandAll(); await settle();
    expect(rows.get("unknown")).toBe(false);
  });
  it("does not retry a refused fallback upgrade on every render", async () => {
    const backend = port([fallback]);
    backend.save = async (keys) => { backend.writes.push(keys); return false; };
    const rows = density(backend);
    expect(rows.get("row")).toBe(true); await settle();
    expect(rows.get("row")).toBe(true); await settle();
    expect(backend.writes).toHaveLength(1);
  });
  it("updates an observed row synchronously before its save starts", () => {
    const backend = port([], new Promise(() => {}));
    const seen: boolean[] = [];
    createRoot((dispose) => {
      disposers.push(dispose);
      const rows = createRowDensity(backend);
      createComputed(() => { seen.push(rows.get("row")); });
      rows.set("row", true);
      rows.set("row", false);
    });
    expect(seen).toEqual([false, true, false]);
    expect(backend.writes).toHaveLength(0);
  });
  it("refuses another collapsed row at the bound without dropping saved hashes", async () => {
    const hashes = Array.from({ length: MAX_COMPACT_ROWS }, (_, n) => n.toString(16).padStart(64, "0"));
    const backend = port(hashes);
    const rows = density(backend);
    rows.set("row", true); await settle();
    expect(rows.get("row")).toBe(false);
    expect(backend.writes).toHaveLength(0);
    expect(backend.stored()).toEqual(hashes);
  });
});
