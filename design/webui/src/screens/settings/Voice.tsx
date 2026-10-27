import { For, Show, createSignal, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import { Icon } from "../../lib/icons";
import { demoState, setDialog, toast } from "../../state";
import { MICROPHONES, SPEECH, VOICE_STATS, type SpeechProvider } from "../../mock/settings-extra";
import { prefs, setPrefs, platform, dialogCtx, setDialogCtx } from "./store";
import { SectionHead, Group, Check, Select, Actions, Status, Lines, Spinner } from "./ui";

// Settings → Voice input (speech/SpeechSettings.cpp). Providers live on a
// processing computer; this device only records and picks the default.

export const [speech, setSpeech] = createStore<Record<string, SpeechProvider[]>>(structuredClone(SPEECH));
export const [selectedProvider, setSelectedProvider] = createSignal<string | undefined>("gigaam-ru");
export const [speechResult, setSpeechResult] = createSignal("");
/** The processing computer of the default provider (speech/host); "" = this computer. */
export const [defaultHost, setDefaultHost] = createSignal("");

export const LOCAL_NAME = platform === "macos" ? "This Mac" : platform === "windows" ? "This Windows computer" : "This computer (Linux)";
export const hostName = (route: string) => (route === "" ? "workstation" : route);

export function VoiceSection() {
  const [loading, setLoading] = createSignal(true);
  const [mic, setMic] = createSignal(demoState() === "error" ? "Selected microphone (unavailable)" : MICROPHONES[0]);
  const load = () => { setLoading(true); setTimeout(() => setLoading(false), 550); };
  onMount(load);
  const host = () => dialogCtx.speechHost;
  const list = () => speech[host()] ?? [];
  const current = () => list().find((p) => p.id === selectedProvider());
  const pending = () => demoState() === "error";
  const ready = () => !loading() && !pending();
  const defaultText = () => {
    if (host() !== defaultHost()) return "The default provider is configured on another computer. Browsing these providers does not change it.";
    const p = list().find((x) => x.id === prefs.speechDefault);
    return p ? `Default for new recordings: ${p.name}` : "The selected default provider is unavailable. Choose another explicitly or restore it on this computer.";
  };
  const status = () => {
    if (loading()) return { kind: "busy" as const, text: "Checking provider configuration…" };
    if (!list().length) return { kind: "muted" as const, text: "No providers yet. Add Local GigaAM or an OpenAI-compatible server." };
    if (list().some((p) => !p.ready)) return { kind: "warn" as const, text: "The recognition service is not ready. You can still add and edit providers." };
    return { kind: "muted" as const, text: "Provider configuration is ready. Test a saved provider before using it." };
  };
  const mics = () => (demoState() === "error" ? [...MICROPHONES, "Selected microphone (unavailable)"] : MICROPHONES);
  return (
    <>
      <SectionHead title="Voice input" />
      <Group>
        <Check label="Preview before sending" checked={prefs.speechPreview} onChange={(v) => setPrefs("speechPreview", v)}
          hint="Show Stop instead of Send while recording audio or video. Stop retains a preview for review and a separate Send. This preference applies on this device." />
      </Group>
      <Group title="Recognition providers" hint="Manage recognition providers on a processing computer. Recording starts on this device; Send voice uses the provider selected beside your recording.">
        <div class="set-row">
          <div class="set-row-text"><label class="set-label" for="voice-host">Configure providers on</label></div>
          <div class="set-row-control">
            <Select id="voice-host" value={host()} onChange={(v) => { setDialogCtx("speechHost", v); setSelectedProvider((speech[v] ?? [])[0]?.id); setSpeechResult(""); load(); }}
              options={[{ value: "", label: LOCAL_NAME }, { value: "macbook", label: "macbook" }, { value: "build-01", label: "build-01" }]} />
          </div>
        </div>
        <div class="voice-list" role="listbox" aria-label="Speech providers">
          <Show when={!loading()} fallback={<div class="set-loading"><Spinner /></div>}>
            <For each={list()} fallback={<div class="voice-empty muted">No providers on {hostName(host())}.</div>}>
              {(p) => (
                <button role="option" class="voice-item" classList={{ on: selectedProvider() === p.id }} aria-selected={selectedProvider() === p.id}
                  onClick={() => setSelectedProvider(p.id)} onDblClick={() => { setDialogCtx("speechEdit", p.id); setDialog("settings-speech-provider"); }}>
                  <Icon name={p.kind === "gigaam" ? "cpu" : "network"} size={15} class="dim" />
                  <span class="col grow" style={{ gap: "1px" }}>
                    <span class="row"><b class="ell">{p.name}</b><Show when={host() === defaultHost() && prefs.speechDefault === p.id}><span class="chip set-accent-chip"><Icon name="star" size={10} />Default</span></Show></span>
                    <span class="muted ell">{p.kind === "gigaam" ? "Local GigaAM" : "HTTP server"} · {p.model} · {p.id}</span>
                  </span>
                  <span class="voice-ready" classList={{ ok: p.ready }}>{p.ready ? "Ready" : "Not ready"}</span>
                </button>
              )}
            </For>
          </Show>
        </div>
        <Show when={current() && !loading()}>
          <Lines class="muted" text={`${current()!.kind === "openai_http" ? current()!.baseUrl : `Device: ${current()!.device}`}\nLanguage: ${current()!.language || "Auto-detect"}`} />
        </Show>
        <Actions>
          <button class="btn" disabled={!ready()} onClick={() => { setDialogCtx("speechEdit", ""); setDialog("settings-speech-provider"); }}><Icon name="plus" size={13} />Add</button>
          <button class="btn" disabled={!ready() || !current()} onClick={() => { setDialogCtx("speechEdit", current()!.id); setDialog("settings-speech-provider"); }}><Icon name="pencil" size={13} />Edit</button>
          <button class="btn" disabled={!ready() || !current()} onClick={() => { setSpeechResult(""); setTimeout(() => setSpeechResult(`The provider test on ${hostName(host())} completed successfully.`), 900); toast("Testing provider", "A short synthetic recording; no microphone, no agent."); }}><Icon name="play" size={13} />Test</button>
          <button class="btn danger" disabled={!ready() || !current()} onClick={() => setDialog("settings-speech-remove")}><Icon name="trash" size={13} />Remove</button>
          <span class="grow" />
          <button class="btn" disabled={loading() || !current()} onClick={() => { setPrefs("speechDefault", current()!.id); setDefaultHost(host()); }}><Icon name="star" size={13} />Use as default</button>
        </Actions>
        <p class="set-group-hint">Test sends a short synthetic recording from the selected computer. It may load a prepared model or use the remote API; it does not use your microphone or send anything to an agent.</p>
        <Status>{defaultText()}</Status>
        <Status kind={status().kind}>{status().text}</Status>
        <Show when={speechResult()}><Status kind="ok">{speechResult()}</Status></Show>
        <Show when={pending()}>
          <Status kind="warn">A provider request on macbook is not yet confirmed. Check its status; it will not be sent again automatically.</Status>
        </Show>
        <Actions>
          <button class="btn" onClick={() => toast("Opening", "docs/speech-setup.md on GitHub")}>Speech provider documentation<Icon name="external-link" size={12} /></button>
          <button class="btn" disabled={loading()} onClick={load}><Icon name="refresh-cw" size={13} />Refresh providers and microphones</button>
          <Show when={pending()}><button class="btn" onClick={() => toast("Operation status", "The request on macbook completed. Nothing was sent again.")}>Check operation status</button></Show>
        </Actions>
      </Group>
      <Group title="This device">
        <div class="set-row">
          <div class="set-row-text"><label class="set-label" for="voice-mic">Microphone on this device</label></div>
          <div class="set-row-control"><Select id="voice-mic" value={mic()} onChange={setMic} options={mics().map((m) => ({ value: m, label: m }))} /></div>
        </div>
        <p class="set-group-hint">Provider settings and credentials stay on their processing computer. Recordings are limited to two minutes. Original audio and recognized text remain in conversation history; your typed draft stays separate.</p>
        <Show when={!loading()} fallback={<Status kind="busy">Loading this computer's voice statistics…</Status>}>
          <Show when={demoState() !== "error"} fallback={<Status kind="error">Voice statistics unavailable.</Status>}>
            <Lines class="muted voice-stats" text={VOICE_STATS} />
          </Show>
        </Show>
      </Group>
    </>
  );
}
