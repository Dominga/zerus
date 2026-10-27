import { invoke } from "@tauri-apps/api/core";
import { emptyBatch, isEmptyBatch, type Batch, type ComputerId } from "./types";
import type { ActivityPort, FollowRequest } from "./store/activity";

// The one path from the Rust side to the stores. The frontend asks for the next
// batch from requestAnimationFrame with one call in flight; `next_batch`
// parks up to 250 ms while nothing is pending, so an idle client does not
// poll. A batch is applied in the frame after it arrives, at most one per
// frame. Outside Tauri (`npm run dev` in a browser) a built-in replay of the
// synthetic fixture stands in, so the UI works without the app.

export type BridgeMode = "tauri" | "replay";

interface Source extends ActivityPort {
  next(): Promise<Batch>;
}

let source: Source | undefined;

export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** `next_batch` answers with raw JSON bytes (`ipc::Response`), so large batches skip script evaluation. */
function decodeBatch(reply: unknown): Batch {
  if (reply instanceof ArrayBuffer) return JSON.parse(new TextDecoder().decode(reply)) as Batch;
  if (ArrayBuffer.isView(reply)) return JSON.parse(new TextDecoder().decode(reply)) as Batch;
  if (Array.isArray(reply)) return JSON.parse(new TextDecoder().decode(Uint8Array.from(reply as number[]))) as Batch;
  return reply as Batch;
}

const tauriSource: Source = {
  next: () => invoke<unknown>("next_batch").then(decodeBatch),
  follow: (request: FollowRequest) => invoke<void>("follow", { ...request }),
  unfollow: (computer: ComputerId, key: string) => invoke<void>("unfollow", { computer, key }),
};

/** Starts the data source and the per-frame pump; resolves with the mode in use. */
export async function startBridge(onBatch: (b: Batch) => void): Promise<BridgeMode> {
  let mode: BridgeMode = "replay";
  if (inTauri()) {
    try {
      await invoke("start");
      source = tauriSource;
      mode = "tauri";
    } catch (error) {
      // A build without the live feature has no data source of its own.
      console.warn("Zerus Next: no live data in this build, using the built-in replay.", error);
    }
  }
  if (!source) {
    const { createReplay } = await import("./dev/replay");
    source = createReplay();
  }
  pump(source, onBatch);
  return mode;
}

/**
 * One `next_batch` in flight. A batch is applied in the next animation frame,
 * which then asks for the following one; an empty answer (the 250 ms park ran
 * out) asks again at once without a frame, so an idle window schedules no
 * frames. While the window is hidden no frame runs, nothing is asked, and the
 * Rust side keeps folding changes into the next batch.
 */
function pump(from: Source, onBatch: (b: Batch) => void) {
  const pull = () => {
    from.next().then(
      (reply) => {
        const b = normalize(reply ?? {});
        if (isEmptyBatch(b)) { pull(); return; }
        // In the next frame; a window whose frames are throttled (occluded)
        // still applies it within 250 ms, so the stores never fall far behind.
        let applied = false;
        const apply = () => {
          if (applied) return;
          applied = true;
          clearTimeout(late);
          try { onBatch(b); } catch (error) { console.error("Zerus Next: applying a batch failed", error); }
          pull();
        };
        const late = setTimeout(apply, 250);
        requestAnimationFrame(apply);
      },
      (error) => {
        console.error("Zerus Next: next_batch failed", error);
        setTimeout(pull, 1000);
      },
    );
  };
  pull();
}

/** Missing arrays default to empty, so a core that omits empty fields still applies. */
function normalize(b: Partial<Batch>): Batch {
  const e = emptyBatch();
  return { upserts: b.upserts ?? e.upserts, removes: b.removes ?? e.removes, replaces: b.replaces ?? e.replaces, computers: b.computers ?? e.computers, activity: b.activity ?? e.activity };
}

/** The Activity port of the current source (follow and unfollow). */
export const activityPort: ActivityPort = {
  follow: (request) => source?.follow(request) ?? Promise.resolve(),
  unfollow: (computer, key) => source?.unfollow(computer, key) ?? Promise.resolve(),
};

/** Process RSS and CPU of the app, when running in Tauri. */
export async function metrics(windowMs?: number): Promise<Record<string, unknown> | undefined> {
  if (!inTauri()) return undefined;
  try { return await invoke<Record<string, unknown>>("metrics", windowMs ? { window_ms: windowMs } : {}); } catch { return undefined; }
}
