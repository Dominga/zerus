import { describe, expect, it } from "vitest";
import { recordingBlob } from "./voice-playback";

describe("native recording playback", () => {
  it("keeps the raw recording bytes with the native voice or video type", async () => {
    const bytes = new Uint8Array([1, 2]).buffer;
    const voice = recordingBlob(bytes, false);
    expect(voice.type).toBe("audio/wav");
    expect(new Uint8Array(await voice.arrayBuffer())).toEqual(new Uint8Array([1, 2]));
    expect(recordingBlob(bytes, true).type).toBe("video/mp4");
  });
  it("refuses an empty or oversized preview and accepts the maximum recording", () => {
    expect(() => recordingBlob(new ArrayBuffer(0), false)).toThrow("Could not read");
    expect(() => recordingBlob(new ArrayBuffer(10 * 1024 * 1024 + 1), true)).toThrow("Could not read");
    expect(recordingBlob(new ArrayBuffer(10 * 1024 * 1024), true).size).toBe(10 * 1024 * 1024);
  });
});
