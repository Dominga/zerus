// @vitest-environment happy-dom
// Valid rows come from the root writer -> core drawer projection fixture.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TeamDrawerButtons, TeamDrawers } from "./TeamDrawers";
import { TeamTimeline, recordedDate } from "./TeamTimeline";
import { teamView } from "./fixtures";
import pages from "../../../core/src/teams/fixtures/drawers.json";
import type { DrawerPage, DrawerRead } from "../../store/teamDrawers";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
const settle = async () => { for (let n = 0; n < 12; n++) await Promise.resolve(); };
const mount = (child: () => import("solid-js").JSX.Element) => {
  const host = document.createElement("div"); document.body.append(host); dispose = render(child, host); return host;
};
const feed = pages.feed as DrawerPage;

it("offers the recorded Timeline on demand without performing a read", () => {
  const open = vi.fn();
  const host = mount(() => createComponent(TeamDrawerButtons, { value: null, onOpen: open }));
  const button = host.querySelector('[data-team-drawer="timeline"]') as HTMLButtonElement;
  expect(button).not.toBeNull(); expect(open).not.toHaveBeenCalled();
  button.click(); expect(open).toHaveBeenCalledWith("timeline");
});

it("Timeline uses one ordinary feed read, displays recorded events and has no task actions", async () => {
  const port = vi.fn(async (read: DrawerRead) => ({ ...feed, after: read.after })); const action = vi.fn();
  const host = mount(() => createComponent(TeamDrawers, { kind: "timeline", view: teamView, busy: false, onAction: action, onClose: vi.fn(), port }));
  await settle();
  expect(port).toHaveBeenCalledExactlyOnceWith({ team: teamView.bar.id, kind: "feed", after: null });
  await vi.waitFor(() => expect(host.querySelectorAll("[data-timeline-seq]")).toHaveLength(feed.rows.length));
  expect(host.textContent).toContain("Recorded events"); expect(host.textContent).toContain("not measured work time");
  expect(host.textContent).toContain(feed.rows[0].text); expect(host.textContent).toContain(feed.rows[0].by);
  expect(host.querySelector("[data-timeline-coverage]")?.textContent).toContain(String(feed.rows.length));
  expect(host.querySelector("[data-report-accept], [data-inbox-answer], [data-card-decision], input, textarea")).toBeNull();
  expect(action).not.toHaveBeenCalled();
});

it("a failed Timeline read is unavailable rather than an empty timeline and never reposts", async () => {
  const port = vi.fn(async () => { throw new Error("teams_inactive: preview off"); });
  const host = mount(() => createComponent(TeamDrawers, { kind: "timeline", view: teamView, busy: false, onAction: vi.fn(), onClose: vi.fn(), port }));
  await settle(); expect(host.querySelector('[role="alert"]')?.textContent).toContain("teams_inactive");
  expect(host.querySelector("[data-timeline-seq]")).toBeNull(); expect(host.textContent).not.toContain("No events on this page.");
  expect(port).toHaveBeenCalledTimes(1);
});

it("closing Timeline drops an in-flight page without launching a second read", async () => {
  let resolve!: (page: DrawerPage) => void; const port = vi.fn(() => new Promise<DrawerPage>((done) => { resolve = done; }));
  const host = mount(() => createComponent(TeamDrawers, { kind: "timeline", view: teamView, busy: false, onAction: vi.fn(), onClose: vi.fn(), port }));
  await settle(); dispose!(); dispose = undefined; host.replaceChildren(); resolve(feed); await settle();
  expect(host.querySelector("[data-timeline-seq]")).toBeNull(); expect(port).toHaveBeenCalledTimes(1);
});

it("handwritten backward clocks preserve sequence order and show the coverage warning", () => {
  const page: DrawerPage = { ...feed, rows: [feed.rows[0], { ...feed.rows[1], at_ms: 0 }], timeline: { ...feed.timeline!, count: 2, clock_reversed: true, earlier_page: true, later_page: true } };
  const host = mount(() => createComponent(TeamTimeline, { page }));
  expect([...host.querySelectorAll("[data-timeline-seq]")].map((row) => Number(row.getAttribute("data-timeline-seq")))).toEqual(page.rows.map((row) => row.seq));
  expect(host.textContent).toContain("clocks moved backwards"); expect(host.textContent).toContain("Earlier pages are not included");
  expect(host.textContent).toContain("A later page is available");
  expect(host.querySelectorAll("time")[1].dateTime).toBe("1970-01-01T00:00:00.000Z");
});

it("handwritten empty last page and absent projection remain distinct", () => {
  const page: DrawerPage = { ...feed, rows: [], timeline: { ...feed.timeline!, count: 0, first_seq: null, last_seq: null, earlier_page: true } };
  const host = mount(() => createComponent(TeamTimeline, { page }));
  expect(host.textContent).toContain("No events on this page."); expect(host.textContent).toContain("Earlier pages are not included.");
  dispose!(); dispose = undefined; host.remove();
  const other = mount(() => createComponent(TeamTimeline, { page: { ...feed, timeline: undefined } }));
  expect(other.querySelector('[role="alert"]')?.textContent).toContain("coverage is unavailable");
  expect(other.textContent).not.toContain("No events on this page.");
});

