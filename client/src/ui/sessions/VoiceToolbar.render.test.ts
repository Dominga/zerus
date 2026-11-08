// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";

vi.mock("../../bridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../bridge")>()), voiceVideo: vi.fn(async () => ({ available: true, reason: null })),
}));

import { VoiceToolbar } from "./VoiceControls";
import type { SessionVoice, VoiceShown } from "../../store/voice";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

const recording = (icon: "send" | "stop"): Partial<VoiceShown> => ({
  key: "draft", id: "rec-1", state: "recording", recording: true, stop_preview: true, voice_enabled: true,
  voice_icon: icon, voice_label: icon === "send" ? "Send recording" : "Stop", voice_detail: "", mode_enabled: false,
  cancel_visible: true, notice_visible: false, notice_label: "", notice_detail: "",
});

function mount(initial: Partial<VoiceShown>) {
  const [shown, setShown] = createSignal<VoiceShown | undefined>(initial as VoiceShown);
  const input = vi.fn(async () => {});
  const voice = { shown, input, error: () => undefined } as unknown as SessionVoice;
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(VoiceToolbar, { voice, compact: false }), host);
  const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.includes(label));
  return { input, setShown, button };
}

it("Stop and preview stops a send-immediately recording and keeps it for review (Qt SpeechComposer.cpp:90-92)", () => {
  const { input, button } = mount(recording("send"));
  const stop = button("Stop and preview");
  expect(stop).toBeDefined();
  stop!.click();
  expect(input).toHaveBeenCalledExactlyOnceWith({ kind: "stop", forget_send: false });
});

it("is not repeated where the record button itself stops to preview, and goes when recording ends", () => {
  const { button, setShown } = mount(recording("stop"));
  expect(button("Stop and preview")).toBeUndefined();
  setShown({ ...recording("send"), state: "preview", recording: false, stop_preview: false } as VoiceShown);
  expect(button("Stop and preview")).toBeUndefined();
});
