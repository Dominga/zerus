import { afterEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
  const values = new Map<string, string>();
  vi.stubGlobal("location", { search: "", hash: "", href: "http://localhost/" });
  vi.stubGlobal("localStorage", { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value), removeItem: (key: string) => values.delete(key) });
  const document = { visibilityState: "visible", addEventListener: () => {}, removeEventListener: () => {} };
  vi.stubGlobal("document", document);
  vi.stubGlobal("window", { document, addEventListener: () => {}, removeEventListener: () => {} });
});

import { bindMenuBarrier, menu, setMenu, visibleMenu } from "./ui";

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
function barrier() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  bindMenuBarrier(() => promise);
  return resolve;
}
afterEach(() => { bindMenuBarrier(undefined); setMenu(undefined); });

describe("native child menu admission", () => {
  it.each([false, true])("waits for both final items and native hiding (items first=%s)", async (itemsFirst) => {
    const hidden = barrier();
    let prepared!: (items: { label: string; disabled: boolean }[]) => void;
    const ready = new Promise<{ label: string; disabled: boolean }[]>((resolve) => { prepared = resolve; });
    const initial = { x: 20, y: 40, sessionKey: "pending-context", items: [{ label: "Open folder", disabled: true }] };
    setMenu(initial, ready);
    expect(menu()).toBe(initial); expect(visibleMenu()).toBeUndefined();
    const final = [{ label: "Open folder", disabled: false }];
    if (itemsFirst) prepared(final); else hidden();
    await tick(); expect(visibleMenu()).toBeUndefined();
    if (itemsFirst) hidden(); else prepared(final);
    await tick();
    expect(visibleMenu()?.items).toBe(final);
    expect(menu()).toBe(visibleMenu());
    expect(menu()?.sessionKey).toBe("pending-context");
  });

  it.each([false, true])("discards final items after close or replacement (replace=%s)", async (replace) => {
    let prepared!: (items: { label: string }[]) => void;
    const ready = new Promise<{ label: string }[]>((resolve) => { prepared = resolve; });
    setMenu({ x: 20, y: 40, items: [] }, ready);
    const replacement = replace ? { x: 40, y: 60, items: [{ label: "New target" }] } : undefined;
    setMenu(replacement);
    prepared([{ label: "Old target" }]); await tick();
    expect(menu()).toBe(replacement); expect(visibleMenu()).toBe(replacement);
  });

  it("retains the painted snapshot during a scope update's native barrier", async () => {
    const initial = { x: 10, y: 20, items: [{ label: "Computer (1)" }] };
    setMenu(initial);
    const hidden = barrier();
    const updated = { ...initial, items: [{ label: "Computer (2)" }] };
    setMenu(updated);
    expect(menu()).toBe(updated); expect(visibleMenu()).toBe(initial);
    hidden(); await tick();
    expect(visibleMenu()).toBe(updated);
  });

  it("retains logical menu identity and independent read updates while hiding the child", async () => {
    const hidden = barrier();
    const initial = { x: 10, y: 20, sessionKey: "context-target", items: [{ label: "Fork session…", disabled: true }] };
    setMenu(initial);
    expect(menu()).toBe(initial);
    expect(visibleMenu()).toBeUndefined();
    const updated = { ...menu()!, items: [{ label: "Fork session…", disabled: false }, { label: "Open folder" }] };
    setMenu(updated);
    expect(menu()).toBe(updated);
    expect(visibleMenu()).toBeUndefined();
    hidden(); await tick();
    expect(visibleMenu()).toBe(updated);
    expect(visibleMenu()?.sessionKey).toBe("context-target");
    setMenu(undefined);
    expect(visibleMenu()?.sessionKey).toBeUndefined();
  });

  it("never paints a menu closed or replaced before the child acknowledged hiding", async () => {
    const hidden = barrier();
    setMenu({ x: 10, y: 20, items: [{ label: "Old session" }] });
    setMenu(undefined);
    hidden(); await tick();
    expect(menu()).toBeUndefined();
    expect(visibleMenu()).toBeUndefined();
  });
});
