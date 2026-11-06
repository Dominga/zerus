import { describe, expect, it } from "vitest";
import { agentChoices, ago, canStart, downUse, footText, formOf, hostText, keyMode, pill, projectOf, rootOf, sessionSummary, sshMode, summary, tagFor, urlHint, usedBy } from "./model-servers";
import type { ModelServer } from "../../bridge";
import type { StatusEntry } from "../../store/modelServers";

// Task 67: the Model servers tab's and Settings' words and rules, as Qt's.

const server = (over: Partial<ModelServer> = {}): ModelServer => ({
  name: "lab", base_url: "http://lab.example:8000", kind: "openai", ssh: "", has_key: true, up_minutes: 0, contexts: null,
  status: ["s"], up: [], down: [], logs: [], read_only: false, agent_url: "http://lab.example:8000/v1", models_url: "http://lab.example:8000/v1/models",
  status_text: "s", up_text: "", down_text: "", logs_text: "", ...over,
});
const read = (over: Partial<NonNullable<StatusEntry["status"]>> = {}): StatusEntry => ({ known: true, reading: false, readAt: 0, status: {
  error: "", detail: "", error_label: { source: "Unreachable" }, problem: { source: "" }, source: "command", models: [], free: -1, total: -1,
  memory: { source: "" }, low: false, used: 0, raw: [], running: 0, active: [], ...over } });

describe("model servers tab", () => {
  it("sums servers, models and running without dots", () => {
    const m = { label: { source: "" }, since: 0, usable: true, can_down: true, can_up: false };
    expect(summary([server(), server({ name: "b" })], { lab: read({ models: [{ name: "x", state: "running", ...m }, { name: "y", state: "stopped", ...m }], running: 1 }) }))
      .toBe("2 servers, 2 models, 1 running");
    expect(summary([], {})).toBe("");
  });

  it("names the pill, host and footer as Qt does", () => {
    expect(pill(server(), { known: false, reading: true, readAt: 0 }).text).toBe("Reading");
    expect(pill(server(), { known: false, reading: false, readAt: 0 }).text).toBe("Not read yet");
    expect(pill(server(), read({ error: "unreachable" }))).toEqual({ text: "Unreachable", tone: "danger" });
    expect(pill(server({ read_only: true }), read()).text).toBe("Read only");
    expect(pill(server(), read())).toEqual({ text: "Online", tone: "accent" });
    expect(hostText(server(), ["gpu"])).toBe("This computer");
    expect(hostText(server({ ssh: "gpu" }), ["gpu"])).toBe("gpu in Machines");
    expect(hostText(server({ ssh: "box" }), ["gpu"])).toBe("ssh alias box");
    expect(footText(server({ ssh: "gpu" }))).toContain("over hgs's SSH to gpu");
    expect(footText(server({ read_only: true }))).toMatch(/^No control commands/);
    expect(ago(0, 61_000)).toBe("read 1 min ago");
  });

  it("says whose sessions a Down stops", () => {
    expect(usedBy([])).toBe("none of your sessions");
    expect(usedBy(["a"])).toBe("1 of your sessions uses it: a");
    expect(usedBy(["a", "b", "c", "d", "e"])).toBe("5 of your sessions use it: a, b, c, 2 more");
    expect(downUse("m", "loading", {}, undefined)).toBe("stops loading");
    expect(downUse("m", "running", undefined, undefined)).toBe("reading your sessions…");
    expect(downUse("m", "running", undefined, "timed out")).toBe("your sessions not read: timed out");
  });

  it("New session offers the agent the API speaks and names the session as hgs does", () => {
    expect(agentChoices(server()).map((a) => [a.id, a.ok])).toEqual([["codex", true], ["claude", false]]);
    expect(agentChoices(server({ kind: "anthropic" }))[0].how).toBe("needs an OpenAI-compatible API; lab serves Anthropic");
    expect(projectOf("C:\\work\\my.app\\")).toBe("my_app");
    expect(projectOf("")).toBe("root");
    expect(tagFor("Qwen/Qwen3-Coder 30B")).toBe("qwen3-coder-30b");
    expect(sessionSummary("codex", server(), "small")).toContain("provider  lab  http://lab.example:8000/v1");
    expect(canStart("codex", "small", "running", "/w", "t")).toBe(true);
    expect(canStart("codex", "small", "stopped", "/w", "t")).toBe(false);
    expect(canStart("codex", "small", "listed", " ", "t")).toBe(false);
  });
});

describe("Settings > Model servers", () => {
  it("shows the URLs hgs derives from the root", () => {
    expect(rootOf("http://x:8000/v1/")).toBe("http://x:8000");
    expect(urlHint("http://x:8000/v1")).toBe("Model list: http://x:8000/v1/models\nCodex: http://x:8000/v1\nClaude Code: http://x:8000\nSaved as its root http://x:8000: the /v1 you typed is added where it is needed.");
    expect(urlHint("")).toMatch(/^The address the server answers at/);
  });

  it("never shows a saved key: Replace and Remove only", () => {
    expect(keyMode(true, "")).toMatchObject({ stored: true, field: false, actions: true });
    expect(keyMode(true, "replace")).toMatchObject({ stored: false, field: true, actions: false });
    expect(keyMode(true, "remove")).toMatchObject({ stored: true, field: false, actions: false, text: "removed when you save" });
    expect(keyMode(false, "")).toMatchObject({ field: true, actions: false });
  });

  it("opens a server's form with its SSH host and argv text", () => {
    const form = formOf(server({ ssh: "gpu", kind: "anthropic", up_text: "serve up {model}" }));
    expect(form).toMatchObject({ was: "lab", name: "lab", kind: "anthropic", up: "serve up {model}", key_change: "", key: "" });
    expect(formOf(undefined)).toMatchObject({ was: "", kind: "openai" });
    expect(sshMode("gpu", ["gpu"])).toBe("machine");
    expect(sshMode("box", ["gpu"])).toBe("alias");
    expect(sshMode("", ["gpu"])).toBe("none");
  });
});
