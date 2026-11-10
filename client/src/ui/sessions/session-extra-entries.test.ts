import { afterEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

import { toSession } from "../../store/catalog";
import { parentDialog, setParentDialog } from "../../store/parentLinks";
import { parentEntries } from "./session-extra-entries";
import type { ViewRow } from "../../types";

const row = (agent: string, extra: Partial<ViewRow> = {}) => toSession("", "run:child", {
  name: `${agent}/example/child`, agent, status: "ready", session_id: "33333333-3333-4333-8333-333333333333",
  run_id: "run-child", conversation_id: agent === "dsh" ? "session-child" : "ses_child", ...extra,
});

afterEach(() => setParentDialog(undefined));

describe("parent actions for native API sessions", () => {
  it.each(["dsh", "opencode"])("offers confirmed Set parent and Detach for %s", (agent) => {
    const child = row(agent, { parent: { machine: "11111111-1111-4111-8111-111111111111",
      session_id: "22222222-2222-4222-8222-222222222222", name: "dsh/example/lead",
      conversation_id: "session-lead", run_id: "run-lead", state: "live" } });
    const entries = parentEntries(child);
    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({ label: "Set parent…", disabled: false });
    expect(entries[1]).toMatchObject({ label: "Detach from lead…", disabled: false });
    if ("action" in entries[0]) entries[0].action?.();
    expect(parentDialog()).toEqual({ kind: "set", child });
    if ("action" in entries[1]) entries[1].action?.();
    expect(parentDialog()).toEqual({ kind: "detach", child });
  });

  it("keeps incomplete identities and unsupported agents disabled and archives hidden", () => {
    for (const child of [row("sh"), row("dsh", { run_id: undefined }), row("opencode", { session_id: undefined }),
      row("opencode", { conversation_id: undefined })]) {
      expect(parentEntries(child)[0]).toMatchObject({ disabled: true,
        hint: "Parent links require a supported agent session with a complete identity." });
    }
    expect(parentEntries(row("dsh", { state: "archived" }))).toEqual([]);
  });
});
