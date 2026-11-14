import { expect, it } from "vitest";
import { formatActionBusy } from "./action-busy";
import { translateWithCatalog } from "./i18n";
import russian from "./i18n.ru.json";

const encoded = "zerus-action-busy-v1:" + JSON.stringify({
  action: "Clear session", elapsed_seconds: 181, started_utc: "2026-11-13 12:00:00",
  message: {
    source: "This session is busy with “%1” (%2 elapsed, since %3 UTC). Wait for it to finish.",
    args: [
      { kind: "text", value: { source: "Clear session" } },
      { kind: "text", value: { source: "%1 min", args: [{ kind: "number", value: 3 }] } },
      { kind: "verbatim", value: "2026-11-13 12:00:00" },
    ],
  },
});
it("names the native action, its elapsed age and its exact start time", () => {
  expect(formatActionBusy(encoded, (source, ...args) => translateWithCatalog({}, "en", source, ...args)))
    .toBe("This session is busy with “Clear session” (3 min elapsed, since 2026-11-13 12:00:00 UTC). Wait for it to finish.");
});
it("translates the sentence, action and elapsed units while preserving the timestamp", () => {
  expect(formatActionBusy(encoded, (source, ...args) => translateWithCatalog(russian, "ru", source, ...args)))
    .toBe("Сессию занимает действие «Очистить сессию» (прошло 3 мин; с 2026-11-13 12:00:00 UTC). Дождитесь его завершения.");
});
it("leaves ordinary errors and malformed or unbounded envelopes untouched", () => {
  for (const error of ["busy", "The session changed", "zerus-action-busy-v1:{", "zerus-action-busy-v1:" + "x".repeat(4096), "zerus-action-busy-v1:null", "zerus-action-busy-v1:{}"])
    expect(formatActionBusy(error, (source) => source)).toBeUndefined();
});
