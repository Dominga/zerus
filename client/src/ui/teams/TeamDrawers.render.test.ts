// @vitest-environment happy-dom
// Rows come from the root product writers through the core projection fixture.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TeamDrawers, TeamFeed, TeamInbox, TeamDrawerButtons } from "./TeamDrawers";
import { TeamReports } from "./TeamReports";
import { ProposalCards } from "./ProposalCards";
import { teamView } from "./fixtures";
import pages from "../../../core/src/teams/fixtures/drawers.json";
import type { DrawerPage, DrawerRead } from "../../store/teamDrawers";

vi.mock("./TeamLeadChat", () => ({ TeamLeadChat: () => null }));
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
const settle = async () => { for (let n = 0; n < 8; n++) await Promise.resolve(); };
const mount = (child: () => import("solid-js").JSX.Element) => {
  const host = document.createElement("div"); document.body.append(host); dispose = render(child, host); return host;
};
const inbox = pages.inbox as DrawerPage, feed = pages.feed as DrawerPage, cards = pages.cards as DrawerPage;

it("the bar offers all five drawers and marks the selected one", () => {
  const open = vi.fn();
  const host = mount(() => createComponent(TeamDrawerButtons, { value: "feed", onOpen: open }));
  expect(host.querySelectorAll("[data-team-drawer]")).toHaveLength(5);
  expect(host.querySelector('[data-team-drawer="feed"]')?.getAttribute("aria-expanded")).toBe("true");
  (host.querySelector('[data-team-drawer="cards"]') as HTMLButtonElement).click(); expect(open).toHaveBeenCalledWith("cards");
});

it("the feed renders the engine's exact journal text and actor without inventing detail", () => {
  const host = mount(() => createComponent(TeamFeed, { rows: feed.rows }));
  expect(host.querySelectorAll("[data-feed-seq]")).toHaveLength(feed.rows.length);
  expect(host.textContent).toContain(feed.rows[0].text); expect(host.textContent).toContain(feed.rows[0].by);
});

it("reports accept and return the product-written report node, with a reason", () => {
  const action = vi.fn();
  const host = mount(() => createComponent(TeamReports, { rows: inbox.rows, busy: false, onAction: action }));
  expect(host.querySelectorAll("[data-report]")).toHaveLength(1);
  expect(host.textContent).toContain(inbox.rows[0].text);
  (host.querySelector("[data-report-accept]") as HTMLButtonElement).click(); expect(action).toHaveBeenCalledWith("accept", { node: inbox.rows[0].node });
  ([...host.querySelectorAll("button")].find((button) => button.textContent === "Reject…") as HTMLButtonElement).click();
  const input = host.querySelector("input") as HTMLInputElement;
  input.value = "Check the empty case"; input.dispatchEvent(new Event("input", { bubbles: true }));
  (host.querySelector("[data-report-return]") as HTMLButtonElement).click();
  expect(action).toHaveBeenLastCalledWith("return", { node: inbox.rows[0].node, reason: "Check the empty case" });
});

it("the inbox preserves the product's answered question and does not answer it twice", () => {
  const host = mount(() => createComponent(TeamInbox, { rows: inbox.rows, busy: false, onAction: vi.fn() }));
  expect(host.textContent).toContain(inbox.rows[1].reply);
  expect(host.querySelector("[data-inbox-answer]")).toBeNull();
});

it("handwritten open-question control replies only through the existing item operation", () => {
  const action = vi.fn(); const row = { ...inbox.rows[1], state: "open", reply: null };
  const host = mount(() => createComponent(TeamInbox, { rows: [row], busy: false, onAction: action }));
  const input = host.querySelector("input") as HTMLInputElement;
  input.value = "Keep the old width"; input.dispatchEvent(new Event("input", { bubbles: true }));
  (host.querySelector("[data-inbox-answer]") as HTMLButtonElement).click();
  expect(action).toHaveBeenCalledWith("reply", { item: row.seq, text: "Keep the old width" });
});

