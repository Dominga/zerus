import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createPreviews, type PreviewPort } from "./attachments";
import type { AttachmentRef, Fetched } from "../types";

const ref: AttachmentRef = { name: "claude/sample-project/review", request_id: "req-1", index: 0, conversation_id: "conv-1", archive_id: null, agent_id: null };
const fetched = (path: string): Fetched => ({ path, name: "shot.png", mime: "image/png", bytes: 8, thumbnail: true });

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

/** A port whose reads answer in turn from `results`; an Error rejects with its message. */
function fakePort(results: (string | Error)[]) {
  const calls: string[] = [];
  const port: PreviewPort = {
    fetch: async (computer) => {
      calls.push(`fetch ${computer}`);
      const next = results.shift();
      if (next === undefined || next instanceof Error) throw next?.message ?? "nothing left";
      return fetched(next);
    },
    pin: (path) => { calls.push(`pin ${path}`); },
    unpin: (path) => { calls.push(`unpin ${path}`); },
    url: (path) => `zerus-file://localhost/${path}`,
  };
  return { port, calls };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("Activity attachment thumbnails", () => {
  it("pin the copy while shown and unpin it when the last view hides it", async () => {
    const f = fakePort(["cache/abc/shot.png"]);
    await createRoot(async (dispose) => {
      const p = createPreviews(f.port);
      p.show("studio", ref);
      p.show("studio", ref);
      await tick();
      expect(p.state("studio", ref)).toMatchObject({ kind: "ready", url: "zerus-file://localhost/cache/abc/shot.png" });
      p.hide("studio", ref);
      expect(f.calls).toEqual(["fetch studio", "pin cache/abc/shot.png"]);
      p.hide("studio", ref);
      expect(f.calls).toEqual(["fetch studio", "pin cache/abc/shot.png", "unpin cache/abc/shot.png"]);
      expect(p.state("studio", ref).kind).toBe("loading");
      dispose();
    });
  });

  it("read a copy that could not be shown once more, then show none", async () => {
    const f = fakePort(["cache/abc/shot.png", "cache/abc/shot.png", new Error("Invalid attachment response. Try again.")]);
    await createRoot(async (dispose) => {
      const p = createPreviews(f.port);
      p.show("studio", ref);
      await tick();
      p.failed("studio", ref);
      await tick();
      expect(p.state("studio", ref).kind).toBe("ready");
      expect(f.calls.filter((c) => c.startsWith("fetch")).length).toBe(2);
      expect(f.calls.filter((c) => c.startsWith("pin")).length).toBe(1);
      p.failed("studio", ref);
      await tick();
      expect(p.state("studio", ref)).toEqual({ kind: "failed" });
      expect(f.calls.filter((c) => c.startsWith("fetch")).length).toBe(2);
      dispose();
    });
  });

  it("show nothing for a read that failed", async () => {
    const f = fakePort([new Error("This attachment has no saved file reference.")]);
    await createRoot(async (dispose) => {
      const p = createPreviews(f.port);
      p.show("studio", ref);
      await tick();
      expect(p.state("studio", ref)).toEqual({ kind: "failed" });
      expect(f.calls).toEqual(["fetch studio"]);
      dispose();
    });
  });

  it("pin nothing and show no thumbnail for an image past Qt's bound", async () => {
    const calls: string[] = [];
    const port: PreviewPort = {
      fetch: async () => ({ ...fetched("cache/abc/huge.png"), thumbnail: false }),
      pin: (path) => calls.push(`pin ${path}`), unpin: (path) => calls.push(`unpin ${path}`), url: (path) => path,
    };
    await createRoot(async (dispose) => {
      const p = createPreviews(port);
      p.show("studio", ref);
      await tick();
      expect(p.state("studio", ref)).toEqual({ kind: "failed" });
      p.hide("studio", ref);
      expect(calls).toEqual([]);
      dispose();
    });
  });

  it("pin nothing when the thumbnail was hidden before its read answered", async () => {
    const answer = deferred<Fetched>();
    const calls: string[] = [];
    const port: PreviewPort = {
      fetch: () => answer.promise, pin: (path) => calls.push(`pin ${path}`), unpin: (path) => calls.push(`unpin ${path}`), url: (path) => path,
    };
    await createRoot(async (dispose) => {
      const p = createPreviews(port);
      p.show("studio", ref);
      p.hide("studio", ref);
      answer.resolve(fetched("cache/abc/shot.png"));
      await tick();
      expect(calls).toEqual([]);
      dispose();
    });
  });

  it("keep another computer's file of the same name apart", async () => {
    const f = fakePort(["cache/abc/shot.png", "cache/def/shot.png"]);
    await createRoot(async (dispose) => {
      const p = createPreviews(f.port);
      p.show("studio", ref);
      p.show("mini", ref);
      await tick();
      expect(f.calls.filter((c) => c.startsWith("fetch"))).toEqual(["fetch studio", "fetch mini"]);
      p.hide("studio", ref);
      expect(p.state("mini", ref).kind).toBe("ready");
      dispose();
    });
  });
});
