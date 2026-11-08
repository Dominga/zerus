import { describe, expect, it } from "vitest";
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const read = (file: string) => fs.readFileSync(new URL(file, import.meta.url), "utf8");

describe("Machines phones (task #318 F12)", () => {
  it("shows the Phones panel Qt has, with pair, revoke and refresh", () => {
    expect(read("./Machines.tsx")).toContain("MachinesPhones");
    const mobile = read("./settings/Mobile.tsx");
    expect(mobile).toContain("export function MachinesPhones");
    expect(mobile).toContain("PhonesPanel");
    expect(mobile).toContain('tr("Pair a phone…")');
    expect(mobile).toContain('tr("Revoke…")');
    expect(mobile).toContain('tr("Refresh")');
  });
});
