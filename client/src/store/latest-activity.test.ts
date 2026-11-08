import { describe, expect, it, vi } from "vitest";
import { createEffect, createRoot } from "solid-js";

// store/ui.ts reads the page's query; the tests run without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

import { detailTab, setDetailTab, showLatestActivity, takeLatestRequest } from "./ui";

describe("Enter on a session row (Qt's jumpToLatestActivity, SessionsWindow.cpp:5499-5504)", () => {
  it("shows the Activity tab and asks its view to jump once per request", () => {
    expect(takeLatestRequest(), "nothing asked yet").toBe(false);
    setDetailTab("terminal");
    showLatestActivity();
    expect(detailTab()).toBe("activity");
    expect(takeLatestRequest()).toBe(true);
    expect(takeLatestRequest(), "the request is taken once").toBe(false);
  });

  it("reaches the mounted view, and a view that mounts after the request", async () => {
    const jumps: string[] = [];
    const disposeFirst = createRoot((dispose) => { createEffect(() => { if (takeLatestRequest()) jumps.push("mounted"); }); return dispose; });
    showLatestActivity();
    await Promise.resolve();
    expect(jumps).toEqual(["mounted"]);
    disposeFirst();
    showLatestActivity();
    const disposeLater = createRoot((dispose) => { createEffect(() => { if (takeLatestRequest()) jumps.push("later"); }); return dispose; });
    await Promise.resolve();
    expect(jumps).toEqual(["mounted", "later"]);
    disposeLater();
  });
});
