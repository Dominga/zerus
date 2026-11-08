import { describe, expect, it } from "vitest";
import { minutes, presentState, stateFacts, stateTooltip, STATE_GLYPH, type StateFacts, type StateKey } from "./session-states";
import type { Session } from "./types";
import contract from "../../../../mobile/android/app/src/test/resources/session-state-contract.json";

// SessionStates::facts and ::present (tray/src/SessionStates.h), over the host's
// session_state, state_reason, last_active, cache_ttl and build.

const NOW = 1_000_000;
const session = (extra: Partial<Session> = {}): Session => ({ state: "running", status: "ready", processState: "running", ...extra }) as Session;
const shown = (extra: Partial<Session>, online = true, unread = false) => presentState(stateFacts(session(extra), online, unread), NOW);

describe("session state facts", () => {
  it("keeps an older host's card as it is: no state, nothing shown", () => {
    const none = shown({});
    expect(none.key).toBe("");
    expect(none.badge).toBe("");
    expect(none.line).toBe("");
    expect(none.coldActions).toBe(false);
  });

  it("shows a machine that does not answer as offline whatever the host said", () => {
    const offline = shown({ sessionState: "deps", stateReason: { kind: "subagents", text: "Waiting for 2 subagents" } }, false);
    expect(offline.key).toBe("offline");
    expect(offline.badge).toBe("Offline");
    expect(offline.tooltip).toBe("The machine does not answer");
  });

  it("names what the user state waits for: approval, input, error, the lead's Needs you, or a reply to read", () => {
    const badge = (extra: Partial<Session>, unread = false) => shown({ sessionState: "user", ...extra }, true, unread).badge;
    expect(badge({ phase: "approval" })).toBe("Needs approval");
    expect(badge({ phase: "input" })).toBe("Needs input");
    expect(badge({ phase: "error" })).toBe("Error");
    expect(badge({ stateReason: { kind: "team_lead", text: "Waiting for you" } })).toBe("Needs you");
    expect(badge({}, true)).toBe("New reply");
    expect(shown({ sessionState: "user" }, true, true).line).toBe("Response ready to read");
    expect(badge({})).toBe("Needs you");
    expect(shown({ sessionState: "user", phase: "approval", stateReason: { kind: "approval", text: "Run npm test?" } }).line).toBe("Run npm test?");
  });

  it("says why a session waits for dependencies, always explicitly", () => {
    const deps = shown({ sessionState: "deps", stateReason: { kind: "subagents", text: "Waiting for 2 subagents" } });
    expect([deps.badge, deps.icon, deps.line]).toEqual(["Waiting", "hourglass", "Waiting for 2 subagents"]);
    expect(shown({ sessionState: "deps" }).line).toBe("Waiting for background work");
    expect(deps.tooltip).toContain("Waiting for dependencies");
  });

  it("says what a build runs: the host's reason, else its label and how long it has run", () => {
    const running = { label: "cargo test", command: "cargo test --workspace", since: NOW - 360 };
    const build = shown({ sessionState: "build", build: running });
    expect([build.badge, build.icon, build.line]).toEqual(["Building", "build", "Running cargo test, 6m so far"]);
    expect(shown({ sessionState: "build", build: { label: "", command: "make", since: 0 } }).line).toBe("Running make");
    expect(shown({ sessionState: "build", build: running, stateReason: { kind: "build", text: "Running the suite" } }).line).toBe("Running the suite");
  });

  it("cools down and turns cold by itself as the clock runs", () => {
    const idle = (seconds: number, extra: Partial<Session> = {}) => shown({ sessionState: "cooling", lastActive: NOW - seconds, cacheTtl: { seconds: 300, kept_warm: false }, ...extra });
    const warm = idle(120);
    expect([warm.key, warm.badge, warm.icon]).toEqual(["cooling", "Cooling 3m", "thermo"]);
    expect(warm.heat).toBeCloseTo(0.6, 5);
    expect(warm.line).toBe("Done, waiting for a new task, cache warm for 3m");
    expect(warm.coldActions).toBe(false);
    expect(idle(120, { cacheTtl: { seconds: 300, kept_warm: true } }).line).toContain("(kept warm)");
    const cold = idle(1000);
    expect([cold.key, cold.badge, cold.icon, cold.line, cold.coldActions]).toEqual(["cold", "Cold", "snow", "Cache cold 11m", true]);
    expect(cold.heat).toBe(-1);
    // The last second of warmth still has its sliver of heat.
    expect(idle(299).heat).toBe(0.04);
  });

  it("falls back to the 5 minute cache and to the last event when the host sent no last_active", () => {
    const cold = shown({ sessionState: "cold", updatedAt: NOW - 400 });
    expect(cold.key).toBe("cold");
    expect(cold.line).toBe("Cache cold 1m");
    expect(shown({ sessionState: "cold", replyAt: NOW - 100 }).key).toBe("cooling");
    expect(shown({ sessionState: "cold" }).line).toBe("Cache cold");
  });

  it("separates a stop by the person from an agent that exited", () => {
    const stopped = shown({ sessionState: "stopped" });
    expect([stopped.badge, stopped.icon, stopped.line]).toEqual(["Stopped", "stop", "Stopped by you, resume to continue"]);
    const exited = shown({ sessionState: "stopped", processState: "exited" });
    expect([exited.badge, exited.line]).toEqual(["Agent exited", "The agent exited, resume to continue"]);
  });

  it("counts whole minutes as the design does", () => {
    expect([0, 59, 60, 3599, 3600, 7500].map(minutes)).toEqual(["0m", "0m", "1m", "59m", "1h 00m", "2h 05m"]);
  });

  it("covers every state of the shared contract, with a glyph for the calm ones", () => {
    const keys = contract.states as StateKey[];
    for (const key of keys) {
      const facts: StateFacts = { ...stateFacts(session({ sessionState: key, lastActive: NOW - 10 }), true, false), key };
      const out = presentState(facts, NOW);
      expect(out.key, key).toBe(key);
      expect(stateTooltip(key), key).not.toBe("");
      if (["deps", "build", "cooling", "cold", "stopped"].includes(key)) expect(STATE_GLYPH[out.icon as keyof typeof STATE_GLYPH], key).toBeTruthy();
    }
  });
});
