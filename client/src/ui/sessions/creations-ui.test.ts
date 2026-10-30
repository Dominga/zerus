import { describe, expect, it, vi } from "vitest";
import {
  BUTTON, NO_PENDING, OLDER, PREPARING, accessibleText, computerOf, createDetailsView, creationOfRow, creationRowId, elapsed, footerText, machineOf,
  newestFirst, noticeOf, panelButtons, panelStatus, placeCreations, projectOf, rowDetail, statusMenu, shownCards, type ListScope, type Placement,
} from "./creations-ui";
import { toSession } from "../../store/catalog";
import { applyGrouping } from "../../store/organization";
import { placement } from "./organize";
import type { Row } from "./model";
import { emptyBatch, type CreationCard, type GroupingOut, type ViewRow } from "../../types";
import type { Session } from "./types";
import { accountBinding } from "./scope-filters";

// The creation cards' rows, buttons, menu and footer (SessionsWindow.cpp
// 463-480, 3729-3756; SessionsWindowRows.cpp 259-341, 376-397, 474-479).

// organize.ts reads the page's query (store/ui.ts) and the list; the tests run without a page and pass their rows.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });
vi.mock("./model", () => ({ listRows: () => [] }));

const card = (id: string, extra: Partial<CreationCard> = {}): CreationCard => ({
  id, tag: `tag-${id}`, state: "queued", caption: "Queued", error: "", created: 1_793_000_000, group: "alpha", host: "",
  machine: "workstation", cmd: "claude", account: "", target: "/workspace/example", show_pending: true, launch_dispatched: false,
  stop_requested: false, worktree_receipts: false, can_resume: false, can_edit: false, bound_session: "", ...extra,
});

it("scopes pending creations by exact account binding and target folder, preserving an explicit reveal", () => {
  const cards = [card("a", { account: "work", target: "/srv/example/app" }), card("b", { account: "personal", target: "/srv/example/app" }), card("c", { account: "work", target: "/srv/example/other" })];
  const scope = { grouping: { loaded: false, default_project: "alpha", headers: [] } as unknown as GroupingOut, filter: "all" as const, hosts: [], projects: [], query: "", accounts: [accountBinding("", "claude", "work")], folder: { computer: "", path: "/srv/example/app", checkout: true, style: "posix" as const } };
  const listed = (revealed?: string) => [...shownCards(scope, { cards, revealed, details: () => undefined }).values()].flat().map((c) => c.id);
  expect(listed()).toEqual(["a"]);
  expect(listed("b")).toEqual(["a", "b"]);
});
const session = (n: number, extra: Partial<ViewRow> = {}): Session =>
  toSession("", `run:${n}`, { name: `claude/sample-project/s${n}`, machine: "workstation", status: "ready", ...extra });
const header = (id: string, collapsed = false) => ({ id, name: id.toUpperCase(), color: null, accessible: true, collapsed, is_default: id === "ungrouped", palette: 0 });
const grouping = (headers: ReturnType<typeof header>[]): GroupingOut => ({
  loaded: true, default_project: "ungrouped", headers, ready: true, reason: null, busy: false, status: "", status_error: false, conflicts: 0,
});

/** The sessions' rows as `groupRows` makes them: a header per project, then its members (none when collapsed). */
function sessionRows(g: GroupingOut, members: Map<string, Session[]>): Row[] {
  const out: Row[] = [];
  for (const h of g.headers) {
    const list = members.get(h.id) ?? [];
    if (!list.length) continue;
    if (g.headers.length > 1) out.push({ type: "header", id: `h:${h.id}`, project: h.id, name: h.name, color: null, palette: 0, collapsed: h.collapsed, total: list.length, attention: 0, unread: 0, working: 0 });
    if (!h.collapsed || g.headers.length <= 1) for (const s of list) out.push({ type: "card", id: s.key, session: s, expandable: false, expanded: false, compactRow: false });
  }
  return out;
}
const scopeOf = (g: GroupingOut, members: Map<string, Session[]>, extra: Partial<ListScope> = {}): ListScope => ({
  grouping: g, members, visible: () => true, online: () => true, projects: [], expanded: () => false, compact: () => false, filter: "all", hosts: [], query: "", ...extra,
});
const stateOf = (cards: CreationCard[], extra: Partial<Placement> = {}): Placement => ({
  cards, groupOf: () => "ungrouped", details: () => undefined, ...extra,
});
const shape = (rows: Row[]) => rows.map((r) => r.type === "header" ? `#${r.project}:${r.total}` : r.type === "card" ? r.session.name.slice(-2) : r.type === "creation" ? `+${r.creation.id}` : "child");

