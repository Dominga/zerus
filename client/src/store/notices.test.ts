import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { MAX_NOTICES, NOTICE_MS, bus, createNoticeBus, toast } from "./notices";

describe("the window's notice bus", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("shows a notice for the design's toast time; a sticky one stays until dismissed", () => {
    createRoot((dispose) => {
      const b = createNoticeBus();
      b.notify("info", "Session ID copied");
      const error = b.notify("error", "Stop failed. Open Terminal to check the agent.", true);
      vi.advanceTimersByTime(NOTICE_MS - 1);
      expect(b.notices().map((n) => n.text)).toEqual(["Session ID copied", "Stop failed. Open Terminal to check the agent."]);
      vi.advanceTimersByTime(1);
      expect(b.notices().map((n) => n.tone)).toEqual(["error"]);
      vi.advanceTimersByTime(60_000);
      expect(b.notices()).toHaveLength(1);
      b.dismiss(error);
      expect(b.notices()).toEqual([]);
      dispose();
    });
  });

  it("replaces a notice with the same key and caps the stack, oldest passing notice first", () => {
    createRoot((dispose) => {
      const b = createNoticeBus();
      b.notify("info", "Marking 1 reply read…", false, "marks");
      b.notify("info", "1 reply marked as read.", false, "marks");
      expect(b.notices().map((n) => n.text)).toEqual(["1 reply marked as read."]);
      b.clearNotice("marks");
      expect(b.notices()).toEqual([]);
      b.notify("error", "kept", true);
      for (let i = 0; i < MAX_NOTICES; i++) b.notify("info", `passing ${i}`);
      expect(b.notices()).toHaveLength(MAX_NOTICES);
      expect(b.notices()[0].text).toBe("kept");
      expect(b.notices().at(-1)!.text).toBe(`passing ${MAX_NOTICES - 1}`);
      dispose();
    });
  });

  it("a toast is a titled notice on the window's bus", () => {
    createRoot((dispose) => {
      for (const n of bus.notices()) bus.dismiss(n.id);
      toast("Copied", "The reply is on the clipboard");
      toast("Updates", "The catalog could not be read", "error", true);
      expect(bus.notices()).toMatchObject([
        { tone: "info", title: "Copied", text: "The reply is on the clipboard", sticky: false },
        { tone: "error", title: "Updates", text: "The catalog could not be read", sticky: true },
      ]);
      vi.advanceTimersByTime(NOTICE_MS);
      expect(bus.notices().map((n) => n.title)).toEqual(["Updates"]);
      for (const n of bus.notices()) bus.dismiss(n.id);
      dispose();
    });
  });
});
