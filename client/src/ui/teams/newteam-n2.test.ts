import { describe, expect, it } from "vitest";
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const read = (file: string) => fs.readFileSync(new URL(file, import.meta.url), "utf8");

describe("New team N2 (task #326)", () => {
  it("shows the Working mode row and the coordinator's details", () => {
    const form = read("./NewTeam.tsx");
    expect(form).toContain('data-working-mode');
    expect(form).toContain('tr("Working mode")');
    expect(form).toContain('tr("Auto: follow the project")');
    expect(form).toContain("coordinatorText()");
  });
  it("offers + New team on a session with no team", () => {
    const detail = read("../sessions/SessionDetail.tsx");
    expect(detail).toContain('tr("+ New team")');
    expect(detail).toContain('detailTab() === "newteam"');
  });
});
