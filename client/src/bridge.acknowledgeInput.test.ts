// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke, isTauri: () => false, Channel: class {} }));
import { acknowledgeInput } from "./bridge";
const target = { computer: "", name: "claude/example/review", run_id: "run", conversation_id: "conversation" };
const binding = "a".repeat(64);
const pending = { original_request_id: "11111111-1111-4111-8111-111111111111", pending_at: 100 };
const receipt = () => ({ name: target.name, run_id: target.run_id, conversation_id: target.conversation_id,
  original_request_id: pending.original_request_id, status: "person-acknowledged-unknown", accepted: true, durable: true, pending: false,
  acknowledgment: { name: target.name, expected_run_id: target.run_id, expected_conversation_id: target.conversation_id,
    ...pending, terminal_binding_id: binding } });
beforeEach(() => ipc.invoke.mockReset());

it("accepts only the reviewed input's completed receipt, including after a previous intent's lookup", async () => {
  ipc.invoke.mockResolvedValueOnce(receipt());
  expect((await acknowledgeInput(target, binding, pending)).status).toBe("person-acknowledged-unknown");
  for (const changes of [{ name: "other" }, { run_id: "other" }, { conversation_id: "other" },
    { original_request_id: "other" }, { durable: false }]) {
    ipc.invoke.mockResolvedValueOnce({ ...receipt(), ...changes });
    expect((await acknowledgeInput(target, binding, pending)).status).toBe("unknown");
  }
  for (const changes of [{ pending_at: 101 }, { terminal_binding_id: "b".repeat(64) }]) {
    const old = receipt(); old.acknowledgment = { ...old.acknowledgment, ...changes };
    ipc.invoke.mockResolvedValueOnce(old);
    expect((await acknowledgeInput(target, binding, pending)).status).toBe("unknown");
  }
});