it("handwritten non-question inbox kinds have no reply authority", () => {
  const row = { ...inbox.rows[1], state: "open", reply: null, kind: "unstaffed:m1" };
  const host = mount(() => createComponent(TeamInbox, { rows: [row], busy: false, onAction: vi.fn() }));
  expect(host.querySelector("[data-inbox-answer]")).toBeNull();
});

it("a refresh of the same report preserves a reason the person is typing", () => {
  const [rows, setRows] = createSignal(inbox.rows);
  const host = mount(() => createComponent(TeamReports, { get rows() { return rows(); }, busy: false, onAction: vi.fn() }));
  ([...host.querySelectorAll("button")].find((button) => button.textContent === "Reject…") as HTMLButtonElement).click();
  const input = host.querySelector("input") as HTMLInputElement;
  input.value = "Keep this reason"; input.dispatchEvent(new Event("input", { bubbles: true }));
  setRows(inbox.rows.map((row) => ({ ...row })));
  expect((host.querySelector("input") as HTMLInputElement).value).toBe("Keep this reason");
});

it("cards offer only the existing decision words for their exact sequence", () => {
  const action = vi.fn();
  const host = mount(() => createComponent(ProposalCards, { rows: cards.rows, busy: false, onAction: action }));
  expect(host.textContent).toContain(cards.rows[0].text);
  (host.querySelector('[data-card-decision="acknowledged"]') as HTMLButtonElement).click();
  expect(action).toHaveBeenCalledWith("decide", { card: cards.rows[0].seq, decision: "acknowledged" });
});

it("paging replaces the page and reuses its opaque snapshot cursor", async () => {
  const reads: DrawerRead[] = [];
  const host = mount(() => createComponent(TeamDrawers, {
    kind: "feed", view: teamView, busy: false, onAction: vi.fn(), onClose: vi.fn(),
    port: async (read) => { reads.push(read); return { ...feed, after: read.after, next: read.after ? null : "t1.ceiling" }; },
  }));
  await settle(); (host.querySelector("[data-drawer-next]") as HTMLButtonElement).click(); await settle();
  expect(reads.map((read) => read.after)).toEqual([null, "t1.ceiling"]);
  expect(host.querySelectorAll("[data-feed-seq]")).toHaveLength(feed.rows.length);
  (host.querySelector("[data-drawer-refresh]") as HTMLButtonElement).click(); await settle();
  expect(reads.at(-1)?.after).toBe("t1.ceiling");
});

it("a changed team revision disables the old report until its fresh read settles", async () => {
  const action = vi.fn(); let resolve!: (page: DrawerPage) => void; let count = 0;
  const [view, setView] = createSignal(teamView);
  const host = mount(() => createComponent(TeamDrawers, {
    kind: "reports", get view() { return view(); }, busy: false, onAction: action, onClose: vi.fn(),
    port: () => ++count === 1 ? Promise.resolve(inbox) : new Promise((done) => { resolve = done; }),
  }));
  await settle(); setView({ ...teamView, revision: teamView.revision + 1 }); await settle();
  const button = host.querySelector("[data-report-accept]") as HTMLButtonElement;
  expect(button.disabled).toBe(true); button.click(); expect(action).not.toHaveBeenCalled();
  resolve(inbox); await settle();
  expect((host.querySelector("[data-report-accept]") as HTMLButtonElement).disabled).toBe(false);
});

it("a read refusal is visible, leaves actions disabled and is never retried automatically", async () => {
  const port = vi.fn(async () => { throw new Error("teams_inactive: preview off"); });
  const host = mount(() => createComponent(TeamDrawers, { kind: "inbox", view: teamView, busy: false, onAction: vi.fn(), onClose: vi.fn(), port }));
  await settle(); expect(host.querySelector('[role="alert"]')?.textContent).toContain("teams_inactive");
  expect(port).toHaveBeenCalledTimes(1); expect(host.querySelector("[data-report-accept]")).toBeNull();
});
