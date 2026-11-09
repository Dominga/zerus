// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";

// store/ui.ts reads the page's query; the tests run without a page.
vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });

import { ActivityPending } from "./ActivityPending";
import { detailTab, setDetailTab } from "../../store/ui";
import type { ActivitySource } from "../../store/activitySource";
import type { Session } from "./types";

// What the Activity shows before its first answer (zerus-bz54l).

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

function mount(session: Partial<Session>, source: Partial<ActivitySource> = {}) {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(ActivityPending, { source: source as ActivitySource, session: session as Session }), host);
  return host;
}
const button = (host: HTMLElement, label: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === label);

it("a shell session says Activity is not recorded and opens Terminal, instead of reading", () => {
  setDetailTab("activity");
  const host = mount({ agent: "sh", tracked: false });
  expect(host.textContent).toContain("Activity is not recorded for shell sessions.");
  expect(host.textContent).not.toContain("Reading activity…");
  button(host, "Open Terminal")!.click();
  expect(detailTab()).toBe("terminal");
});

it("an agent session reads, and a read that did not answer in time offers Retry", () => {
  const [stalled, setStalled] = createSignal(false);
  const retry = vi.fn();
  const host = mount({ agent: "claude", tracked: true }, { stalled, retry });
  expect(host.textContent).toContain("Reading activity…");
  setStalled(true);
  expect(host.textContent).toContain("Activity did not load");
  button(host, "Retry")!.click();
  expect(retry).toHaveBeenCalledTimes(1);
});

it("a recorded (read-only) source of a shell row keeps reading its record", () => {
  const host = mount({ agent: "sh", tracked: false }, { readOnly: true });
  expect(host.textContent).toContain("Reading activity…");
});