describe("the creation rows (SessionsWindowRows.cpp 300-341)", () => {
  const g = grouping([header("alpha"), header("ungrouped"), header("beta")]);
  const members = new Map([["alpha", [session(1), session(2)]], ["ungrouped", [session(3)]]]);
  const rows = sessionRows(g, members);

  it("follow their project's sessions and count in its header", () => {
    expect(shape(placeCreations(rows, scopeOf(g, members), stateOf([card("a")])))).toEqual(["#alpha:3", "s1", "s2", "+a", "#ungrouped:1", "s3"]);
  });

  it("go under the default project when theirs is not listed", () => {
    expect(shape(placeCreations(rows, scopeOf(g, members), stateOf([card("a", { group: "gone" })])))).toEqual(["#alpha:2", "s1", "s2", "#ungrouped:2", "s3", "+a"]);
    expect(projectOf({ group: "gone" }, g)).toBe("ungrouped");
    // Before the organization is presented, every card is the default project's.
    expect(projectOf({ group: "alpha" }, { ...g, loaded: false })).toBe("ungrouped");
  });

  it("give a project with only cards its header, with its members' counters", () => {
    const busy = new Map([...members, ["beta", [session(4, { status: "working" })]]]);
    const out = placeCreations(sessionRows(g, members), scopeOf(g, busy), stateOf([card("b", { group: "beta" })]));
    expect(shape(out)).toEqual(["#alpha:2", "s1", "s2", "#ungrouped:1", "s3", "#beta:1", "+b"]);
    const beta = out[5];
    expect(beta.type === "header" && [beta.name, beta.working]).toEqual(["BETA", 1]);
    const offline = placeCreations(sessionRows(g, members), scopeOf(g, busy, { online: () => false }), stateOf([card("b", { group: "beta" })]));
    expect(offline[5].type === "header" && offline[5].working).toBe(0);
  });

  it("hide in a collapsed project, whose total still counts them", () => {
    const folded = grouping([header("alpha", true), header("ungrouped")]);
    expect(shape(placeCreations(sessionRows(folded, members), scopeOf(folded, members), stateOf([card("a"), card("b")])))).toEqual(["#alpha:4", "#ungrouped:1", "s3"]);
  });

  it("walk the operations in id order, as Qt's QJsonObject does", () => {
    const out = placeCreations(rows, scopeOf(g, members), stateOf([card("c", { created: 9 }), card("a", { created: 1 }), card("b", { created: 5 })]));
    expect(shape(out).filter((s) => s.startsWith("+"))).toEqual(["+a", "+b", "+c"]);
  });

  it("show only under All and Active, never hidden, and only for a chosen machine", () => {
    const one = [card("a")];
    for (const filter of ["attention", "paused", "archived"] as const) expect(shape(placeCreations(rows, scopeOf(g, members, { filter }), stateOf(one)))).not.toContain("+a");
    expect(shape(placeCreations(rows, scopeOf(g, members, { filter: "active" }), stateOf(one)))).toContain("+a");
    expect(shape(placeCreations(rows, scopeOf(g, members), stateOf([card("a", { show_pending: false })])))).not.toContain("+a");
    expect(shape(placeCreations(rows, scopeOf(g, members, { hosts: ["studio"] }), stateOf(one)))).not.toContain("+a");
    expect(shape(placeCreations(rows, scopeOf(g, members, { hosts: ["workstation"] }), stateOf(one)))).toContain("+a");
    // A peer's card goes by its machine's name, as that computer's session rows do.
    expect(machineOf(card("p", { host: "studio", machine: "studio.example.invalid" }))).toBe("studio.example.invalid");
    expect(machineOf(card("p", { host: "studio", machine: "" }))).toBe("studio");
  });

  it("follow the project filter", () => {
    expect(shape(placeCreations(sessionRows(g, new Map([["alpha", members.get("alpha")!]])), scopeOf(g, members, { projects: ["alpha"] }), stateOf([card("a"), card("u", { group: "ungrouped" })]))))
      .toEqual(["#alpha:3", "s1", "s2", "+a"]);
  });

  it("stay under a search when their details contain it, ignoring case (deviation 6)", () => {
    const details = (id: string) => (id === "a" ? "tag-a\n\nComputer: workstation\n\nFolder: /workspace/Example" : undefined);
    expect(shape(placeCreations(rows, scopeOf(g, members, { query: "folder: /WORKSPACE/example" }), stateOf([card("a"), card("b")], { details })))).toContain("+a");
    // A card whose details have not been read yet waits for them.
    expect(shape(placeCreations(rows, scopeOf(g, members, { query: "folder" }), stateOf([card("b")], { details })))).not.toContain("+b");
  });

  it("show the revealed card through every filter but hiding, with its project", () => {
    const revealed = stateOf([card("a"), card("b")], { revealed: "a" });
    const filtered = scopeOf(g, members, { filter: "attention", hosts: ["studio"], query: "nothing", projects: ["ungrouped"] });
    // The project filter keeps the revealed card's project; the other card there still follows the filters.
    expect(shape(placeCreations(sessionRows(g, new Map([["ungrouped", members.get("ungrouped")!]])), filtered, revealed))).toEqual(["#alpha:1", "+a", "#ungrouped:1", "s3"]);
    expect(shape(placeCreations(rows, filtered, stateOf([card("a", { show_pending: false })], { revealed: "a" })))).not.toContain("+a");
  });

  it("show the adopted session through every filter, at its rank, until it is listed anyway", () => {
    const adopted = session(2);
    const state = stateOf([], { adopted, groupOf: (key) => (key === adopted.key ? "alpha" : "ungrouped") });
    const onlyFirst = sessionRows(g, new Map([["alpha", [session(1)]], ["ungrouped", [session(3)]]]));
    expect(shape(placeCreations(onlyFirst, scopeOf(g, members), state))).toEqual(["#alpha:2", "s1", "s2", "#ungrouped:1", "s3"]);
    // Its project shows through the project filter.
    expect(shape(placeCreations(sessionRows(g, new Map([["ungrouped", [session(3)]]])), scopeOf(g, members, { projects: ["ungrouped"] }), state)))
      .toEqual(["#alpha:1", "s2", "#ungrouped:1", "s3"]);
    // Already listed: nothing changes.
    expect(placeCreations(rows, scopeOf(g, members), state)).toBe(rows);
    // An archived row is never adopted.
    expect(placeCreations(onlyFirst, scopeOf(g, members), { ...state, adopted: session(2, { state: "archived" } as Partial<ViewRow>) })).toBe(onlyFirst);
  });

  it("count an adopted session once in a collapsed project's header", () => {
    const folded = grouping([header("alpha", true), header("ungrouped")]);
    const adopted = session(2);
    const state = stateOf([], { adopted, groupOf: () => "alpha" });
    // groupRows already counts this visible member although its collapsed project has no card row.
    expect(shape(placeCreations(sessionRows(folded, members), scopeOf(folded, members), state))).toEqual(["#alpha:2", "#ungrouped:1", "s3"]);
    // One the ordinary visibility rule excluded is the single adopted addition.
    expect(shape(placeCreations(sessionRows(folded, members), scopeOf(folded, members, { visible: () => false }), state))).toEqual(["#alpha:3", "#ungrouped:1", "s3"]);
  });

  it("leave the rows as they are without a card to show", () => {
    expect(placeCreations(rows, scopeOf(g, members), stateOf([]))).toBe(rows);
    expect(placeCreations(rows, scopeOf(g, members, { filter: "paused" }), stateOf([card("a")]))).toBe(rows);
  });

  it("follow the sessions without a header in a single project", () => {
    const single = grouping([header("ungrouped", true)]);
    const list = new Map([["ungrouped", [session(1)]]]);
    expect(shape(placeCreations(sessionRows(single, list), scopeOf(single, list), stateOf([card("a", { group: "ungrouped" })])))).toEqual(["s1", "+a"]);
  });

  it("are keyed as Qt keys a creation row", () => {
    expect(creationRowId("a")).toBe("#launch\na");
    expect(creationOfRow("#launch\na")).toBe("a");
    expect(creationOfRow("\nrun:1")).toBeUndefined();
    expect(creationOfRow(undefined)).toBeUndefined();
  });
});

