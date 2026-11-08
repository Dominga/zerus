import { describe, expect, it } from "vitest";
import { lagText, missingSuffix, resumeBehindText, transcriptBehind } from "./resume-behind";

describe("the transcript-behind refusal", () => {
  it("reads the JSON times and the count hgs appends", () => {
    const detail = 'review: the native transcript stops 14400 s before the events Zerus recorded (transcript_behind); resume with --accept-stale or --with-lost-dialogue {"error":"transcript_behind","transcript_at":1700000000,"events_at":1700014400,"missing":3}';
    expect(transcriptBehind(detail)).toEqual({ transcriptAt: 1700000000, eventsAt: 1700014400, missing: 3, lag: 14400 });
  });

  it("keeps only the lag from an older refusal and no gap otherwise", () => {
    expect(transcriptBehind("stops 120 s before the events Zerus recorded (transcript_behind)")).toEqual({ transcriptAt: 0, eventsAt: 0, missing: 0, lag: 120 });
    expect(transcriptBehind("an unrelated failure")).toBeUndefined();
  });

  it("names both times with the missing count, as Qt does", () => {
    const text = resumeBehindText({ transcriptAt: 1700000000, eventsAt: 1700014400, missing: 3, lag: 14400 }, () => "1 Jan 10:00");
    expect(text).toContain("Claude's transcript stops at 1 Jan 10:00");
    expect(text).toContain("(3 entries missing)");
    expect(text).toContain("leaves that recorded dialogue out");
  });

  it("falls back to the lag, then to the gap alone", () => {
    expect(resumeBehindText({ transcriptAt: 0, eventsAt: 0, missing: 0, lag: 7200 }, () => "x")).toContain("stops 2 h before");
    expect(resumeBehindText({ transcriptAt: 0, eventsAt: 0, missing: 0, lag: 90 }, () => "x")).toContain("stops 1 min before");
    expect(resumeBehindText({ transcriptAt: 0, eventsAt: 0, missing: 0, lag: 0 }, () => "x")).toContain("is behind the events Zerus recorded.");
  });

  it("words the missing count and the lag as Qt does", () => {
    expect(missingSuffix(0)).toBe("");
    expect(missingSuffix(1)).toBe(" (1 entries missing)");
    expect(lagText(3599)).toBe("59 min");
    expect(lagText(3600)).toBe("1 h");
  });
});
