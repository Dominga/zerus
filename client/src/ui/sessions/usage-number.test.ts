import { afterEach, expect, it } from "vitest";
import { numberText, setSystemLocale } from "./usage-number";

afterEach(() => setSystemLocale(""));

it("groups numbers with the owner's regional format, as Qt (task 323 N5)", () => {
  setSystemLocale("ru-RU");
  expect(numberText(3695480).replace(/\s/g, " ")).toBe("3 695 480");
  expect(numberText(1234567.891, { minimumFractionDigits: 2, maximumFractionDigits: 2 }).replace(/\s/g, " ")).toBe("1 234 567,89");
});

it("falls back to en-US when the core reports no locale", () => {
  expect(numberText(1000)).toBe("1,000");
});