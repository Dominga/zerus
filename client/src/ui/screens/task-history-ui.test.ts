import { describe, expect, it } from "vitest";
import { changeRequest, deliveryError, deliveryText, historyAction } from "./task-history-ui";
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
  it("shows a reopen's note as a change request sent only when its attempt was", () => {
    const reopen = (phase: string, origin = "reopen"): TaskEntry => ({ ...entry("note", "note"), attempt: { origin, phase } });
    for (const phase of ["submitted", "running", "paused", "ready"]) expect(deliveryText(reopen(phase), "")).toBe("Change request — submitted to executor");
    expect(deliveryText(reopen("not_started"), "")).toBe("Change request — not sent");
    expect(deliveryText(reopen("cancelled"), "")).toBe("Change request — cancelled");
    for (const phase of ["unknown", "unconfirmed", "", "Submitted"]) expect(deliveryText(reopen(phase), "")).toBe("Change request — delivery unconfirmed, never resent automatically");
    expect(changeRequest(reopen("submitted"))).toBe(true);
    // A note without an attempt, or another origin, keeps its own wording.
    expect(changeRequest(entry("note"))).toBe(false);
    expect(deliveryText(reopen("submitted", "send"), "")).toBe("Saved note — not sent");
    expect(changeRequest({ ...reopen("submitted"), kind: "comment" })).toBe(false);
  });
  it("names history actions and explains an unverifiable executor", () => {
    const e = entry("failed"); e.kind = "stage"; e.text = "todo";
    expect(historyAction(e)).toBe("moved it to To do");
    e.kind = "started"; expect(historyAction(e)).toBe("started the task");
    e.delivery.error = "execution changed"; expect(deliveryError(e)).toContain("previous executor could not be verified");
    e.delivery.error = "offline"; expect(deliveryError(e)).toBe("offline");
  });
});
