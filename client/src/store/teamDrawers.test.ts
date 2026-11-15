import { expect, it } from "vitest";
import { TeamDrawerRead, type DrawerPage, type DrawerRead } from "./teamDrawers";
import pages from "../../core/src/teams/fixtures/drawers.json";

const product = pages.inbox as DrawerPage;
const settle = async () => { for (let n = 0; n < 6; n++) await Promise.resolve(); };
const deferred = <T>() => { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; };

it("keeps only the selected page and forwards the server cursor exactly", async () => {
  const calls: DrawerRead[] = [];
  const model = new TeamDrawerRead(async (read) => { calls.push(read); return { ...product, after: read.after, next: read.after ? null : "t1.snapshot-ceiling" }; });
  model.open(product.team, "inbox"); await settle();
  model.next(); await settle();
  expect(calls.map((read) => read.after)).toEqual([null, "t1.snapshot-ceiling"]);
  expect(model.page?.rows).toHaveLength(product.rows.length);
  model.refresh(); await settle(); expect(calls.at(-1)?.after).toBe("t1.snapshot-ceiling");
  model.first(); await settle(); expect(calls.at(-1)?.after).toBeNull();
});

it("a closed or replaced scope cannot publish its in-flight page", async () => {
  const old = deferred<DrawerPage>(); const current = deferred<DrawerPage>(); let calls = 0;
  const model = new TeamDrawerRead(() => (++calls === 1 ? old.promise : current.promise));
  model.open(product.team, "inbox"); model.open(product.team, "cards");
  old.resolve(product); await settle();
  expect(model.page).toBeUndefined(); expect(model.loading).toBe(true);
  model.close(); current.resolve(pages.cards as DrawerPage); await settle();
  expect(model.page).toBeUndefined(); expect(model.loading).toBe(false);
});

it("many changes coalesce behind one physical read", async () => {
  const first = deferred<DrawerPage>(); const reads: DrawerRead[] = [];
  const model = new TeamDrawerRead((read) => { reads.push(read); return reads.length === 1 ? first.promise : Promise.resolve({ ...product, ...read }); });
  model.open(product.team, "inbox");
  for (let n = 0; n < 100; n++) model.refresh();
  expect(reads).toHaveLength(1);
  first.resolve(product); await settle();
  expect(reads).toHaveLength(2); expect(model.loading).toBe(false);
});

it("handwritten foreign team, wrong drawer, wrong cursor and over-cap pages fail closed", async () => {
  for (const page of [{ ...product, team: "other" }, { ...product, kind: "cards" as const }, { ...product, after: "t1.foreign" }, { ...product, rows: Array(201).fill(product.rows[0]) }]) {
    const model = new TeamDrawerRead(async () => page);
    model.open(product.team, "inbox"); await settle();
    expect(model.page).toBeUndefined(); expect(model.error).toContain("did not match");
  }
});

it("a read failure remains visible and never becomes an empty success or a retry", async () => {
  let calls = 0;
  const model = new TeamDrawerRead(async () => { calls++; throw new Error("storage_failed: unknown"); });
  model.open(product.team, "inbox"); await settle();
  expect(calls).toBe(1); expect(model.error).toContain("storage_failed"); expect(model.page).toBeUndefined();
  await settle(); expect(calls).toBe(1);
});

it("a timing read preserves the selected opaque cursor and has an explicit events-only fallback", async () => {
  const calls: DrawerRead[] = []; const feed = pages.feed as DrawerPage;
  const model = new TeamDrawerRead(async (read) => {
    calls.push(read);
    if (read.recorded_timings) throw new Error("unknown option: --recorded-timings");
    return { ...feed, after: read.after, next: read.after ? null : "t1.original" };
  });
  model.open(feed.team, "feed"); await settle(); model.next(); await settle(); model.timings(true); await settle();
  expect(calls.at(-1)).toMatchObject({ team: feed.team, kind: "feed", after: "t1.original", recorded_timings: true, timing_seqs: feed.rows.map((row) => row.seq) });
  expect(model.error).toContain("unknown option"); expect(model.page?.after).toBe("t1.original");
  expect(calls).toHaveLength(3); model.timings(false); await settle();
  expect(calls).toHaveLength(3); expect(model.timingsRequested()).toBe(false); expect(model.error).toBe("");
});

it("handwritten missing or future timing projections fail as unavailable rather than empty facts", async () => {
  for (const timings of [undefined, { schema: 2, revision: 1 }]) {
    const feed = pages.feed as DrawerPage; const model = new TeamDrawerRead(async (read) => ({ ...feed, after: read.after, timings: read.recorded_timings ? timings : undefined }));
    model.open(feed.team, "feed"); await settle(); model.timings(true); await settle();
    expect(model.error).not.toBe(""); expect(model.page?.rows).toEqual(feed.rows);
  }
});

it("handwritten continuation envelope binds timing joins to the already displayed event IDs", async () => {
  const calls: DrawerRead[] = []; const feed = pages.feed as DrawerPage;
  const model = new TeamDrawerRead(async (read) => { calls.push(read); return { ...feed, after: read.after, next: "t1.original-window" }; });
  model.open(feed.team, "feed"); await settle(); model.timings(true); await settle();
  expect(calls.at(-1)).toMatchObject({ timing_seqs: feed.rows.map((row) => row.seq), timing_next: "t1.original-window" });
});
