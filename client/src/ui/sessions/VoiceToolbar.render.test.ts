// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";

vi.mock("../../bridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../bridge")>()), voiceVideo: vi.fn(async () => ({ available: true, reason: null })),
  voicePermission: vi.fn(async () => ({ text: "Allow Zerus to use the microphone in System Settings.", can_open: false })),
}));

import { VoicePreview, VoiceToolbar } from "./VoiceControls";
import { createVoice, type SessionVoice, type VoiceOut, type VoiceShown } from "../../store/voice";

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
  const openNotice = vi.fn();
  const voice = { shown, input, openNotice, error: () => undefined } as unknown as SessionVoice;
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(VoiceToolbar, { voice, compact: false }), host);
  const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.includes(label));
  return { input, openNotice, setShown, button };
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

const idle = (notice: Partial<VoiceShown>): Partial<VoiceShown> => ({
  ...recording("stop"), state: "idle", recording: false, stop_preview: false, cancel_visible: false,
  notice_visible: true, notice_label: "Voice notice", notice_detail: "The voice recording is invalid.", ...notice,
});

it("the notice chip is a button that opens the notice (Qt SpeechComposer::showResult)", () => {
  const { openNotice, button } = mount(idle({}));
  const chip = button("Voice notice");
  expect(chip?.getAttribute("aria-haspopup")).toBe("dialog");
  expect(chip?.title).toBe("The voice recording is invalid.");
  chip!.click();
  expect(openNotice).toHaveBeenCalledTimes(1);
});

const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
async function previewWith(notice: Partial<VoiceShown>) {
  let out!: (event: VoiceOut) => void;
  const voice = createVoice({
    open: vi.fn(async (_owner, receive) => { out = receive; }), input: vi.fn(async () => {}), close: vi.fn(async () => {}),
    remember: vi.fn(), notice: vi.fn(),
  }, "private-owner");
  voice.follow({ handle: 1, key: "draft", visible: true }); await tick();
  out({ kind: "shown", shown: { ...idle(notice), preview: null, video_preview: false } as VoiceShown });
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(VoicePreview, { voice }), host);
  const dialog = host.querySelector("dialog")!;
  const button = (label: string) => [...dialog.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label);
  return { voice, dialog, button };
}

it("opens the full notice in a Voice message dialog with Close", async () => {
  const { voice, dialog, button } = await previewWith({});
  expect(dialog.open).toBe(false);
  voice.openNotice(); await tick();
  expect(dialog.open).toBe(true);
  expect(dialog.querySelector("h2")?.textContent).toBe("Voice message");
  expect(dialog.querySelector(".confirm-body")?.textContent).toBe("The voice recording is invalid.");
  expect([...dialog.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Close"]);
  button("Close")!.click(); await tick();
  expect(voice.notice()).toBeUndefined();
  expect(dialog.open).toBe(false);
  await voice.close();
});

it("opens the microphone recovery for a permission notice", async () => {
  const { voice, dialog } = await previewWith({ notice_permission: true, notice_label: "Microphone access", notice_detail: "Microphone access was denied." });
  voice.openNotice(); await tick();
  expect(dialog.open).toBe(true);
  expect(dialog.querySelector("h2")?.textContent).toBe("Microphone access");
  expect(dialog.querySelector(".confirm-body")?.textContent).toBe("Allow Zerus to use the microphone in System Settings.");
  expect(voice.notice()).toBeUndefined();
  await voice.close();
});