describe("a creation row's texts (376-391)", () => {
  it("shows the error, else that it is preparing only before the first stage", () => {
    expect(rowDetail(card("a", { state: "saving" }))).toBe(PREPARING);
    expect(rowDetail(card("a"))).toBe("");
    expect(rowDetail(card("a", { error: "The folder is gone." }))).toBe("The folder is gone.");
    // paintCard drops a detail that repeats the status.
    expect(rowDetail(card("a", { caption: "Creation failed", error: "Creation failed: the folder is gone." }))).toBe("the folder is gone.");
    expect(rowDetail(card("a", { caption: "Creation failed", error: "Creation failed" }))).toBe("");
  });

  it("names this computer by its host name and a peer by its alias", () => {
    expect(computerOf(card("a"))).toBe("workstation");
    expect(computerOf(card("a", { host: "studio", machine: "studio.example.invalid" }))).toBe("studio");
    expect(accessibleText(card("a", { caption: "Creating worktree…" }))).toBe("tag-a, workstation, Creating worktree…");
  });
});

describe("the progress view (renderCreation 3729-3756)", () => {
  it("shows each button for its states", () => {
    const states = ["saving", "queued", "checking", "checking_repository", "cloning", "worktree", "verifying", "launching", "confirming", "stopping", "stopped", "recover", "unknown", "storage_error", "failed"];
    const check = states.filter((state) => panelButtons(card("a", { state })).check);
    expect(check).toEqual(["confirming", "recover", "unknown"]);
    const stop = states.filter((state) => panelButtons(card("a", { state })).stop);
    expect(stop).not.toContain("stopped");
    expect(stop).not.toContain("failed");
    expect(stop).not.toContain("storage_error");
    expect(stop).toHaveLength(states.length - 3);
    // A dispatched launch can no longer stop.
    expect(panelButtons(card("a", { state: "confirming", launch_dispatched: true })).stop).toBe(false);
    // Continue setup and Edit choices… follow the core.
    expect(panelButtons(card("a", { can_resume: true, can_edit: true }))).toMatchObject({ resume: true, edit: true });
    expect(panelButtons(card("a"))).toMatchObject({ resume: false, edit: false });
  });

  it("words Stop for what it waits for, and disables it once asked", () => {
    expect(panelButtons(card("a", { state: "cloning" })).stopText).toBe(BUTTON.stopAfter);
    expect(panelButtons(card("a", { state: "worktree" })).stopText).toBe(BUTTON.stopAfter);
    expect(panelButtons(card("a", { state: "worktree", worktree_receipts: true })).stopText).toBe(BUTTON.stop);
    expect(panelButtons(card("a", { state: "checking" })).stopText).toBe("Stop creating");
    expect(panelButtons(card("a", { stop_requested: true })).stopEnabled).toBe(false);
  });

  it("shows the caption and the elapsed time under the tag (D6)", () => {
    const c = card("a", { caption: "Creating worktree…", created: 1000 });
    expect(elapsed(c, 1000 + 65.9)).toBe("1:05");
    expect(elapsed(c, 999)).toBe("0:00");
    expect(panelStatus(c, 1000 + 600)).toBe("Creating worktree… (10:00)");
  });
});

