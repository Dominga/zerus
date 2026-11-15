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
