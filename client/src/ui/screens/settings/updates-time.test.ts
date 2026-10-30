import { describe, expect, it } from "vitest";
import { lastCheckedText } from "./updates-time";

describe("Updates last successful check", () => {
  it("keeps Never unchanged", () => {
    expect(lastCheckedText("Never")).toBe("Never");
  });

  it("shows the timestamp in the locale's short form at local time", () => {
    const raw = "1970-01-01T00:16:40Z";
    const expected = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" }).format(new Date(raw));
    expect(lastCheckedText(raw)).toBe(expected);
    expect(lastCheckedText(raw)).not.toBe(raw);
    expect(lastCheckedText(raw)).not.toContain("T00:16:40Z");
  });
});