describe("the progress details' answers", () => {
  it("drop a stale card's answer and keep the scroll on refresh", () => {
    let value = "", top = 0;
    // A textarea resets its scroll when the test writes its value, as the DOM does.
    const box = {
      get value() { return value; },
      set value(next: string) { value = next; top = 0; },
      get scrollTop() { return top; },
      set scrollTop(next: number) { top = next; },
    };
    const view = createDetailsView(box);
    const first = view.read("a");
    first("one");
    expect(box.value).toBe("one");
    const stale = view.read("a");
    const other = view.read("b");
    stale("stale");
    first("also stale");
    expect([box.value, box.scrollTop]).toEqual(["", 0]);
    other("two");
    expect(box.value).toBe("two");
    box.scrollTop = 37;
    const refresh = view.read("b");
    refresh("three");
    expect([box.value, box.scrollTop]).toEqual(["three", 37]);
    const pending = view.read("b");
    view.read(undefined);
    pending("late");
    expect([box.value, box.scrollTop]).toEqual(["", 0]);
  });
});

describe("the status menu (463-480)", () => {
  it("lists newest first, then by id, as \"{tag}: {caption}\" (D6)", () => {
    const choose = vi.fn();
    const cards = [card("b", { created: 5 }), card("a", { created: 5 }), card("c", { created: 9, caption: "Creation failed" })].sort(newestFirst);
    const entries = statusMenu(cards, choose);
    expect(entries.map((e) => ("label" in e ? e.label : ""))).toEqual(["tag-c: Creation failed", "tag-a: Queued", "tag-b: Queued"]);
    const first = entries[0];
    if ("action" in first) first.action?.();
    expect(choose).toHaveBeenCalledWith("c");
  });

  it("puts a heading before every further 50", () => {
    const cards = Array.from({ length: 101 }, (_, i) => card(String(i).padStart(3, "0")));
    const entries = statusMenu(cards, () => {});
    const headings = entries.flatMap((e, i) => ("heading" in e ? [i] : []));
    expect(headings).toEqual([50, 101]);
    expect(entries[50]).toEqual({ heading: OLDER });
    expect(entries).toHaveLength(103);
  });

  it("has one disabled entry when empty", () => {
    expect(statusMenu([], () => {})).toEqual([{ label: NO_PENDING, disabled: true }]);
  });
});

