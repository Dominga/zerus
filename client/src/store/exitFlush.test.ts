import { describe, expect, it } from "vitest";
import { createExitFlush } from "./exitFlush";

describe("the page's quit handshake", () => {
  it("listens once and acknowledges only after every registered composer flush", async () => {
    const calls: string[] = []; let request!: (nonce: string) => Promise<void>;
    let endA!: () => void, endB!: () => void;
    const a = new Promise<void>((r) => { endA = r; }), b = new Promise<void>((r) => { endB = r; });
    const exit = createExitFlush({ listen: async (f) => { request = f; calls.push("listen"); return () => {}; }, ack: async (n) => { calls.push(`ack ${n}`); } });
    exit.register(async () => { calls.push("edit A"); await a; calls.push("flushed A"); });
    exit.register(async () => { calls.push("edit B"); await b; calls.push("flushed B"); });
    await exit.start(); const pending = request("nonce-a");
    await Promise.resolve(); expect(calls).toEqual(["listen", "edit A", "edit B"]);
    endA(); await Promise.resolve(); expect(calls).not.toContain("ack nonce-a");
    endB(); await pending;
    expect(calls).toEqual(["listen", "edit A", "edit B", "flushed A", "flushed B", "ack nonce-a"]);
    await request("nonce-a"); expect(calls.filter((c) => c.startsWith("ack"))).toEqual(["ack nonce-a"]);
  });

  it("a removed participant no longer flushes and a failed one cannot block the others", async () => {
    const calls: string[] = []; let request!: (nonce: string) => Promise<void>;
    const exit = createExitFlush({ listen: async (f) => { request = f; return () => {}; }, ack: async () => { calls.push("ack"); } });
    const remove = exit.register(async () => { calls.push("removed"); }); remove();
    exit.register(async () => { throw new Error("closed composer"); });
    exit.register(async () => { calls.push("flush"); });
    await exit.start(); await request("nonce-b"); expect(calls).toEqual(["flush", "ack"]);
  });
});
