// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TOOL_OUTPUT, ToolRow, type ProcessLinks } from "./ToolEvents";
import type { ActivityItem } from "./types";

// zerus-5wmh.13.8.1: an expanded tool row shows each event's recorded output
// (ActivityView.cpp:1105-1140), bounded so a huge output never becomes one
// giant DOM block: a fixed first part, then an explicit Show more step.
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

const big = `${"x".repeat(3_000_000)}\n${"y".repeat(2_000_000)}`;
const tool = (detail: string, process?: string): ActivityItem => ({
  id: "t", kind: "tool", at: "10:00", title: "Tool finished / Bash", body: "x…",
  events: [{ label: "Tool finished / Bash", at: "10:00", size: detail.length, process, detail: () => detail }],
});

function mount(item: ActivityItem, process?: ProcessLinks) {
  const host = document.createElement("div");
  document.body.append(host);
  const [open, setOpen] = createSignal(false);
  const [more, setMore] = createSignal<Record<number, number>>({});
  const onMore = vi.fn((index: number) => setMore((m) => ({ ...m, [index]: (m[index] ?? TOOL_OUTPUT.first) + TOOL_OUTPUT.step })));
  dispose = render(() => createComponent(ToolRow, {
    item, process,
    get open() { return open(); },
    onToggle: () => setOpen(!open()),
    shown: (index: number) => more()[index],
    onMore,
  }), host);
  const head = () => host.querySelector<HTMLButtonElement>(".act-tools-head")!;
  const detail = () => host.querySelector(".act-tool-detail");
  const button = (label: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === label);
  return { host, head, detail, button, onMore };
}

describe("an expanded tool row's output", () => {
  it("renders a multi-megabyte output only when expanded, capped, with an explicit Show more step", () => {
    const row = mount(tool(big));
    expect(row.detail()).toBeNull();
    expect(row.host.querySelector(".act-tool-cmd")?.textContent).toBe("x…");
    expect(row.host.textContent!.length).toBeLessThan(200);
    row.head().click();
    expect(row.detail()!.textContent).toBe(big.slice(0, TOOL_OUTPUT.first));
    expect(row.host.textContent!.length).toBeLessThan(TOOL_OUTPUT.first + 300);
    // Expanded, the output replaces the one-line preview.
    expect(row.host.querySelector(".act-tool-cmd")).toBeNull();
    row.button("Show more")!.click();
    expect(row.onMore).toHaveBeenCalledWith(0);
    expect(row.detail()!.textContent!.length).toBe(TOOL_OUTPUT.first + TOOL_OUTPUT.step);
    expect(row.host.textContent).toContain(`Showing ${TOOL_OUTPUT.first + TOOL_OUTPUT.step} of ${big.length} characters`);
    row.head().click();
    expect(row.detail()).toBeNull();
  });

  it("shows a short output whole, without Show more, and cuts a long one at a character boundary", () => {
    const short = mount(tool("line one\nline two"));
    short.head().click();
    expect(short.detail()!.textContent).toBe("line one\nline two");
    expect(short.button("Show more")).toBeUndefined();
    dispose?.(); document.body.replaceChildren();
    const emoji = `${"a".repeat(TOOL_OUTPUT.first - 1)}😀${"b".repeat(10)}`;
    const cut = mount(tool(emoji));
    cut.head().click();
    expect(cut.detail()!.textContent).toBe("a".repeat(TOOL_OUTPUT.first - 1));
  });

  it("links View process only for a process the session's list holds", () => {
    const id = "f".repeat(32);
    const open = vi.fn();
    const linked = mount(tool("done", id), { linkable: (process) => process === id, open });
    linked.head().click();
    linked.button("View process")!.click();
    expect(open).toHaveBeenCalledWith(id);
    dispose?.(); document.body.replaceChildren();
    const unknown = mount(tool("done", "e".repeat(32)), { linkable: (process) => process === id, open });
    unknown.head().click();
    expect(unknown.button("View process")).toBeUndefined();
    dispose?.(); document.body.replaceChildren();
    const none = mount(tool("done", id));
    none.head().click();
    expect(none.button("View process")).toBeUndefined();
  });
});
