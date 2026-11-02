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
