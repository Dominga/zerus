import { createRoot, createSignal } from "solid-js";
import { describe, expect, it } from "vitest";
import { createFavorites, favoriteId, parseFavorites, MAX_FAVORITES } from "./favorites";

const MACHINE = "11111111-1111-4111-8111-111111111111";
const SESSION = "22222222-2222-4222-8222-222222222222";
const ID = `${MACHINE}\n${SESSION}`;
const tick = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
describe("local durable favorites", () => {
  it("requires canonical computer and durable session identity, never a name or row key", () => {
    expect(favoriteId(MACHINE, SESSION)).toBe(ID);
    for (const [machine, session] of [[undefined, SESSION], [MACHINE, undefined], ["arch", SESSION], [MACHINE, "codex/p/title"]])
      expect(favoriteId(machine, session)).toBeUndefined();
  });
  it("reads a bounded, deduplicated set of exact identities", () => {
    expect(parseFavorites(JSON.stringify([ID, ID, 7, `${ID}\nextra`, "name:reused"]))).toEqual([ID]);
    expect(parseFavorites("not json")).toEqual([]);
    expect(parseFavorites(JSON.stringify({ [ID]: true }))).toEqual([]);
    const many = Array.from({ length: MAX_FAVORITES + 1 }, (_, n) => `${MACHINE}\n${String(n).padStart(8, "0")}-2222-4222-8222-222222222222`);
    expect(parseFavorites(JSON.stringify(many))).toEqual(many.slice(1));
  });
  it("persists an add and removal across a fresh store (GUI restart)", async () => {
    await createRoot(async (dispose) => {
      const [value, setValue] = createSignal<unknown>("[]");
      const backend = { value, ready: Promise.resolve(), save: async (v: string) => { setValue(v); return true; } };
      const first = createFavorites(backend); first.set(ID, true); await tick();
      const restarted = createFavorites(backend); expect(restarted.has(ID)).toBe(true);
      restarted.set(ID, false); await tick();
      expect(createFavorites(backend).has(ID)).toBe(false); dispose();
    });
  });
  it("merges an early choice with loaded preferences, without losing earlier favorites", async () => {
    await createRoot(async (dispose) => {
      let ready!: () => void; const [value, setValue] = createSignal<unknown>("[]");
      const saved = `${SESSION}\n${MACHINE}`;
      const store = createFavorites({ value, ready: new Promise<void>((r) => { ready = r; }), save: async (v) => { setValue(v); return true; } });
      store.set(ID, true); expect(store.has(ID)).toBe(true);
      setValue(JSON.stringify([saved])); ready(); await tick();
      expect(parseFavorites(value())).toEqual([saved, ID]); dispose();
    });
  });
  it("serializes changes made while a save is held and never restores a removed favorite", async () => {
    await createRoot(async (dispose) => {
      const [value, setValue] = createSignal<unknown>("[]"); let release!: () => void; let calls = 0;
      const store = createFavorites({ value, ready: Promise.resolve(), save: async (v) => { if (++calls === 1) await new Promise<void>((r) => { release = r; }); setValue(v); return true; } });
      store.set(ID, true); await tick(); store.set(ID, false); release(); await tick();
      expect(store.has(ID)).toBe(false); expect(parseFavorites(value())).toEqual([]); dispose();
    });
  });
  it("does not transfer a choice to the same session UUID on another computer", async () => {
    createRoot((dispose) => {
      const store = createFavorites({ value: () => JSON.stringify([ID]), ready: Promise.resolve(), save: async () => true });
      expect(store.has(favoriteId(SESSION, SESSION))).toBe(false); dispose();
    });
  });
});
