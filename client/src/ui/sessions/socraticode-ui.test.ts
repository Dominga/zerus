import { describe, expect, it } from "vitest";
import {
  blankState, connectConfirm, controlsEnabled, detailText, indexedAgo, nextAction, nextVerb, settingsFile, stateParts, stepConfirm,
} from "./socraticode-ui";
import type { SocratiCodePanel } from "../../store/socraticode";

// WhiteWh fork: the SocratiCode card's Qt texts (tray/src/SocratiCodePanel.h
// render 171-246, act 134-151) and its one next step.

const NOW = 1_800_000_000;

const panel = (over: Partial<SocratiCodePanel> = {}): SocratiCodePanel => ({
  state: { kind: "not_indexed", qdrant: true, startable: true },
  details: [], action: null, connect: false, stop: false, poll: false,
  ...over,
});

describe("SocratiCode card texts", () => {
  it("shows the state line as separate parts without dots", () => {
    expect(stateParts({ kind: "checking" }, NOW)).toEqual(["Checking…"]);
    expect(stateParts({ kind: "not_checked" }, NOW)).toEqual(["Not checked"]);
    expect(stateParts({ kind: "unavailable" }, NOW)).toEqual(["Unavailable"]);
    expect(stateParts({
      kind: "indexed", chunks: 1234, files: 56, last_indexed_at: "2026-11-06T09:00:00.000Z",
      watching: true, qdrant: true, startable: true,
    }, Date.parse("2026-11-06T09:05:00.000Z") / 1000)).toEqual(["Indexed", "1234 chunks", "56 files", "5 min ago", "watching"]);
    expect(stateParts({
      kind: "indexed", chunks: 3, files: 0, last_indexed_at: "not a date", watching: false, qdrant: false, startable: false,
    }, NOW)).toEqual(["Indexed", "3 chunks", "not watched", "no Qdrant"]);
    expect(stateParts({ kind: "indexing", indexed: 12, total: 30, qdrant: false, startable: true }, NOW))
      .toEqual(["Indexing…", "12 of 30 files", "Qdrant is not running"]);
    expect(stateParts({ kind: "indexing", indexed: 0, total: 0, qdrant: true, startable: true }, NOW)).toEqual(["Indexing…"]);
    expect(stateParts({ kind: "not_indexed", qdrant: true, startable: true }, NOW)).toEqual(["Not indexed"]);
  });

  it("dates the last index from the clock, only when it parses", () => {
    const at = "2026-11-06T09:00:00.000Z";
    const then = Date.parse(at) / 1000;
    expect(indexedAgo(at, then + 10)).toBe("just now");
    expect(indexedAgo(at, then + 120)).toBe("2 min ago");
    expect(indexedAgo(at, then + 7200)).toBe("2 h ago");
    expect(indexedAgo(at, then + 172_800)).toBe("2 days ago");
    expect(indexedAgo(at, then - 60)).toBe("");
    expect(indexedAgo("", then)).toBe("");
  });

  it("words every detail line", () => {
    expect(detailText({ code: "plugin_missing" })).toBe("The Claude Code plugin is not installed, so agents do not use this index.");
    expect(detailText({ code: "docker_missing" })).toBe("SocratiCode keeps its index in a Qdrant that runs in Docker. Install Docker, or turn on a native Qdrant in Settings > Extensions.");
    expect(detailText({ code: "native_missing", installable: true })).toBe("The native Qdrant is not installed yet.");
    expect(detailText({ code: "native_missing", installable: false })).toBe("Install qdrant on PATH for a native Qdrant.");
    expect(detailText({ code: "agents_docker", url: "http://127.0.0.1:6333" }))
      .toBe("Zerus uses the native Qdrant at http://127.0.0.1:6333; this folder's agents still look for SocratiCode's Docker one.");
    expect(detailText({ code: "node_missing" })).toBe("SocratiCode needs Node.js with npx on this computer.");
    expect(detailText({ code: "ollama_model_missing", model: "nomic-embed-text" }))
      .toBe("Ollama has no nomic-embed-text yet; SocratiCode downloads it with the first index.");
    expect(detailText({ code: "runner_failed", error: "boom" })).toBe("boom");
  });

  it("labels the one next step as Qt does", () => {
    expect(nextAction({ kind: "install" })).toEqual({ text: "Install plugin", tip: "claude plugin install socraticode@socraticode" });
    expect(nextAction({ kind: "install_qdrant", version: "1.19.1" }).text).toBe("Install Qdrant…");
    expect(nextAction({ kind: "start", fresh: true, qdrant: true }).text).toBe("Index and watch");
    expect(nextAction({ kind: "start", fresh: false, qdrant: true }).text).toBe("Watch");
    expect(nextAction({ kind: "start", fresh: false, qdrant: false }).text).toBe("Start");
    expect(nextVerb({ kind: "install" })).toBe("install");
    expect(nextVerb({ kind: "install_qdrant", version: "1" })).toBe("install-qdrant");
    expect(nextVerb({ kind: "start", fresh: false, qdrant: true })).toBe("start");
  });

  it("asks only for the steps Qt asks about", () => {
    expect(stepConfirm({ kind: "install" }, "D:\\repo")).toBeUndefined();
    expect(stepConfirm({ kind: "start", fresh: false, qdrant: true }, "D:\\repo")).toBeUndefined();
    const fresh = stepConfirm({ kind: "start", fresh: true, qdrant: true }, "D:\\repo")!;
    expect(fresh.title).toBe("Index this folder?");
    expect(fresh.text).toContain("D:\\repo");
    const qdrant = stepConfirm({ kind: "install_qdrant", version: "1.19.1" }, "D:\\repo")!;
    expect(qdrant.title).toBe("Install Qdrant?");
    expect(qdrant.text).toContain("Qdrant 1.19.1");
    expect(settingsFile("D:\\repo")).toBe("D:\\repo/.claude/settings.local.json");
    const connect = connectConfirm("D:\\repo");
    expect(connect.title).toBe("Connect this folder's agents?");
    expect(connect.text).toContain("D:\\repo/.claude/settings.local.json");
  });

  it("names what the panel knows before any answer and when controls work", () => {
    expect(blankState(true, "")).toEqual({ kind: "checking" });
    expect(blankState(false, "failed")).toEqual({ kind: "unavailable" });
    expect(blankState(false, "")).toEqual({ kind: "not_checked" });
    expect(controlsEnabled(panel(), false, false)).toBe(true);
    expect(controlsEnabled(panel(), true, false)).toBe(false);
    expect(controlsEnabled(panel(), false, true)).toBe(false);
    expect(controlsEnabled(undefined, false, false)).toBe(false);
  });
});
