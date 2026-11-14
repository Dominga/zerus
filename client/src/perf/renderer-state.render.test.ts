// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import Diagnostics from "./Diagnostics";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });

it("distinguishes the requested renderer from native policy and unavailable observations", async () => {
  const renderer = vi.fn().mockResolvedValue({
    requested: { renderer: "dmabuf", reason: "requested DMA-BUF" },
    observed: { hardware_acceleration_policy: "never", composited_layers: "unavailable", buffer_transport: "unavailable" },
    overrides: [{ name: "WEBKIT_DISABLE_DMABUF_RENDERER", value: "0" }],
  });
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(Diagnostics, {
    displayHz: async () => 144, renderer,
    scrollList: () => null, scrollActivity: () => null, openSession: async () => {},
  }), host);
  for (let i = 0; i < 10; i++) await Promise.resolve();
  expect(host.textContent).toContain("Requested renderer");
  expect(host.textContent).toContain("Observed WebKit state");
  const observed = host.querySelector('[aria-label="Observed WebKit state"]')!;
  expect(observed.textContent).toContain("never");
  expect(observed.textContent).not.toContain("dmabuf");
  expect(observed.textContent?.match(/Unavailable/g)).toHaveLength(2);
  expect(host.textContent).toContain("The public WebKitGTK API does not expose the layer tree or selected buffer transport.");
  expect(host.textContent).toContain("WEBKIT_DISABLE_DMABUF_RENDERER");
  expect(renderer).toHaveBeenCalledTimes(1);
});

it("shows an unavailable diagnostic when the native command cannot be read", async () => {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(Diagnostics, {
    displayHz: async () => 144, renderer: async () => { throw new Error("unavailable"); },
    scrollList: () => null, scrollActivity: () => null, openSession: async () => {},
  }), host);
  for (let i = 0; i < 10; i++) await Promise.resolve();
  expect(host.textContent).toContain("Native renderer diagnostics unavailable");
  expect(host.textContent).not.toContain("Observed WebKit state");
});
