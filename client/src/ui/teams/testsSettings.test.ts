import { describe, expect, it } from "vitest";
import { checkTestConfig, overrideText, parseOverride, readTestConfig, sourceLine, testsLines, testsPatch, type Send, type TestConfig } from "./testsSettings";
import { caseFailed, caseName, reportCounts } from "./planModes";
import type { TeamCall, TeamOutcome } from "../../types";

// Task 88 (owner, 2026-11-06: builds and tests of any stack): the Tests
// settings read the project's providers and templates through hgs, check the
// owner's override without writing it, and the Tests mode shows each test.

const config: TestConfig = {
  ok: true, source: "file", overridden: false, proposal: "version = 1\n",
  providers: [
    { name: "engine", kind: "ctest", results: { format: "junit", path: "build/ctest.xml" } },
    { name: "python", kind: "pytest", results: { format: "junit", path: "out/pytest.xml" } },
    { name: "game", kind: "command", results: { format: "unreal", path: "Saved/Report/index.json" } },
    { name: "old", kind: "npm", disabled: true },
  ],
  templates: [
    { id: "make-all", program: "make", accepted: true, hash: "a".repeat(64) },
    { id: "uat", program: "RunUAT.bat", accepted: false, hash: "0123456789abcdef".repeat(4) },
  ],
};

function fake(answers: (call: TeamCall) => TeamOutcome) {
  const sent: TeamCall[] = [];
  const send: Send = async (call) => { sent.push(call); return answers(call); };
  return { send, sent };
}

describe("the Tests settings", () => {
  it("list every provider of any stack and each template with its acceptance, as Qt does", () => {
    expect(testsLines(config)).toEqual([
      "Provider engine (ctest), results junit from build/ctest.xml",
      "Provider python (pytest), results junit from out/pytest.xml",
      "Provider game (command), results unreal from Saved/Report/index.json",
      "Provider old (npm), no per-test report, off",
      "Template make-all: make, accepted",
      "Template uat: RunUAT.bat, waits for your acceptance (hgs team templates accept 0123456789ab)",
    ]);
    expect(testsLines({ ok: true })).toEqual(["No test providers yet"]);
    expect(sourceLine(config)).toBe("From the project's .zerus/tests.toml");
    expect(sourceLine({ ok: true, source: "detected", overridden: true })).toBe("Detected from the project: no .zerus/tests.toml yet, with your override");
  });

  it("send only a changed, valid override; an emptied one is dropped", () => {
    const stored = { provider: [{ name: "python", kind: "pytest" }] };
    expect(overrideText(stored)).toBe(JSON.stringify(stored, null, 2));
    expect(overrideText(undefined)).toBe("");
    // Unchanged, in another key order: nothing to send.
    expect(testsPatch(stored, '{"provider":[{"kind":"pytest","name":"python"}]}')).toEqual({});
    expect(testsPatch(stored, '{"provider":[{"name":"python","kind":"unittest"}]}')).toEqual({ tests: { provider: [{ name: "python", kind: "unittest" }] } });
    expect(testsPatch(stored, "")).toEqual({ tests: null });
    expect(testsPatch(stored, "{}")).toEqual({ tests: null });
    expect(testsPatch(undefined, "")).toEqual({});
    expect(testsPatch(undefined, "{nope")).toEqual({});
    expect(parseOverride("{nope").error).toContain("not valid JSON");
    expect(parseOverride("[1]").error).toContain("JSON object");
  });

  it("read and check through hgs, never writing", async () => {
    const { send, sent } = fake((call) => {
      const action = call.action as { kind: string; action: string; tests?: { provider?: { results?: { format: string } }[] } | null };
      if (action.action === "get") return { outcome: "done", receipt: config as unknown as Record<string, unknown>, request_id: "r1" };
      const bad = action.tests?.provider?.some((provider) => provider.results && !["junit", "trx", "unreal"].includes(provider.results.format));
      const receipt = bad ? { ok: false, error: "The provider x's results format is junit, trx or unreal" } : { ...config, overridden: true };
      return { outcome: "done", receipt: receipt as unknown as Record<string, unknown>, request_id: "r2" };
    });
    expect((await readTestConfig(send, "t1", "Engine")).providers?.length).toBe(4);
    const refused = await checkTestConfig(send, "t1", "Engine", '{"provider":[{"name":"x","kind":"pytest","results":{"format":"html","path":"r"}}]}');
    expect(refused).toMatchObject({ good: false });
    expect(refused.message).toContain("hgs refuses it: The provider x's results format");
    const good = await checkTestConfig(send, "t1", "Engine", '{"provider":[{"name":"py","kind":"pytest"}]}');
    expect(good.good).toBe(true);
    expect(good.message).toBe("hgs loads it: 4 providers, 2 templates. Save applies it.");
    // Not JSON: nothing is sent.
    const before = sent.length;
    expect((await checkTestConfig(send, "t1", "Engine", "{nope")).good).toBe(false);
    expect(sent.length).toBe(before);
    expect(sent.map((call) => call.action)).toEqual([
      { kind: "test_config", action: "get" },
      { kind: "test_config", action: "check", tests: { provider: [{ name: "x", kind: "pytest", results: { format: "html", path: "r" } }] } },
      { kind: "test_config", action: "check", tests: { provider: [{ name: "py", kind: "pytest" }] } },
    ]);
  });
});

describe("a run's tests in the Tests mode", () => {
  it("count every test and name each one, failures marked", () => {
    expect(reportCounts({ passed: 41, failed: 1, skipped: 1, errors: 0 })).toBe("41 passed, 1 failed, 1 skipped");
    expect(reportCounts({ passed: 0, failed: 0, skipped: 0, errors: 2 })).toBe("2 errors");
    expect(reportCounts({ passed: 0, failed: 0, skipped: 0, errors: 0 })).toBe("no tests reported");
    expect(reportCounts(null)).toBe("");
    // C++ (CTest names the class as the test), Python, Unreal.
    expect(caseName({ suite: "divides", name: "divides" })).toBe("divides");
    expect(caseName({ suite: "math", name: "divides" })).toBe("math::divides");
    expect(caseName({ suite: "tests.test_ext", name: "test_cython_sum" })).toBe("tests.test_ext::test_cython_sum");
    expect(caseName({ suite: "", name: "Project.World.Spawns" })).toBe("Project.World.Spawns");
    expect([caseFailed({ status: "failed" }), caseFailed({ status: "error" }), caseFailed({ status: "skipped" })]).toEqual([true, true, false]);
  });
});
