// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("../../../bridge", () => ({ inTauri: () => true }));
import { desktopService } from "../../../store/desktopService";
import { ApprovalsSection } from "./Approvals";
import { trustSentence, type Entry } from "./approvals-journal";
import pageFixture from "./approvals-page.r6.fixture.json";

// zerus-63rru.66 round 8: every row's own Scope cell is bound to that row's
// own outcome. Counting sentences over the whole page could not tell a grant
// shown on a refused row from one shown on the approved row; these
// assertions can. The expected sentences are written out here, never taken
// from the production selector.
const GRANT = "Trusted for /fixture/main and its subfolders on this account";
const REQUESTED = "Trust was requested for /fixture/main and its subfolders; it was not confirmed";

function expectedScope(entry: { result: string; scope?: string | null; standing?: string | null }): string {
  if (!entry.scope || entry.standing !== "team_folder_trust") return "";
  return entry.result === "Approved once" ? GRANT : REQUESTED;
}

describe("trustSentence", () => {
  const base: Entry = { id: 1, at: 0, host: "h", session: "s", prompt: "p", result: "Approved once", scope: "/fixture/main", standing: "team_folder_trust" };
  it.each([
    ["Approved once", GRANT],
    ["Failed", REQUESTED],
    ["Cancelled", REQUESTED],
    ["Unconfirmed", REQUESTED],
    ["Unknown", REQUESTED],
  ])("a folder-trust record whose outcome is %s says its own sentence", (result, sentence) => {
    expect(trustSentence({ ...base, result })).toBe(sentence);
  });
  it("records that are not folder trust, or have no scope, say nothing", () => {
    expect(trustSentence({ ...base, standing: null })).toBeNull();
    expect(trustSentence({ ...base, standing: "other" })).toBeNull();
    expect(trustSentence({ ...base, scope: null })).toBeNull();
    expect(trustSentence({ ...base, scope: "   " })).toBeNull();
  });
});

let dispose: (() => void) | undefined;
beforeEach(() => { try { localStorage.clear(); } catch { /* none */ } });
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

it("each journal row's Scope cell matches that row's own Result", async () => {
  native.invoke.mockImplementation(async (command: string) => {
    if (command === "approvals_detail") return "";
    return { ...pageFixture, next: 0, previous: 0 };
  });
  desktopService.apply({ connected: true, epoch: "fixture", generation: 1, background: {},
    attention_counts: null, automatic_approvals: true, details_available: true, code: "" } as never);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(ApprovalsSection, {}), host);

  const entries = pageFixture.entries as Array<{ id: number; result: string; scope?: string | null; standing?: string | null }>;
  const table = () => host.querySelector<HTMLTableElement>(".an-table table");
  const bodyRows = () => Array.from(table()?.querySelectorAll<HTMLTableRowElement>("tbody > tr") ?? []);
  const cells = (row: Element) => Array.from(row.querySelectorAll(":scope > td, :scope > th"));
  await vi.waitFor(() => expect(bodyRows().length).toBe(entries.length));

  const headers = Array.from(table()!.querySelectorAll("thead th")).map((cell) => cell.textContent?.trim());
  const scopeColumn = headers.indexOf("Scope");
  const resultColumn = headers.indexOf("Result");
  expect(scopeColumn).toBeGreaterThanOrEqual(0);
  expect(resultColumn).toBeGreaterThanOrEqual(0);

  const rows = bodyRows();
  const seen = new Set<string>();
  rows.forEach((row, index) => {
    const entry = entries[index]!;
    expect(cells(row)[resultColumn]!.textContent).toBe(entry.result);
    expect(cells(row)[scopeColumn]!.textContent, `row ${entry.id} (${entry.result})`).toBe(expectedScope(entry));
    seen.add(`${entry.result}/${entry.standing ?? "none"}`);
  });
  // The fixture keeps every outcome this binding has to tell apart.
  for (const outcome of ["Approved once/team_folder_trust", "Failed/team_folder_trust", "Cancelled/team_folder_trust", "Unconfirmed/team_folder_trust", "Approved once/none"]) {
    expect(seen.has(outcome), outcome).toBe(true);
  }
});
