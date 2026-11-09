// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import type { CreationCard } from "../../types";

// zerus-5wmh.13.17.3 (SessionSearch.cpp:287-305): pending creations whose
// details match the search follow the session results, in the same list.
const fixture = vi.hoisted(() => ({ cards: [] as CreationCard[], selected: [] as string[], activated: [] as string[] }));
vi.mock("@tanstack/solid-virtual", () => ({
  createVirtualizer: (options: { count: number; getItemKey(n: number): string }) => ({
    measure() {}, measureElement() {}, _willUpdate() {}, scrollToIndex() {}, getTotalSize: () => options.count * 64,
    getVirtualItems: () => Array.from({ length: options.count }, (_, index) => ({ key: options.getItemKey(index), start: index * 64, index })),
  }),
}));
vi.mock("../../store/contentSearch", async () => {
  const { createSignal } = await import("solid-js");
  const [cursor, setCursor] = createSignal("");
  const session = { key: "k1", name: "codex/demo/api", title: "API work", agent: "codex", machine: "workstation", computer: "", model: "gpt" };
  return {
    contentSearch: { errors: () => [], busy: () => false, refresh: () => {} },
    searchItems: () => [{ id: "metadata:k1", session, run: "", conversation: "" }],
    searchStatus: () => "0 messages, 1 session, 1 creation",
    activateSearchItem: (item: { id: string }) => fixture.activated.push(item.id),
    searchCursor: cursor, setSearchCursor: setCursor,
  };
});
vi.mock("../../store/creations", () => ({
  searchCards: () => fixture.cards,
  selectCreation: (id: string) => fixture.selected.push(id),
  selectedCreation: () => undefined,
}));
vi.mock("../../store/ui", () => ({ search: () => "api", selectedKey: () => undefined, stateFilter: () => "all" }));
vi.mock("../../store/catalog", () => ({ agentKind: (cmd: string) => cmd }));
// The archive's partial-scan notice (zerus-080n8) shows only under the Archive filter.
vi.mock("../../store/archive", () => ({ archive: { partial: () => false, partialNewer: () => false, searched: () => 0 } }));
vi.mock("./SessionCard", () => ({ AgentBadge: () => null, MachineBadge: () => null }));
import { SearchResults } from "./SearchResults";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); fixture.selected.length = 0; fixture.activated.length = 0; });
const card = (extra: Partial<CreationCard> = {}): CreationCard => ({
  id: "c1", tag: "api-fix", state: "failed", caption: "Failed / codex", error: "API key missing", created: 1, group: "", host: "", machine: "workstation",
  cmd: "codex", account: "", target: "/workspace/api", show_pending: true, launch_dispatched: false, stop_requested: false, worktree_receipts: false,
  can_resume: true, can_edit: true, bound_session: "", ...extra,
});

describe("pending creations in the search results", () => {
  it("lists a matching creation after the sessions, reachable by keyboard, and opens it", () => {
    fixture.cards = [card()];
    const host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(SearchResults, {}), host);
    const rows = [...host.querySelectorAll<HTMLButtonElement>(".content-search-row")];
    expect(rows.map((r) => r.classList.contains("creation-hit"))).toEqual([false, true]);
    expect(rows[1].textContent).toContain("api-fix");
    expect(rows[1].textContent).toContain("Failed / codex");
    expect(rows[1].textContent).toContain("API key missing");
    const list = host.querySelector<HTMLElement>("[role=listbox]")!;
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    expect(fixture.activated).toEqual(["metadata:k1"]);
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    expect(fixture.selected).toEqual(["c1"]);
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(fixture.selected).toEqual(["c1", "c1"]);
    rows[1].click();
    expect(fixture.selected).toEqual(["c1", "c1", "c1"]);
  });

  it("shows only the sessions when no creation matches", () => {
    fixture.cards = [];
    const host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(SearchResults, {}), host);
    expect(host.querySelectorAll(".creation-hit")).toHaveLength(0);
    expect(host.querySelectorAll(".content-search-row")).toHaveLength(1);
  });
});
