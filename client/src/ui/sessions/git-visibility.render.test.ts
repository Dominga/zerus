// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { Row } from "./model";

const fixture = vi.hoisted(() => ({
  range: (): number => 0, setRange: (_n: number) => {},
  rows: () => [] as Row[],
  send: vi.fn().mockResolvedValue(undefined), hidden: false, visibilityChanged: () => {},
}));
vi.mock("../../bridge", async (original) => ({ ...await original<object>(), gitRows: fixture.send,
  windowVisibility: { hidden: () => fixture.hidden, onChange: (listener: () => void) => {
    fixture.visibilityChanged = listener; return () => { fixture.visibilityChanged = () => {}; };
  } },
}));
vi.mock("./model", async (original) => ({ ...await original<object>(), listRows: () => fixture.rows() }));
vi.mock("@tanstack/solid-virtual", async () => {
  const { createSignal } = await import("solid-js");
  const [range, setRange] = createSignal(0); fixture.range = range; fixture.setRange = setRange;
  return { createVirtualizer: (options: { count: number; getItemKey(n: number): string }) => ({
    measure() {}, _willUpdate() {}, scrollToIndex() {}, getTotalSize: () => options.count * 94,
    getVirtualItems: () => options.count ? [{ key: options.getItemKey(range()), start: range() * 94, index: range() }] : [],
  }) };
});
import { SessionList } from "./SessionList";
import { applyBatch, catalog } from "../../store/catalog";
import { emptyBatch } from "../../types";
import { setPage, setSelectedKey, setStripMode } from "../../store/ui";

let dispose: (() => void) | undefined;
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
afterEach(async () => {
  dispose?.(); dispose = undefined; await settle();
  document.body.replaceChildren(); vi.restoreAllMocks(); vi.unstubAllGlobals();
});
describe("the maintained session list Git interests", () => {
  it("prioritizes selection, follows the current viewport and drops it on hiding/navigation/disposal", async () => {
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    setPage("sessions"); setStripMode(false);
    applyBatch({ ...emptyBatch(), replaces: [{ computer: "", rows: [0, 1, 2].map((n) => ({
      computer: "", key: "run:fixture-" + n, row: {
        name: "codex/fixture/" + n, agent: "codex", status: "ready", state: "running", cwd: "/fixture/" + n, run_id: "fixture-" + n,
      },
    })) }] });
    fixture.rows = () => [0, 1, 2].map((n) => ({
      type: "card", id: "\nrun:fixture-" + n, session: catalog.rows["\nrun:fixture-" + n], expandable: false, expanded: false, compactRow: false,
    }));
    fixture.setRange(0); setSelectedKey("\nrun:fixture-2");
    const host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(SessionList, {}), host);
    await settle();
    expect(fixture.send.mock.calls.at(-1)?.[0]).toEqual([
      { computer: "", key: "run:fixture-2" }, { computer: "", key: "run:fixture-0" },
    ]);
    expect(host.querySelector('[data-key="\nrun:fixture-0"]')).not.toBeNull();
    fixture.setRange(1); await settle();
    expect(fixture.send.mock.calls.at(-1)?.[0]).toEqual([
      { computer: "", key: "run:fixture-2" }, { computer: "", key: "run:fixture-1" },
    ]);
    fixture.hidden = true; fixture.visibilityChanged(); await settle();
    expect(fixture.send.mock.calls.at(-1)?.[0]).toEqual([]);
    fixture.hidden = false; fixture.visibilityChanged(); await settle();
    expect(fixture.send.mock.calls.at(-1)?.[0]).toHaveLength(2);
    setPage("machines"); await settle();
    expect(fixture.send.mock.calls.at(-1)?.[0]).toEqual([]);
    setPage("sessions"); await settle();
    expect(fixture.send.mock.calls.at(-1)?.[0]).toHaveLength(2);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    document.dispatchEvent(new Event("visibilitychange")); await settle();
    expect(fixture.send.mock.calls.at(-1)?.[0]).toEqual([]);
    dispose(); dispose = undefined; await settle();
    expect(fixture.send.mock.calls.at(-1)?.[0]).toEqual([]);
  });
});