describe("the footer (474-479)", () => {
  it("adds the creations after the sessions' text, or alone", () => {
    expect(footerText(0, true)).toBe("");
    expect(footerText(1, true)).toBe(", 1 creation");
    expect(footerText(3, true)).toBe(", 3 creations");
    expect(footerText(1, false)).toBe("1 creation");
    expect(footerText(2, false)).toBe("2 creations");
  });
});

describe("the notices (1577-1586)", () => {
  it("are kept for failed, unknown and unsaved creations, keyed by state and error", () => {
    for (const state of ["queued", "stopped", "recover", "bound"]) expect(noticeOf(card("a", { state }))).toBeUndefined();
    expect(noticeOf(card("a", { state: "failed", caption: "Creation failed", error: "No folder." })))
      .toEqual({ key: "failed\nNo folder.", text: "Creation tag-a: Creation failed\nNo folder." });
    expect(noticeOf(card("a", { state: "unknown" }))?.key).toBe("unknown\n");
    expect(noticeOf(card("a", { state: "storage_error" }))?.key).toBe("storage_error\n");
  });
});

describe("a drop over a creation row (SessionList.cpp:490-528)", () => {
  it("goes into the card's project", () => {
    applyGrouping({ ...emptyBatch(), grouping: grouping([header("alpha"), header("ungrouped")]) });
    const s1 = session(1), s2 = session(2);
    const rows: Row[] = [
      { type: "header", id: "h:alpha", project: "alpha", name: "ALPHA", color: null, palette: 0, collapsed: false, total: 2, attention: 0, unread: 0, working: 0 },
      { type: "creation", id: creationRowId("a"), creation: card("a") },
      { type: "card", id: s1.key, session: s1, expandable: false, expanded: false, compactRow: false },
      { type: "creation", id: creationRowId("b"), creation: card("b", { group: "gone" }) },
    ];
    const offsets = [0, 40, 134, 228], heights = [40, 94, 94, 94];
    const item = { kind: "session" as const, id: s2.key };
    // The upper half names the card, which is no session: the drop goes to the project's end, as Qt's empty identity does.
    expect(placement(rows, offsets, heights, creationRowId("a"), 50, item)).toEqual({ group: "alpha", before: creationRowId("a"), line: 40, inset: 12 });
    // A card under an unlisted project is the default project's.
    expect(placement(rows, offsets, heights, creationRowId("b"), 230, item).group).toBe("ungrouped");
  });
});
