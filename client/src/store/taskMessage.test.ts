import { expect, it } from "vitest";
import { taskItem } from "../ui/screens/tasks-ui";
import { messageChoice, messageLabel, messagePlaceholder, messageDelivery } from "./taskMessage";
const task = (stage: string) => taskItem({ stage, stage_at: 123, dispatch_id: "exact-dispatch", session: { host: "peer", session: "codex/example/worker" }, target: { kind: "any" } });
it.each([
  ["todo", "queue", "Add to task, send on Start", "Add to task, sent on Start", "Add context for the executor…"],
  ["backlog", "queue", "Add to task, send on Start", "Add to task, sent on Start", "Add context for the executor…"],
  ["active", "send", "Send to current executor", "Send to worker @peer", "Message this task’s executor…"],
  ["review", "reopen", "Request changes and send", "Request changes and send to worker @peer", "Message this task’s executor…"],
  ["done", "reopen", "Reopen and send", "Reopen and send to worker @peer", "Message this task’s executor…"],
])("uses the mockup's choices and labels for %s", (stage, mode, choice, label, placeholder) => {
  const t = task(stage);
  expect(messageChoice(t)).toBe(choice); expect(messageLabel(t, false)).toBe(label); expect(messagePlaceholder(t, false)).toBe(placeholder);
  expect(messageDelivery(t, false)).toEqual({ mode, stage_at: 123, dispatch_id: "exact-dispatch" });
  expect(messageLabel(t, true)).toBe("Save note only, no delivery"); expect(messagePlaceholder(t, true)).toBe("Write a note for this task…");
  expect(messageDelivery(t, true).mode).toBe("note");
});
