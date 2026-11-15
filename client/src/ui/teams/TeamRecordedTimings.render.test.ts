// @vitest-environment happy-dom
// Data is produced by Root operations/read and the core's pinned projection.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { TeamTimeline } from "./TeamTimeline";
import { TeamDrawers } from "./TeamDrawers";
import type { DrawerPage, DrawerRead } from "../../store/teamDrawers";
import data from "../../../core/src/teams/fixtures/recorded-timings.json";
import pages from "../../../core/src/teams/fixtures/drawers.json";
import { teamView } from "./fixtures";
const page = data as DrawerPage;
const events: DrawerPage = { ...page, timings: undefined, rows: page.rows.map(({ timing: _timing, ...row }) => row) };
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
const settle = async () => { for (let n = 0; n < 16; n++) await Promise.resolve(); };
const mount = (child: () => import("solid-js").JSX.Element) => { const host = document.createElement("div"); document.body.append(host); dispose = render(child, host); return host; };

it("shows product effect and stage times as recorded bookkeeping at the read revision", () => {
  const host = mount(() => createComponent(TeamTimeline, { page }));
  expect(host.querySelector('[data-recorded-revision]')?.textContent).toContain(String(page.timings!.revision));
  expect(host.textContent).toContain("Reservation recorded"); expect(host.textContent).toContain("Stage creation recorded");
  expect(host.textContent).toContain("Recorded stage state");
  expect(host.textContent).toContain("Delivery bookkeeping elapsed"); expect(host.textContent).toContain("not measured work or test duration");
  expect(host.querySelector("input, textarea, button")).toBeNull();
});

it("the explicit timing read is one read of the chosen page and Events only is reversible", async () => {
  const reads: DrawerRead[] = [];
  const port = async (read: DrawerRead) => { reads.push(read); return read.recorded_timings ? { ...page, after: read.after } : { ...events, after: read.after }; };
  const host = mount(() => createComponent(TeamDrawers, { kind: "timeline", view: teamView, busy: false, onAction: vi.fn(), onClose: vi.fn(), port }));
  await settle(); (host.querySelector('[data-timeline-timings]') as HTMLButtonElement).click(); await settle();
  expect(reads).toHaveLength(2); expect(reads[1].recorded_timings).toBe(true);
  expect(host.textContent).toContain("Events only"); expect(host.querySelector('[data-recorded-revision]')).not.toBeNull();
  (host.querySelector('[data-timeline-timings]') as HTMLButtonElement).click(); await settle();
  expect(reads).toHaveLength(2);
  expect(host.querySelector('[data-recorded-revision]')).toBeNull();
});

it("handwritten reversed fact clocks are unavailable instead of a zero work interval", () => {
  const corrupted = structuredClone(page); const effect = corrupted.rows.find((r) => r.timing?.effect)!.timing!.effect!;
  effect.record.reserved_ms = 2000; effect.record.first_outcome_recorded_ms = 1000; effect.bookkeeping_elapsed_ms = null; effect.clock_or_range_unknown = true;
  const host = mount(() => createComponent(TeamTimeline, { page: corrupted }));
  expect(host.textContent).toContain("recorded clocks or range are inconsistent");
});
