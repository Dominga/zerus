import { describe, expect, it } from "vitest";
import { historyLine } from "./history";

// Task 52: the history lines of the two team events added by tasks 22 and 36,
// as Qt's feedLine words them (without its separator dots).

const tasks = [{ id: 1, title: "Saved cards API" }];

describe("team history lines", () => {
  it("a start that waits for its host says which try it is", () => {
    const line = historyLine({ kind: "launch_waiting", detail: { member: "Backend", reason: "hgs: DeepSeek host is still starting", wait: 2, of: 20 } }, tasks);
    expect(line).toEqual({ text: "The host is still starting; starting again (2 of 20)", by: "Backend", tone: "muted" });
  });

  it("a member woken by its own settled run names the task and the attempt; a refused wake warns", () => {
    expect(historyLine({ kind: "run_settled_woken", detail: { member: "Backend", task: 1, attempt: 2, run: "r1", delivered: true } }, tasks))
      .toEqual({ text: "Woke Backend: its run settled (#1 Saved cards API, attempt 2)", by: "Backend", tone: "neutral" });
    expect(historyLine({ kind: "run_settled_woken", detail: { member: "Backend", task: 3, delivered: false } }, tasks))
      .toEqual({ text: "Its run settled, but waking Backend was refused (#3)", by: "Backend", tone: "warn" });
  });

  it("an event without its own line shows its kind, as Qt does", () => {
    expect(historyLine({ kind: "something_new", detail: { by: "Lead" } })).toEqual({ text: "something_new", by: "Lead", tone: "neutral" });
  });
});
