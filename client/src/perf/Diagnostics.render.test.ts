// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";

const probe = vi.hoisted(() => ({ run: vi.fn(), estimate: vi.fn().mockResolvedValue(60) }));
vi.mock("./scenarios", async (original) => ({ ...await original<typeof import("./scenarios")>(), runAll: probe.run, estimateHz: probe.estimate }));
import Diagnostics from "./Diagnostics";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); vi.clearAllMocks(); });
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };
function setup(displayHz: () => Promise<number | null>) {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(Diagnostics, { displayHz, scrollList: () => null, scrollActivity: () => null, openSession: async () => {} }), host);
  probe.run.mockRejectedValue(new Error("measurement reached"));
}

it("uses the mapped monitor's native rate after an unavailable startup probe and a 60 Hz estimate", async () => {
  const displayHz = vi.fn().mockResolvedValueOnce(null).mockResolvedValue(144);
  setup(displayHz); await flush();
  await expect(globalThis.zerusPerf!.run({ ids: ["raf-light"] })).rejects.toThrow("measurement reached");
  expect(probe.run.mock.calls[0][2]).toMatchObject({ hz: 144, hzSource: "native" });
});

it("preserves an explicitly selected manual budget", async () => {
  setup(vi.fn().mockResolvedValue(144)); await flush();
  await expect(globalThis.zerusPerf!.run({ ids: ["raf-light"], hz: 120, hzSource: "manual" })).rejects.toThrow("measurement reached");
  expect(probe.run.mock.calls[0][2]).toMatchObject({ hz: 120, hzSource: "manual" });
});
