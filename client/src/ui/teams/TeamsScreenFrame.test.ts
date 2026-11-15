// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, onCleanup, onMount } from "solid-js";
import { render } from "solid-js/web";
import fixture from "../screens/fixtures/teams-screen.json";
import { TeamsRow } from "./TeamsScreenFrame";
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
it("resolves and mounts each synthetic row hook once for a product-written team", () => {
  const mounts = vi.fn(), cleanups = vi.fn();
  const Hook = () => { onMount(mounts); onCleanup(cleanups); return document.createElement("span"); };
  const actions = vi.fn(() => createComponent(Hook, {}));
  const details = vi.fn(() => createComponent(Hook, {}));
  const host = document.createElement("ul"); document.body.append(host);
  dispose = render(() => createComponent(TeamsRow, { team: fixture.running.teams[0], onOpen: vi.fn(), children: "Open", get actions() { return actions(); }, get details() { return details(); } }), host);
  expect(actions).toHaveBeenCalledOnce(); expect(details).toHaveBeenCalledOnce(); expect(mounts).toHaveBeenCalledTimes(2);
  expect(host.querySelectorAll(".teams-row-actions span, .teams-row-details span")).toHaveLength(2);
  dispose(); dispose = undefined; expect(cleanups).toHaveBeenCalledTimes(2);
});
