import { describe, expect, it } from "vitest";

// Task 245: the test setup (src/test-locale.ts) reads numbers and dates in
// en-US on every machine, and a call that names its locale keeps it.
describe("the tests' locale", () => {
  it("formats in en-US unless a locale is named", () => {
    expect((8193).toLocaleString()).toBe("8,193");
    expect(new Intl.NumberFormat(undefined, { minimumFractionDigits: 1 }).format(7)).toBe("7.0");
    expect(new Intl.NumberFormat().resolvedOptions().locale).toBe("en-US");
    expect((8193).toLocaleString("de-DE")).toBe("8.193");
    expect(new Date(Date.UTC(2026, 10, 8)).toLocaleDateString(undefined, { timeZone: "UTC" })).toBe("11/8/2026");
    expect(new Intl.NumberFormat() instanceof Intl.NumberFormat).toBe(true);
  });
});
