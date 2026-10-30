import { describe, expect, it, vi } from "vitest";
import { createVoice, type VoiceOut, type VoicePort, type VoiceShown } from "./voice";
import { mediaPlayback } from "./mediaPlayback";

const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const context = { handle: 1, key: "peer\ncodex/sample-project/review", visible: true };
const shown = (extra: Partial<VoiceShown> = {}) => ({ key: context.key, id: "recording-1", state: "preview", ...extra } as VoiceShown);
function setup() {
  let receive!: (event: VoiceOut) => void, frame!: (bytes: Uint8Array) => void;
  const port: VoicePort = {
    open: vi.fn(async (_owner, out, jpeg) => { receive = out; frame = jpeg; }),
    input: vi.fn(async () => {}), close: vi.fn(async () => {}), remember: vi.fn(), notice: vi.fn(),
  };
  const store = createVoice(port, "private-owner");
  return { store, port, out: (event: VoiceOut) => receive(event), frame: (bytes: Uint8Array) => frame(bytes) };
}

describe("the main session's voice driver", () => {
  it("recording cancels a pending Activity player before its read can autoplay", async () => {
    const stop = vi.fn(), pending = mediaPlayback.take(stop);
    const a = setup(); a.store.follow(context); await tick();
    a.out({ kind: "pause_messages" });
    expect(stop).toHaveBeenCalledTimes(1);
    expect(pending.current()).toBe(false);
    await a.store.close();
  });
  it("opens once, coalesces contexts and never adopts a stale session's view", async () => {
    const a = setup();
    a.store.follow(context); a.store.follow({ ...context }); await tick();
    expect(a.port.open).toHaveBeenCalledTimes(1);
    expect(a.port.input).toHaveBeenCalledExactlyOnceWith("private-owner", 1, { kind: "update", input_visible: true });
    a.out({ kind: "shown", shown: shown() }); expect(a.store.shown()?.id).toBe("recording-1");
    a.store.follow({ ...context, handle: 2, key: "peer\ncodex/sample-project/next" }); await tick();
    expect(a.store.shown()).toBeUndefined();
    a.out({ kind: "shown", shown: shown() }); expect(a.store.shown()).toBeUndefined();
    expect(a.port.open).toHaveBeenCalledTimes(1);
    await a.store.close(); expect(a.port.close).toHaveBeenCalledExactlyOnceWith("private-owner");
  });
  it("holds only the newest pending context while the native open is in flight", async () => {
    const a = setup(); let ready!: () => void;
    vi.mocked(a.port.open).mockImplementationOnce(() => new Promise((resolve) => { ready = resolve; }));
    a.store.follow(context); a.store.follow({ ...context, handle: 2 }); a.store.follow({ ...context, handle: 3 });
    ready(); await tick();
    expect(a.port.input).toHaveBeenCalledExactlyOnceWith("private-owner", 3, { kind: "update", input_visible: true });
  });
  it("never retries an uncertain Send, and keeps its explicit Check as another user action", async () => {
    const a = setup(); a.store.follow(context); await tick();
    vi.mocked(a.port.input).mockRejectedValueOnce("The answer is uncertain");
    await a.store.input({ kind: "send" }); await tick();
    expect(a.port.input).toHaveBeenCalledTimes(2);
    expect(a.port.notice).toHaveBeenCalledExactlyOnceWith("The answer is uncertain");
    a.out({ kind: "shown", shown: shown({ state: "uncertain", send_label: "Check message" }) });
    expect(a.store.shown()?.send_label).toBe("Check message");
    expect(a.port.input).toHaveBeenCalledTimes(2);
    await a.store.input({ kind: "send" }); expect(a.port.input).toHaveBeenCalledTimes(3);
  });
  it("returns an exact confirmation ticket, drops old dialogs, and remembers only explicit profile outputs", async () => {
    const a = setup(); a.store.follow(context); await tick(); a.out({ kind: "shown", shown: shown() });
    a.out({ kind: "ask", ask: { kind: "discard", session: "codex/sample-project/review", ticket: 7 } });
    a.store.confirm(true); await tick();
    expect(a.port.input).toHaveBeenLastCalledWith("private-owner", 1, { kind: "confirm", ticket: 7, confirmed: true });
    const profile = { machine_id: "machine-1", profile_id: "profile-1", profile_revision: 3 };
    a.out({ kind: "remember_profile", profile }); expect(a.port.remember).toHaveBeenCalledExactlyOnceWith(profile);
    a.store.follow({ ...context, handle: 2, key: "new" }); await tick();
    a.out({ kind: "ask", ask: { kind: "cold_cache", warning: "Old warning", ticket: 8 } }); await tick();
    expect(a.store.ask()).toBeUndefined();
    expect(a.port.input).toHaveBeenLastCalledWith("private-owner", 2, { kind: "confirm", ticket: 8, confirmed: false });
  });
  it("keeps only the latest frame of the current video and ignores outputs after closing", async () => {
    const a = setup(); a.store.follow(context); await tick();
    a.out({ kind: "shown", shown: shown({ video_preview: true, state: "recording" }) });
    a.frame(new Uint8Array([1])); a.frame(new Uint8Array([2])); expect(a.store.frame()).toEqual(new Uint8Array([2]));
    a.out({ kind: "permission_recovery" }); expect(a.store.permission()).toBe(true);
    a.store.closePermission(); expect(a.store.permission()).toBe(false);
    await a.store.close(); a.frame(new Uint8Array([3])); a.out({ kind: "shown", shown: shown() });
    expect(a.store.frame()).toBeUndefined(); expect(a.store.shown()).toBeUndefined();
  });
  it("stops private capture on losing the composer, without a send or automatic reopening", async () => {
    const a = setup(); a.store.follow(context); await tick(); a.store.follow(undefined); await tick();
    expect(a.port.input).toHaveBeenLastCalledWith("private-owner", 1, { kind: "stop", forget_send: true });
    expect(a.port.open).toHaveBeenCalledTimes(1);
    await a.store.close();
  });
});
