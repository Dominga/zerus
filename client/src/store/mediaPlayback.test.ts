import { describe, expect, it, vi } from "vitest";
import { createPlaybackOwner } from "./mediaPlayback";

describe("recording playback ownership", () => {
  it("cancels the old player before a new asynchronous read can play", async () => {
    const owner = createPlaybackOwner(), firstStop = vi.fn(), secondStop = vi.fn();
    const first = owner.take(firstStop);
    let resolve!: () => void;
    const read = new Promise<void>((done) => { resolve = done; });
    const play = vi.fn();
    const pending = read.then(() => { if (first.current()) play(); });
    const second = owner.take(secondStop);
    expect(firstStop).toHaveBeenCalledTimes(1);
    first.release();
    expect(second.current()).toBe(true);
    resolve(); await pending;
    expect(play).not.toHaveBeenCalled();
    owner.stop(); expect(secondStop).toHaveBeenCalledTimes(1);
    expect(second.current()).toBe(false);
  });
  it("a recording, navigation or unmount cancels a pending load without replay", () => {
    const owner = createPlaybackOwner(), stop = vi.fn();
    const load = owner.take(stop);
    owner.stop(); owner.stop(); load.release();
    expect(stop).toHaveBeenCalledTimes(1);
    expect(load.current()).toBe(false);
  });
  it("releasing the current player leaves no retained owner", () => {
    const owner = createPlaybackOwner(), stop = vi.fn();
    const current = owner.take(stop); current.release(); owner.stop();
    expect(stop).not.toHaveBeenCalled();
    expect(current.current()).toBe(false);
  });
  it("recording pauses a loaded player without losing its position or source", () => {
    const owner = createPlaybackOwner(), stop = vi.fn(), pause = vi.fn();
    const current = owner.take(stop, pause);
    owner.pause();
    expect(pause).toHaveBeenCalledTimes(1);
    expect(stop).not.toHaveBeenCalled();
    expect(current.current()).toBe(true);
    owner.stop(); expect(current.current()).toBe(false);
  });
});
