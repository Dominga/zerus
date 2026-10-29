import { describe, expect, it } from "vitest";
import { saveOutcome } from "./recovery-wait";

describe("waiting for a recovery save", () => {
  it("ignores a read's answer that leaves the service idle", () => {
    expect(saveOutcome(3, { busy: false, error: "", saves: 3 })).toEqual({ done: false });
  });
  it("waits while copies are still being synchronized", () => {
    expect(saveOutcome(3, { busy: true, error: "", saves: 4 })).toEqual({ done: false });
  });
  it("reports the save, or why it failed", () => {
    expect(saveOutcome(3, { busy: false, error: "", saves: 4 })).toEqual({ done: true, message: "Saved.", saved: true });
    expect(saveOutcome(3, { busy: false, error: "Recovery settings changed on this machine; reload them", saves: 4 }))
      .toEqual({ done: true, message: "Recovery settings changed on this machine; reload them", saved: false });
  });
});
