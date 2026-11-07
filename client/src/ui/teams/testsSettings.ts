import { tr } from "../../i18n";
import type { TeamCall, TeamOutcome } from "../../types";

// Task 88 (owner, 2026-11-06: builds and tests of any stack): the Tests
// section of the team's settings, as Qt's TeamSettingsDialog shows it. The
// project's configuration comes from `team test-config`; the owner's override
// is edited as JSON shaped like .zerus/tests.toml, checked by hgs without
// writing, and saved only with the dialog's Save (`settings.tests`, which hgs
// checks again).

/** What `team test-config` answers. */
export interface TestConfig {
  ok: boolean;
  error?: string;
  source?: "file" | "detected" | "none";
  overridden?: boolean;
  providers?: { name: string; kind: string; disabled?: boolean; results?: { format: string; path: string } }[];
  templates?: { id: string; program: string; accepted: boolean; hash: string }[];
  proposal?: string;
}

/** The override the field holds: null for an empty field (or `{}`), or why it is not a JSON object. */
export function parseOverride(text: string): { value: Record<string, unknown> | null; error: string } {
  const trimmed = text.trim();
  if (!trimmed) return { value: null, error: "" };
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch (cause) {
    return { value: null, error: tr("The tests override is not valid JSON: %1", cause instanceof Error ? cause.message : String(cause)) };
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { value: null, error: tr("The tests override is a JSON object shaped like .zerus/tests.toml") };
  }
  const value = parsed as Record<string, unknown>;
  return { value: Object.keys(value).length === 0 ? null : value, error: "" };
}

/** The field's first text: the stored override, indented, or empty. */
export function overrideText(stored: unknown): string {
  return stored && typeof stored === "object" && !Array.isArray(stored) && Object.keys(stored).length > 0 ? JSON.stringify(stored, null, 2) : "";
}

const same = (a: unknown, b: unknown): boolean => JSON.stringify(sorted(a)) === JSON.stringify(sorted(b));
function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value as Record<string, unknown>).sort().map((key) => [key, sorted((value as Record<string, unknown>)[key])]));
  }
  return value;
}

/** The settings patch's `tests`, as Qt's changes() builds it: absent when unchanged or invalid, null to drop the override. */
export function testsPatch(stored: unknown, text: string): { tests?: Record<string, unknown> | null } {
  const { value, error } = parseOverride(text);
  if (error) return {};
  const none = !stored || typeof stored !== "object" || Object.keys(stored as object).length === 0;
  if (value === null) return none ? {} : { tests: null };
  return !none && same(value, stored) ? {} : { tests: value };
}

/** What the section lists, one line each (Qt's testsLines). */
export function testsLines(config: TestConfig): string[] {
  const lines: string[] = [];
  for (const provider of config.providers ?? []) {
    let line = tr("Provider %1 (%2)", provider.name, provider.kind);
    line += provider.results ? tr(", results %1 from %2", provider.results.format, provider.results.path) : tr(", no per-test report");
    if (provider.disabled) line += tr(", off");
    lines.push(line);
  }
  for (const template of config.templates ?? []) {
    lines.push(template.accepted
      ? tr("Template %1: %2, accepted", template.id, template.program)
      : tr("Template %1: %2, waits for your acceptance (hgs team templates accept %3)", template.id, template.program, template.hash.slice(0, 12)));
  }
  if (lines.length === 0) lines.push(tr("No test providers yet"));
  return lines;
}

/** Where the configuration comes from, as the section's first line says. */
export function sourceLine(config: TestConfig): string {
  const base = config.source === "file" ? tr("From the project's .zerus/tests.toml")
    : config.source === "detected" ? tr("Detected from the project: no .zerus/tests.toml yet")
      : tr("No test configuration: the project has no .zerus/tests.toml and no known stack");
  return config.overridden ? tr("%1, with your override", base) : base;
}

/** Sends a test-config call; the receipt is the answer. */
export type Send = (call: TeamCall) => Promise<TeamOutcome>;

/** Reads the project's configuration with the stored override. */
export async function readTestConfig(send: Send, team: string, teamName: string): Promise<TestConfig> {
  const outcome = await send({ team, team_name: teamName, action: { kind: "test_config", action: "get" } });
  if (outcome.outcome !== "done") return { ok: false, error: outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail };
  return (outcome.receipt ?? { ok: false }) as unknown as TestConfig;
}

/** Checks the field's override with hgs; nothing is written. */
export async function checkTestConfig(send: Send, team: string, teamName: string, text: string): Promise<{ config?: TestConfig; message: string; good: boolean }> {
  const { value, error } = parseOverride(text);
  if (error) return { message: error, good: false };
  const outcome = await send({ team, team_name: teamName, action: { kind: "test_config", action: "check", tests: value } });
  if (outcome.outcome !== "done") return { message: tr("hgs refuses it: %1", outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail), good: false };
  const config = (outcome.receipt ?? { ok: false }) as unknown as TestConfig;
  if (!config.ok) return { config, message: tr("hgs refuses it: %1", config.error ?? ""), good: false };
  return { config, message: tr("hgs loads it: %1 providers, %2 templates. Save applies it.", config.providers?.length ?? 0, config.templates?.length ?? 0), good: true };
}
