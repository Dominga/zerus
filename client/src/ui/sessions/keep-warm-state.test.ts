import { describe, expect, it, vi } from "vitest";
vi.mock("../../store/inspector", () => ({ openInspector: vi.fn(), inspectorClosed: () => new Set<string>(), toggleInspectorCard: vi.fn() }));
import { confirmKeepWarm, contextTokensOf, currentKeepWarm, keepWarmTarget } from "./keep-warm-state";
import type { KeepWarmState } from "../../types";
import type { Session } from "./types";

const warm = (extra: Partial<KeepWarmState> = {}): KeepWarmState => ({ enabled: true, until: null, revision: 3, state: "waiting", next_at: null, reason: "", turn_active: false, ...extra });
const session = (key: string, keepWarm?: KeepWarmState) => ({ key, computer: "", name: `codex/p/${key}`, runId: `run-${key}`, conversationId: `conv-${key}`, keepWarm }) as unknown as Session;

describe("one keep-warm state for the header and Details (zerus-yacns)", () => {
  it("uses a change confirmed in Details for that session only, until a newer revision arrives", () => {
    const alpha = session("alpha", warm({ enabled: false, state: "off", revision: 3 }));
    const beta = session("beta", warm({ enabled: false, state: "off", revision: 3 }));
    confirmKeepWarm(keepWarmTarget(alpha), warm({ revision: 4 }));
    expect(currentKeepWarm(alpha, undefined)).toMatchObject({ enabled: true, revision: 4 });
    expect(currentKeepWarm(beta, undefined)).toMatchObject({ enabled: false, revision: 3 });
    // The listing catches up and then moves on: the newer revision wins.
    expect(currentKeepWarm(session("alpha", warm({ enabled: false, state: "off", revision: 5 })), undefined)).toMatchObject({ enabled: false, revision: 5 });
    // Another run of the same row is another target.
    expect(currentKeepWarm({ ...alpha, runId: "run-other" } as Session, undefined)).toMatchObject({ enabled: false, revision: 3 });
  });

  it("reads the Details summary's keep-warm state and context size, and nothing else", () => {
    const s = session("gamma");
    expect(currentKeepWarm(s, { keep_warm: { ...warm({ revision: 7 }), supported: true } })).toMatchObject({ enabled: true, revision: 7, supported: true });
    expect(currentKeepWarm(s, undefined)).toBeUndefined();
    expect(contextTokensOf({ session_usage: { context: { used: 48_000 } } })).toBe(48_000);
    expect(contextTokensOf(undefined)).toBe(0);
  });
});
