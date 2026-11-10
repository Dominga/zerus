import { describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { createHeldReview, type HeldAnswer, type HeldCursor, type HeldInputsSource, type HeldPage, type HeldRow } from "./heldInputs";
import { heldInputsNotice, serviceStatus } from "./desktopService";
import type { DesktopService } from "../types";

const row = (n: number, file: HeldRow["file"] = "present"): HeldRow => ({
  request_id: `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`, created: 1_700_000_000_000 + n,
  bytes: 70_000 + n, state: n % 2 ? "failed" : "orphaned", file,
});
const cursor = (r: HeldRow): HeldCursor => ({ created: r.created, request_id: r.request_id });
/** Pages answered in order, each numbered as the native listing would be. */
function source(pages: Omit<HeldPage, "listing">[][], answers: (HeldAnswer | Error)[] = []) {
  let listings = 0;
  const page = vi.fn(async (after: HeldCursor | null): Promise<HeldPage> => {
    const listing = ++listings;
    const next = pages.shift();
    if (!next) throw new Error("unexpected read");
    const [value] = next;
    if (!value) throw new Error("read failed");
    void after;
    return { listing, ...value };
  });
  const release = vi.fn(async (_id: string, _listing: number) => {
    const answer = answers.shift();
    if (!answer || answer instanceof Error) throw answer ?? new Error("unexpected release");
    return answer;
  });
  return { page, release } satisfies HeldInputsSource;
}
const review = (s: HeldInputsSource) => createRoot(() => createHeldReview(s));

describe("held message copy review", () => {
  it("reads one page per explicit read and continues from its cursor", async () => {
    const first = [row(1), row(2)], second = [row(3)];
    const s = source([[{ rows: first, next: cursor(row(2)) }], [{ rows: second, next: null }]]);
    const r = review(s);
    expect(s.page).not.toHaveBeenCalled();
    await r.read(0, null);
    expect(r.pages().map((p) => p.rows.length)).toEqual([2]);
    expect(s.page).toHaveBeenCalledTimes(1);
    await r.read(r.pages().length, r.pages()[0].next);
    expect(s.page).toHaveBeenLastCalledWith(cursor(row(2)));
    expect(r.pages().flatMap((p) => p.rows)).toEqual([...first, ...second]);
    expect(s.page).toHaveBeenCalledTimes(2);
  });

  it("allows one read at a time", async () => {
    const s = source([[{ rows: [row(1)], next: null }]]);
    const r = review(s);
    const reading = r.read(0, null);
    expect(r.loading()).toBe(0);
    await r.read(0, null);
    await reading;
    expect(s.page).toHaveBeenCalledTimes(1);
    expect(r.loading()).toBeUndefined();
  });

  it("sends each confirmation once and never retries an unknown result", async () => {
    const s = source([[{ rows: [row(1), row(2)], next: null }]], [new Error("lost"), "changed"]);
    const r = review(s);
    await r.read(0, null);
    const listing = r.pages()[0].listing;
    const sending = r.release(row(1), listing);
    expect(r.outcomes()[row(1).request_id]).toBe("sending");
    await r.release(row(1), listing);
    await sending;
    expect(r.outcomes()[row(1).request_id]).toBe("unknown");
    await r.release(row(1), listing);
    expect(s.release).toHaveBeenCalledTimes(1);
    expect(s.release).toHaveBeenCalledWith(row(1).request_id, listing);
    await r.release(row(2), listing);
    expect(r.outcomes()[row(2).request_id]).toBe("changed");
    expect(s.release).toHaveBeenCalledTimes(2);
  });

  it("Review again reads that page from its own cursor and drops later pages", async () => {
    const s = source([
      [{ rows: [row(1)], next: cursor(row(1)) }],
      [{ rows: [row(2)], next: cursor(row(2)) }],
      [{ rows: [row(4)], next: null }],
      [{ rows: [row(3)], next: null }],
    ], ["kept"]);
    const r = review(s);
    await r.read(0, null);
    await r.read(1, cursor(row(1)));
    await r.read(2, cursor(row(2)));
    await r.release(row(2), r.pages()[1].listing);
    expect(s.release).toHaveBeenLastCalledWith(row(2).request_id, 2);
    expect(r.outcomes()[row(2).request_id]).toBe("kept");
    await r.read(1, r.pages()[1].after);
    expect(r.pages()[1].listing).toBe(4);
    expect(s.page).toHaveBeenLastCalledWith(cursor(row(1)));
    expect(r.pages().map((p) => p.rows.map((x) => x.request_id))).toEqual([[row(1).request_id], [row(3).request_id]]);
    expect(r.outcomes()).toEqual({});
    expect(s.release).toHaveBeenCalledTimes(1);
  });

  it("keeps earlier pages and offers the failed page again after a failed read", async () => {
    const s = source([[{ rows: [row(1)], next: cursor(row(1)) }], [undefined as unknown as HeldPage], [{ rows: [row(2)], next: null }]]);
    const r = review(s);
    await r.read(0, null);
    await r.read(1, cursor(row(1)));
    expect(r.failed()).toEqual({ index: 1, after: cursor(row(1)) });
    expect(r.pages()).toHaveLength(1);
    await r.read(r.failed()!.index, r.failed()!.after);
    expect(r.failed()).toBeUndefined();
    expect(r.pages()).toHaveLength(2);
  });

  it("ignores answers that arrive after the dialog closed", async () => {
    let open = true;
    const s = source([[{ rows: [row(1)], next: null }]]);
    const r = createRoot(() => createHeldReview(s, () => open));
    const reading = r.read(0, null);
    open = false;
    await reading;
    expect(r.pages()).toEqual([]);
  });
});

const connected = (patch: Partial<DesktopService> = {}): DesktopService => ({ connected: true, epoch: "fixture", generation: 1,
  background: Object.fromEntries(["fleet", "presence", "reads", "attention", "approvals"].map((key) => [key, { ready: true, code: "ready" }])),
  attention_counts: null, automatic_approvals: false, details_available: true, code: "", ...patch });

describe("held message copies in the service status", () => {
  it("offers the review only while the service reports copies to review", () => {
    for (const held of [undefined, null, { review: 0, full: false }, { review: 0, full: true }]) {
      expect(heldInputsNotice(connected({ task_message_inputs: held }))).toBe("");
      expect(serviceStatus(connected({ task_message_inputs: held }))).toMatchObject({ healthy: true, inputsReview: false });
    }
    expect(heldInputsNotice(connected({ connected: false, task_message_inputs: { review: 2, full: true } }))).toBe("");
  });
  it("names the paused large messages only when the limit is reached", () => {
    const some = serviceStatus(connected({ task_message_inputs: { review: 1, full: false } }));
    expect(some).toMatchObject({ healthy: false, inputsReview: true, retry: false, label: "Held message copies" });
    expect(some?.details).toBe("Some held task message copies need review.");
    const full = serviceStatus(connected({ task_message_inputs: { review: 3, full: true } }));
    expect(full?.details).toBe("Large task messages are paused: some held message copies need review.");
    expect(full).toMatchObject({ inputsReview: true, retry: false });
  });
  it("keeps a not-ready service's label and Retry, and names the copies without a review it cannot list", () => {
    const status = serviceStatus(connected({ code: "background_unready", task_message_inputs: { review: 1, full: false } }));
    expect(status).toMatchObject({ label: "Background service not ready", retry: true, inputsReview: false });
    expect(status?.details).toContain("Some held task message copies need review.");
  });
});
