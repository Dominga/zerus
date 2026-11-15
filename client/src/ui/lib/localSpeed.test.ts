import { describe, expect, it } from "vitest";
import type { ModelServer, ModelServerStatus } from "../../bridge";
import { localServer, speedIn, speedText, speedTitle } from "./localSpeed";

// Owner, 2026-11-15: a local model account shows its model's speed in place of limits.

const server = (name: string, base_url: string) => ({ name, base_url } as ModelServer);
const status = (models: { name: string; tok_s?: number; tok_s_kind?: string }[]) =>
  ({ models: models.map((m) => ({ state: "running", label: { source: "" }, since: 0, usable: true, can_down: false, can_up: false, ...m })) } as ModelServerStatus);

describe("local model speed", () => {
  const servers = [server("gb10-claude", "http://192.0.2.7:4000"), server("gb10-codex", "http://192.0.2.7:4000/v1"), server("lab", "https://lab.example:8443")];

  it("finds an account's server by its gateway, then by a server of its name", () => {
    expect(localServer(servers, [{ id: "work", gateway: "http://192.0.2.7:4000/" }])).toBe("gb10-claude");
    expect(localServer(servers, [{ id: "lab" }])).toBe("lab");
    // A gateway match on any member wins over a name match on an earlier one.
    expect(localServer(servers, [{ id: "lab" }, { id: "x", gateway: "HTTP://192.0.2.7:4000/v1" }])).toBe("gb10-claude");
    expect(localServer(servers, [{ id: "native-claude" }, undefined])).toBeUndefined();
    expect(localServer(servers, [{ id: "cloud", gateway: "https://api.example" }])).toBeUndefined();
  });

  it("shows a live reading first, then the latest answer, then an average", () => {
    expect(speedIn("s", status([{ name: "a", tok_s: 80, tok_s_kind: "mean" }, { name: "b", tok_s: 40, tok_s_kind: "last" }])))
      .toEqual({ server: "s", model: "b", tok_s: 40, kind: "last" });
    expect(speedIn("s", status([{ name: "a", tok_s: 40, tok_s_kind: "last" }, { name: "b", tok_s: 20, tok_s_kind: "live" }]))?.model).toBe("b");
    expect(speedIn("s", status([{ name: "a" }, { name: "b", tok_s: 0 }]))).toBeUndefined();
    expect(speedIn("s", undefined)).toBeUndefined();
  });

  it("says the figure, what it measures, and when the server reports none", () => {
    const speed = { server: "gb10-claude", model: "qwen3.8-flash-next", tok_s: 56.63, kind: "last" };
    expect(speedText(speed)).toBe("56.6 tok/s");
    expect(speedText({ ...speed, tok_s: 123.4 })).toBe("123 tok/s");
    expect(speedText(undefined)).toBe("local model");
    expect(speedTitle("gb10-claude", speed)).toBe("qwen3.8-flash-next on gb10-claude generated its latest answer at 56.6 tokens per second.");
    expect(speedTitle("gb10-claude", { ...speed, kind: "live" })).toBe("qwen3.8-flash-next on gb10-claude generates 56.6 tokens per second now.");
    expect(speedTitle("gb10-claude", undefined)).toBe("A local model on gb10-claude: it reports no speed yet.");
  });
});
