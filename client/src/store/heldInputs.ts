import { createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

/** A stable listing position: creation time, then request ID. */
export interface HeldCursor { created: number; request_id: string }
/** One held task message copy, as the native review observed its slot. */
export interface HeldRow {
  request_id: string;
  /** Milliseconds since the epoch. */
  created: number;
  bytes: number;
  state: "orphaned" | "failed";
  file: "present" | "missing" | "unsafe";
}
/** One page and the native listing that observed it; a release names that listing. */
export interface HeldPage { listing: number; rows: HeldRow[]; next: HeldCursor | null }
/** What one confirmed release did; nothing is ever retried. */
export type HeldAnswer = "removed" | "released" | "changed" | "kept" | "unknown" | "not_sent";

export interface HeldInputsSource {
  /** One bounded read-only page, only ever on an explicit click. */
  page(after: HeldCursor | null): Promise<HeldPage>;
  /** Sends exactly what `listing` observed of this record, once. */
  release(requestId: string, listing: number): Promise<HeldAnswer>;
}

export const heldInputs: HeldInputsSource = {
  page: (after) => invoke<HeldPage>("desktop_service_task_message_inputs", { after }),
  release: (request_id, listing) => invoke<HeldAnswer>("desktop_service_task_message_input_release", { request_id, listing }),
};

export type HeldReviewPage = { after: HeldCursor | null; listing: number; rows: HeldRow[]; next: HeldCursor | null };

/** The review dialog's state: pages read one per click and one release per
 * confirmation. `alive` is false once the dialog closed. */
export function createHeldReview(source: HeldInputsSource, alive: () => boolean = () => true) {
  const [pages, setPages] = createSignal<HeldReviewPage[]>([]);
  const [loading, setLoading] = createSignal<number>();
  const [failed, setFailed] = createSignal<{ index: number; after: HeldCursor | null }>();
  const [outcomes, setOutcomes] = createSignal<Record<string, HeldAnswer | "sending">>({});
  /** Reads page `index` from its own cursor again; it and every later page
   * are replaced, so no record is listed twice. One read at a time. */
  const read = (index: number, after: HeldCursor | null): Promise<void> => {
    if (loading() !== undefined) return Promise.resolve();
    setLoading(index);
    setFailed(undefined);
    const replaced = pages().slice(index).flatMap((page) => page.rows.map((row) => row.request_id));
    return source.page(after).then((page) => {
      if (!alive()) return;
      setPages([...pages().slice(0, index), { after, listing: page.listing, rows: page.rows, next: page.next }]);
      setOutcomes((all) => {
        const next = { ...all };
        for (const id of replaced) if (next[id] !== "sending") delete next[id];
        return next;
      });
    }, () => {
      if (!alive()) return;
      setPages(pages().slice(0, index));
      setFailed({ index, after });
    }).finally(() => { if (alive()) setLoading(undefined); });
  };
  /** One confirmed release of `row` as `listing` observed it: sent once and
   * never retried. */
  const release = (row: HeldRow, listing: number): Promise<void> => {
    if (outcomes()[row.request_id]) return Promise.resolve();
    setOutcomes((all) => ({ ...all, [row.request_id]: "sending" }));
    return source.release(row.request_id, listing).catch((): HeldAnswer => "unknown").then((answer) => {
      if (alive()) setOutcomes((all) => ({ ...all, [row.request_id]: answer }));
    });
  };
  return { pages, loading, failed, outcomes, read, release };
}
