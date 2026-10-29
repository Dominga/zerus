import { createStore } from "solid-js/store";
import { convertFileSrc } from "@tauri-apps/api/core";
import * as bridge from "../bridge";
import type { AttachmentRef, Fetched } from "../types";

// Activity attachment thumbnails (Qt's AttachmentLoader and
// ActivityView::setAttachmentPreview): a shown thumbnail reads its file into
// the private cache once and pins it, so the cache bound never removes it
// while it shows; hiding it unpins. A copy that could not be shown (removed
// outside the client) is read once more; after that the card shows no
// thumbnail, as Qt shows none for a failed read.

export interface PreviewPort {
  fetch(computer: string, ref: AttachmentRef): Promise<Fetched>;
  pin(path: string): void;
  unpin(path: string): void;
  url(path: string): string;
}

export type PreviewState = { kind: "loading" } | { kind: "ready"; url: string; fetched: Fetched } | { kind: "failed" };

/** One file of one message of one conversation on one computer (the Fetcher's key). */
const keyOf = (computer: string, r: AttachmentRef) =>
  JSON.stringify([computer, r.name, r.conversation_id, r.archive_id, r.agent_id, r.request_id, r.index]);

export function createPreviews(port: PreviewPort) {
  const [states, setStates] = createStore<Record<string, PreviewState | undefined>>({});
  /** Replaces a state whole: a path setter would merge the old one's fields into it. */
  const put = (key: string, state: PreviewState | undefined) => setStates({ [key]: state });
  /** The thumbnails shown, with how many views show each and the copy they pinned. */
  const shown = new Map<string, { computer: string; ref: AttachmentRef; views: number; path?: string; retried: boolean }>();

  async function load(key: string) {
    const entry = shown.get(key);
    if (!entry) return;
    put(key, { kind: "loading" });
    let fetched: Fetched;
    try {
      fetched = await port.fetch(entry.computer, entry.ref);
    } catch {
      if (shown.get(key) === entry) put(key, { kind: "failed" });
      return;
    }
    // Hidden meanwhile: nothing to pin.
    if (shown.get(key) !== entry) return;
    // An image past Qt's bound, or one whose size its header does not give, shows no thumbnail (and pins nothing).
    if (!fetched.thumbnail) { put(key, { kind: "failed" }); return; }
    if (entry.path !== fetched.path) {
      port.pin(fetched.path);
      if (entry.path) port.unpin(entry.path);
      entry.path = fetched.path;
    }
    put(key, { kind: "ready", url: port.url(fetched.path), fetched });
  }

  return {
    state: (computer: string, ref: AttachmentRef): PreviewState => states[keyOf(computer, ref)] ?? { kind: "loading" },
    /** A view shows the thumbnail. */
    show(computer: string, ref: AttachmentRef) {
      const key = keyOf(computer, ref);
      const entry = shown.get(key);
      if (entry) { entry.views++; return; }
      shown.set(key, { computer, ref, views: 1, retried: false });
      void load(key);
    },
    /** A view no longer shows it; the last one unpins its copy and forgets it. */
    hide(computer: string, ref: AttachmentRef) {
      const key = keyOf(computer, ref);
      const entry = shown.get(key);
      if (!entry || --entry.views > 0) return;
      shown.delete(key);
      if (entry.path) port.unpin(entry.path);
      put(key, undefined);
    },
    /** The image could not load the cached copy: read it once more, then show none. */
    failed(computer: string, ref: AttachmentRef) {
      const key = keyOf(computer, ref);
      const entry = shown.get(key);
      if (!entry) return;
      if (entry.retried) { put(key, { kind: "failed" }); return; }
      entry.retried = true;
      void load(key);
    },
  };
}

export const previews = createPreviews({
  fetch: (computer, ref) => bridge.attachmentFetch(computer, ref),
  pin: (path) => bridge.attachmentPin(path),
  unpin: (path) => bridge.attachmentUnpin(path),
  url: (path) => convertFileSrc(path, "zerus-file"),
});
