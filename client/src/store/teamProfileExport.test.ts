import { expect, it, vi } from "vitest";
import { RecordedPageExport, recordedSnapshot, type PageExport } from "./teamProfileExport";
import type { DrawerPage } from "./teamDrawers";
import timingPage from "../../core/src/teams/fixtures/recorded-timings.json";
import pages from "../../core/src/teams/fixtures/drawers.json";
const feed = pages.feed as DrawerPage;

it("exports exactly the captured product page and never reads or drains another page", async () => {
  const port = vi.fn(async (_value: PageExport) => true); const state = new RecordedPageExport(port);
  await state.save(feed);
  expect(port).toHaveBeenCalledExactlyOnceWith(recordedSnapshot(feed));
  const value = port.mock.calls[0][0];
  expect(value.rows).toHaveLength(feed.rows.length); expect(value.after).toBe(feed.after); expect(value.next).toBe(feed.next);
  expect(value.computer).toBeNull(); expect(value.phase).toBeNull(); expect(value.history_complete).toBe(false);
  expect(state.outcome).toBe("saved");
});

it("the held export stays on its original scope while the view is retired", async () => {
  let resolve!: (saved: boolean) => void; const port = vi.fn(() => new Promise<boolean>((done) => { resolve = done; }));
  const state = new RecordedPageExport(port); const held = state.save(feed); state.retire();
  await state.save({ ...feed, team: "other" }); expect(port).toHaveBeenCalledTimes(1);
  resolve(true); await held; expect(state.outcome).toBe("none"); expect(state.busy).toBe(false);
});

it("unknown or cancelled file outcomes never repost or become a success", async () => {
  for (const port of [vi.fn(async () => { throw new Error("write result unknown"); }), vi.fn(async () => undefined as unknown as boolean)]) {
    const state = new RecordedPageExport(port); await state.save(feed);
    expect(state.outcome).toBe("unknown"); expect(state.error).not.toBe(""); expect(port).toHaveBeenCalledTimes(1);
  }
  const port = vi.fn(async () => false); const state = new RecordedPageExport(port); await state.save(feed);
  expect(state.outcome).toBe("cancelled"); expect(port).toHaveBeenCalledTimes(1);
});

it("the captured snapshot cannot be mutated through the original page", () => {
  const page = structuredClone(feed); const captured = recordedSnapshot(page);
  page.rows[0].text = "changed later";
  expect(captured.rows[0].text).toBe(feed.rows[0].text);
  expect(captured.rows[0]).not.toHaveProperty("card"); expect(captured.rows[0]).not.toHaveProperty("reply");
});

it("includes only the already displayed timing snapshot without a new read", async () => {
  const page = timingPage as DrawerPage; const port = vi.fn(async (_value: PageExport) => true); const state = new RecordedPageExport(port);
  await state.save(page); expect(port).toHaveBeenCalledExactlyOnceWith(recordedSnapshot(page));
  expect(port.mock.calls[0][0].timings).toEqual(page.timings);
});
