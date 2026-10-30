import { describe, expect, it } from "vitest";
import { deliveryError, deliveryText, historyAction } from "./task-history-ui";
import type { TaskEntry } from "../../store/taskDetails";
const entry = (state: string, mode = "send"): TaskEntry => ({ kind: "message", text: "Context", at: 1, actor: "You", session: "", requestId: "request", files: [], delivery: { state, mode, error: "" } });
describe("Qt task journal labels", () => {
  it("distinguishes saved notes, queued context, submitted feedback and unknown delivery", () => {
    expect(deliveryText(entry("note"), "")).toBe("Saved note — not sent");
    expect(deliveryText(entry("queued"), "")).toBe("Waiting for Start");
    expect(deliveryText(entry("submitted", "queue"), "")).toBe("Included in Start request");
    expect(deliveryText(entry("submitted"), "child")).toContain("child receipt unavailable");
    expect(deliveryText(entry("failed"), "")).toBe("Saved in task — not sent");
    expect(deliveryText(entry("uncertain"), "")).toBe("Saved in task — delivery unconfirmed");
  });
  it("names history actions and explains an unverifiable executor", () => {
    const e = entry("failed"); e.kind = "stage"; e.text = "todo";
    expect(historyAction(e)).toBe("moved it to To do");
    e.kind = "started"; expect(historyAction(e)).toBe("started the task");
    e.delivery.error = "execution changed"; expect(deliveryError(e)).toContain("previous executor could not be verified");
    e.delivery.error = "offline"; expect(deliveryError(e)).toBe("offline");
  });
});
