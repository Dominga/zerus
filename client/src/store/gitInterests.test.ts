import { describe, expect, it, vi } from "vitest";
import { createGitInterestSender } from "./gitInterests";

describe("visible Git interest delivery", () => {
  it("keeps one original IPC and replaces queued scroll updates with the final unfollow", async () => {
    let finish: (() => void) | undefined;
    const send = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const follow = createGitInterestSender(send);
    follow([{ computer: "", key: "run:selected" }]);
    follow([{ computer: "peer", key: "run:scrolled" }]);
    follow([]);
    expect(send.mock.calls).toEqual([[ [{ computer: "", key: "run:selected" }] ]]);
    finish!(); await Promise.resolve(); await Promise.resolve();
    expect(send.mock.calls).toEqual([[ [{ computer: "", key: "run:selected" }] ], [ [] ]]);
    finish!(); await Promise.resolve();
    expect(send).toHaveBeenCalledTimes(2);
  });

  it("does not retry a refused read", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      const send = vi.fn().mockRejectedValue(new Error("fixture refusal"));
      createGitInterestSender(send)([{ computer: "", key: "run:selected" }]);
      await Promise.resolve(); await Promise.resolve();
      expect(send).toHaveBeenCalledTimes(1);
      expect(warn).toHaveBeenCalledTimes(1);
    } finally { warn.mockRestore(); }
  });
});