it("handwritten untrusted event text stays text and invalid dates stay unavailable", () => {
  const page: DrawerPage = { ...feed, rows: [{ ...feed.rows[0], at_ms: 9e18, text: '<img src=x onerror="alert(1)">', text_truncated: true }] };
  const host = mount(() => createComponent(TeamTimeline, { page }));
  expect(host.querySelector("img")).toBeNull(); expect(host.textContent).toContain("<img");
  expect(host.textContent).toContain("Recorded time unavailable"); expect(host.textContent).toContain("Only the beginning");
  expect(recordedDate(Number.NaN)).toBeNull(); expect(recordedDate(1.5)).toBeNull();
  expect(recordedDate(0)?.iso).toBe("1970-01-01T00:00:00.000Z");
});

it("Timeline Next replaces its page using the exact cursor and does not call task actions", async () => {
  const reads: DrawerRead[] = []; const action = vi.fn();
  const port = async (read: DrawerRead) => { reads.push(read); return { ...feed, after: read.after, next: read.after ? null : "t1.snapshot", rows: read.after ? feed.rows.slice(-1) : feed.rows }; };
  const host = mount(() => createComponent(TeamDrawers, { kind: "timeline", view: teamView, busy: false, onAction: action, onClose: vi.fn(), port }));
  await settle(); (host.querySelector("[data-drawer-next]") as HTMLButtonElement).click(); await settle();
  expect(reads.map((read) => read.after)).toEqual([null, "t1.snapshot"]);
  expect(host.querySelectorAll("[data-timeline-seq]")).toHaveLength(1);
  (host.querySelector("[data-drawer-refresh]") as HTMLButtonElement).click(); await settle();
  expect(reads.at(-1)?.after).toBe("t1.snapshot"); expect(action).not.toHaveBeenCalled();
});

it("hiding Timeline clears the page and late result; showing starts at First without accumulation", async () => {
  const [visible, setVisible] = createSignal(true);
  let resolve!: (page: DrawerPage) => void; let calls = 0;
  const port = vi.fn(() => ++calls === 1 ? new Promise<DrawerPage>((done) => { resolve = done; }) : Promise.resolve(feed));
  const host = mount(() => createComponent(TeamDrawers, { kind: "timeline", view: teamView, busy: false, get visible() { return visible(); }, onAction: vi.fn(), onClose: vi.fn(), port }));
  await settle(); setVisible(false); resolve(feed); await settle();
  expect(host.querySelector("[data-timeline-seq]")).toBeNull(); expect(port).toHaveBeenCalledTimes(1);
  setVisible(true); await settle(); expect(port).toHaveBeenCalledTimes(2);
  expect(host.querySelectorAll("[data-timeline-seq]")).toHaveLength(feed.rows.length);
});

it("a replaced team cannot publish its older in-flight Timeline", async () => {
  const [view, setView] = createSignal(teamView); let resolve!: (page: DrawerPage) => void; let calls = 0;
  const other = "00000000-0000-4000-8000-00000000cafe";
  const port = vi.fn((read: DrawerRead) => ++calls === 1 ? new Promise<DrawerPage>((done) => { resolve = done; }) : Promise.resolve({ ...feed, team: read.team, rows: [] }));
  const host = mount(() => createComponent(TeamDrawers, { kind: "timeline", get view() { return view(); }, busy: false, onAction: vi.fn(), onClose: vi.fn(), port }));
  await settle(); setView({ ...teamView, bar: { ...teamView.bar, id: other } }); resolve(feed); await settle();
  expect(host.querySelector("[data-timeline-seq]")).toBeNull(); expect(port).toHaveBeenCalledTimes(2);
  expect(host.textContent).toContain("No events on this page.");
});

it("new team events do not silently refresh Timeline's first-page ceiling before Next", async () => {
  const [view, setView] = createSignal(teamView); const reads: DrawerRead[] = [];
  const port = vi.fn(async (read: DrawerRead) => { reads.push(read); return { ...feed, after: read.after, next: read.after ? null : "t1.original-ceiling" }; });
  const host = mount(() => createComponent(TeamDrawers, { kind: "timeline", get view() { return view(); }, busy: false, onAction: vi.fn(), onClose: vi.fn(), port }));
  await settle(); setView({ ...teamView, revision: teamView.revision + 10 }); await settle();
  expect(port).toHaveBeenCalledTimes(1);
  (host.querySelector("[data-drawer-next]") as HTMLButtonElement).click(); await settle();
  expect(reads.map((read) => read.after)).toEqual([null, "t1.original-ceiling"]);
});
