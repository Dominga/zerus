import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MediaAnswer } from "../ui/sessions/media";

// Task 248: the media store asks the native side once per session and
// reference, keeps a late "loading" from hiding an event's answer, opens the
// viewer on a click and says why a file cannot open.
const calls: { request: number; computer: string; href: string; cwd: string; open: boolean }[] = [];
let answer: (request: number, open: boolean) => Partial<MediaAnswer> = () => ({});
let event: ((value: MediaAnswer) => void) | undefined;
const external: string[] = [];

vi.mock("../bridge", () => ({
  inTauri: () => true,
  sessionMedia: async (request: number, computer: string, href: string, cwd: string, open: boolean) => {
    calls.push({ request, computer, href, cwd, open });
    return { request, state: "ready", code: "", detail: "", token: "", kind: "image", size: 0, name: "", ...answer(request, open) };
  },
  sessionMediaExternal: async (token: string) => { external.push(token); },
  onSessionMedia: async (listener: (value: MediaAnswer) => void) => { event = listener; return () => {}; },
}));
vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (path: string, protocol: string) => `${protocol}://localhost/${encodeURIComponent(path)}` }));

const flush = () => new Promise((done) => setTimeout(done, 0));
const local = { sessionKey: "\nrun:1", computer: "", cwd: "/work", machine: "this computer" };
const peer = { sessionKey: "arch\nrun:2", computer: "arch", cwd: "/srv/work", machine: "arch" };

describe("media store (task 248)", () => {
  beforeEach(() => { calls.length = 0; external.length = 0; vi.resetModules(); });

  it("asks once per session and reference and serves the granted token", async () => {
    const media = await import("./media");
    answer = () => ({ token: "a".repeat(32), name: "shot.png" });
    media.previewMedia(local, "shots/shot.png");
    media.previewMedia(local, "shots/shot.png");
    await flush();
    expect(calls).toEqual([{ request: 1, computer: "", href: "shots/shot.png", cwd: "/work", open: false }]);
    expect(media.mediaTile(local, "shots/shot.png")).toMatchObject({ state: "ready", token: "a".repeat(32) });
    expect(media.mediaUrl("a".repeat(32))).toBe(`zerus-file://localhost/media%2F${"a".repeat(32)}`);
    // A click on a ready tile opens the viewer without asking again.
    media.openMedia(local, "shots/shot.png");
    expect(calls).toHaveLength(1);
    expect(media.mediaViewer()).toMatchObject({ kind: "image", name: "shot.png", remote: false });
    media.openMediaExternally(media.mediaViewer()!);
    await flush();
    expect(external).toEqual(["a".repeat(32)]);
  });

  it("waits for a peer's video click, then loads it and opens the viewer when it arrives", async () => {
    const media = await import("./media");
    const notices: string[] = [];
    media.onMediaNotice((text) => notices.push(text));
    answer = (_, open) => open ? { state: "loading", kind: "video", name: "run.mp4" } : { state: "click", kind: "video", name: "run.mp4" };
    media.previewMedia(peer, "demo/run.mp4");
    await flush();
    expect(media.mediaTile(peer, "demo/run.mp4")).toMatchObject({ state: "click" });
    media.openMedia(peer, "demo/run.mp4");
    await flush();
    expect(calls.map((call) => call.open)).toEqual([false, true]);
    expect(notices).toEqual(["Loading run.mp4 from arch…"]);
    expect(media.mediaViewer()).toBeUndefined();
    event!({ request: calls[1].request, state: "ready", code: "", detail: "", token: "b".repeat(32), kind: "video", size: 9, name: "run.mp4" });
    expect(media.mediaViewer()).toMatchObject({ kind: "video", remote: true, machine: "arch" });
  });

  it("keeps an event's answer when the late loading reply comes after it", async () => {
    const media = await import("./media");
    answer = (request) => {
      // The download finished before the command returned.
      event!({ request, state: "ready", code: "", detail: "", token: "c".repeat(32), kind: "image", size: 1, name: "a.png" });
      return { state: "loading" };
    };
    media.previewMedia(peer, "a.png");
    await new Promise((done) => setTimeout(done, 0));
    // onSessionMedia is registered by the first ask; the answer function used it.
    await flush();
    expect(media.mediaTile(peer, "a.png")).toMatchObject({ state: "ready", token: "c".repeat(32) });
  });

  it("says why a file cannot open", async () => {
    const media = await import("./media");
    const notices: string[] = [];
    media.onMediaNotice((text) => notices.push(text));
    answer = () => ({ state: "error", code: "missing" });
    media.openMedia(local, "gone.png");
    await flush();
    expect(notices).toEqual(["Not found on this computer"]);
    expect(media.mediaViewer()).toBeUndefined();
  });
});
