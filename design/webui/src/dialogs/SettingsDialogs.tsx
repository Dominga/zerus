import { For, Show, createEffect, createMemo, createSignal, onCleanup, type JSX } from "solid-js";
import { Dialog, Confirm } from "../components/Overlays";
import { Icon, AgentGlyph } from "../lib/icons";
import { dialog, setDialog, toast, demoState } from "../state";
import { resolveSkins, compileSkin, SkinError, type SkinFile, type ResolvedSkin } from "../skin/skin";
import zerusDark from "../../skins/zerus-dark.json";
import zerusLight from "../../skins/zerus-light.json";
import fjord from "../../skins/fjord.json";
import paper from "../../skins/paper.json";
import { ACCOUNT_MODELS, LABEL_SWATCHES, OFFICIAL_RELAY, PAIR_WORKSPACES, PHONES, type SpeechKind, type AccountMember } from "../mock/settings-extra";
import {
  setPrefs, dialogCtx, setDialogCtx, registerSkin, chooseSkin, editing, setEditing, resetAllChords, chordText, isMac,
} from "../screens/settings/store";
import { SkinPreview, variantSummary } from "../screens/settings/Appearance";
import { assignReason, conflictsOf, assignChord, commandTitle } from "../screens/settings/Keyboard";
import { phones, setPhones, setRevokedNote } from "../screens/settings/Phones";
import { speech, setSpeech, selectedProvider, setSelectedProvider, setSpeechResult, hostName } from "../screens/settings/Voice";
import { setAnalyticsNote } from "../screens/settings/Analytics";
import { setLegacyPending, workspaceChange } from "../screens/settings/Mobile";
import { labels, setLabels, labelColor } from "../screens/settings/machine-label";
import { selectedMachine, pendingMachine, commitSelection, removeSelectedMachine, enableLocal, installHgs } from "../screens/Machines";
import {
  accounts, setAccounts, currentAccount, currentMember, accountHosts, online, addTarget, setAccountNotice, setSelectedHost, setSelectedAccount, PROVIDER_NAME,
} from "../screens/Accounts";
import "../styles/settings.css";

// Dialogs opened from Settings, Machines and Accounts (inventory §9). Every
// destructive confirmation starts on its safe button; uncertain outcomes are
// reported, never replayed.

const list = (names: string[]) => (names.length < 2 ? names.join("") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`);

/** A question with Yes/No or custom buttons where the safe answer is the default. */
function Ask(props: { id: string; title: string; body: JSX.Element; yes: string; no?: string; danger?: boolean; onYes: () => void; onNo?: () => void }) {
  return (
    <Dialog id={props.id} title={props.title} width="460px" icon={props.danger ? "triangle-alert" : "circle-question-mark"}
      footer={<>
        <span class="spacer" />
        <button class="btn" autofocus onClick={() => { setDialog(undefined); props.onNo?.(); }}>{props.no ?? "No"}</button>
        <button class={props.danger ? "btn solid-danger" : "btn primary"} onClick={() => { setDialog(undefined); props.onYes(); }}>{props.yes}</button>
      </>}>
      <div class="confirm-body">{props.body}</div>
    </Dialog>
  );
}

// ------------------------------------------------------------------ skins

const BUILTIN = [zerusDark, zerusLight, fjord, paper] as SkinFile[];
const importedFiles: SkinFile[] = [];
const ALLOWED = new Set(["schema", "id", "name", "appearance", "extends", "tokens", "components", "layout"]);
const SAMPLE = JSON.stringify({
  schema: 1, id: "ember", name: "Ember", appearance: "dark", extends: "zerus-dark",
  tokens: {
    color: { accent: "#f0a35e", "accent-hover": "#f6bf8a", "accent-muted": "#a8703f", "accent-bg": "rgba(240,163,94,.12)", "segment-on": "#3a2b1e", "card-selected": "#2a2119", "card-selected-border": "#6b4a2c", bubble: "#33261b", "bubble-border": "#6b4a2c" },
    radius: { card: 10, dialog: 14 },
  },
  components: { "status-badge": "dot" },
  layout: { rail: "labeled" },
}, null, 2);

type Validation = { ok: true; skin: ResolvedSkin; css: string; file: SkinFile; tokens: number } | { ok: false; error: string };

export function validateSkin(text: string): Validation {
  let file: unknown;
  try { file = JSON.parse(text); } catch (e) { return { ok: false, error: `Not valid JSON: ${(e as Error).message}` }; }
  if (!file || typeof file !== "object" || Array.isArray(file)) return { ok: false, error: "A skin file is one JSON object." };
  const f = file as Record<string, unknown>;
  for (const key of Object.keys(f))
    if (!ALLOWED.has(key)) return { ok: false, error: `Unknown field “${key}”. A skin carries tokens, component variants and layout variants only, never CSS or code.` };
  if (f.schema !== 1) return { ok: false, error: `Unsupported schema ${String(f.schema)}. This Zerus reads skin schema 1.` };
  if (typeof f.name !== "string" || !f.name.trim() || f.name.length > 40) return { ok: false, error: "name: 1 to 40 characters." };
  if (BUILTIN.some((b) => b.id === f.id)) return { ok: false, error: `A built-in skin already uses the id “${String(f.id)}”. Choose another id.` };
  for (const group of ["tokens", "components", "layout"] as const)
    if (f[group] !== undefined && (typeof f[group] !== "object" || Array.isArray(f[group]))) return { ok: false, error: `${group}: expected an object.` };
  try {
    const others = importedFiles.filter((x) => x.id !== f.id);
    const resolved = resolveSkins([...BUILTIN, ...others, f as unknown as SkinFile]);
    const skin = resolved[resolved.length - 1];
    const tokens = Object.values((f.tokens ?? {}) as Record<string, object>).reduce((n, g) => n + Object.keys(g).length, 0);
    return { ok: true, skin, css: compileSkin(skin), file: f as unknown as SkinFile, tokens };
  } catch (e) {
    return { ok: false, error: e instanceof SkinError ? e.message : String(e) };
  }
}

function SkinImportDialog() {
  const [text, setText] = createSignal(SAMPLE);
  const [fileName, setFileName] = createSignal("");
  const result = createMemo(() => validateSkin(text()));
  let picker!: HTMLInputElement;
  const add = () => {
    const r = result();
    if (!r.ok) return;
    const i = importedFiles.findIndex((x) => x.id === r.file.id);
    if (i >= 0) importedFiles.splice(i, 1, r.file); else importedFiles.push(r.file);
    registerSkin(r.skin, r.css);
    const applied = chooseSkin(r.skin.id);
    setDialog(undefined);
    toast("Skin imported", applied ? `${r.skin.name} is in use. It is saved in the skins folder on this device.` : `${r.skin.name} is used with the ${r.skin.appearance} appearance.`);
  };
  return (
    <Dialog id="settings-skin-import" title="Import skin" width="760px" icon="palette"
      footer={<>
        <span class="hint">Schema 1: tokens, component variants and layout variants. CSS, selectors, URLs and code are rejected.</span>
        <span class="spacer" />
        <button class="btn" onClick={() => setDialog(undefined)}>Cancel</button>
        <button class="btn primary" disabled={!result().ok} onClick={add}>Add skin</button>
      </>}>
      <div class="si-grid">
        <div class="col" style={{ gap: "8px" }}>
          <div class="row">
            <button class="btn" onClick={() => picker.click()}><Icon name="folder-open" size={14} />Choose file…</button>
            <span class="muted ell">{fileName() || "Or paste the skin JSON below"}</span>
            <input ref={picker} type="file" accept=".json,application/json" hidden onChange={async (e) => {
              const f = e.currentTarget.files?.[0];
              if (!f) return;
              if (f.size > 256 * 1024) { setFileName(f.name); setText("{}"); return; }
              setFileName(f.name); setText(await f.text());
            }} />
          </div>
          <textarea class="field mono si-text" spellcheck={false} aria-label="Skin JSON" value={text()} onInput={(e) => setText(e.currentTarget.value)} />
        </div>
        <div class="si-result">
          <Show when={result().ok ? result() as Extract<Validation, { ok: true }> : undefined} fallback={
            <div class="notice danger"><Icon name="circle-alert" size={14} /><span>{(result() as { error: string }).error}</span></div>
          }>
            {(r) => (
              <>
                <SkinPreview skin={r().skin} />
                <div class="si-name"><b>{r().skin.name}</b><span class="chip">{r().skin.appearance === "dark" ? "Dark" : "Light"}</span></div>
                <div class="kv"><span class="muted">Id</span><span class="mono">{r().skin.id}</span></div>
                <div class="kv"><span class="muted">Based on</span><span>{r().file.extends ?? "zerus-dark (base)"}</span></div>
                <div class="kv"><span class="muted">Tokens changed</span><span class="tabular">{r().tokens}</span></div>
                <p class="muted">{variantSummary(r().skin)}</p>
                <div class="notice"><Icon name="check" size={14} /><span>Valid skin. Every value matched its token type.</span></div>
              </>
            )}
          </Show>
        </div>
      </div>
    </Dialog>
  );
}

// ------------------------------------------------------------------ keyboard

function chordFromEvent(e: KeyboardEvent): string | undefined {
  const k = e.key;
  if (["Control", "Shift", "Alt", "Meta", "AltGraph", "CapsLock"].includes(k)) return undefined;
  const code = e.code;
  let key = code.startsWith("Key") ? code.slice(3) : code.startsWith("Digit") ? code.slice(5)
    : ({ Backquote: "`", Comma: ",", Period: ".", Slash: "/", Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Semicolon: ";", Quote: "'", Backslash: "\\" } as Record<string, string>)[code]
    ?? ({ ArrowDown: "Down", ArrowUp: "Up", ArrowLeft: "Left", ArrowRight: "Right", Enter: "Return", PageDown: "PgDown", PageUp: "PgUp", " ": "Space", Escape: "Esc", Delete: "Del", Insert: "Ins", ContextMenu: "Menu" } as Record<string, string>)[k]
    ?? k;
  if (key.length === 1) key = key.toUpperCase();
  // Portable notation: on macOS ⌘ is "Ctrl" and Control is "Meta".
  const ctrl = isMac ? e.metaKey : e.ctrlKey;
  const meta = isMac ? e.ctrlKey : e.metaKey;
  return `${meta ? "Meta+" : ""}${ctrl ? "Ctrl+" : ""}${e.altKey ? "Alt+" : ""}${e.shiftKey ? "Shift+" : ""}${key}`;
}

function ShortcutEditDialog() {
  const [reason, setReason] = createSignal("");
  let last = 0;
  const e = () => editing();
  const capture = (ev: KeyboardEvent) => {
    const plain = !ev.ctrlKey && !ev.metaKey && !ev.altKey;
    if (plain && (ev.key === "Escape" || ev.key === "Tab")) return;
    // The chord being recorded must not also run its current command.
    ev.preventDefault();
    ev.stopPropagation();
    const chord = chordFromEvent(ev);
    if (!chord || !e()) return;
    const now = performance.now();
    const prev = e()!.captured;
    const append = prev && !prev.includes(", ") && now - last < 1200 && last > 0;
    last = now;
    setReason("");
    setEditing({ ...e()!, captured: append ? `${prev}, ${chord}` : chord });
  };
  const accept = () => {
    const cur = e();
    if (!cur) return;
    const why = cur.captured ? assignReason(cur.captured) : "";
    if (why) { setReason(why); return; }
    const others = conflictsOf(cur.id, cur.captured);
    if (others.length) { setEditing({ ...cur, conflicts: others }); setDialog("settings-shortcut-conflict"); return; }
    assignChord(cur.id, cur.slot, cur.captured, []);
    setDialog(undefined);
  };
  createEffect(() => { if (dialog() === "settings-shortcut-edit") { setReason(""); last = 0; queueMicrotask(() => document.getElementById("kb-capture")?.focus()); } });
  return (
    <Dialog id="settings-shortcut-edit" title="Change shortcut" width="440px" icon="keyboard"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" onClick={accept}>OK</button></>}>
      <p>Press the new shortcut for {commandTitle(e()?.id ?? "session.new")}.</p>
      <div class="row kb-edit">
        <div id="kb-capture" class="field kb-capture" tabindex={0} role="textbox" aria-label="New shortcut" aria-live="polite" onKeyDown={capture}>
          <Show when={e()?.captured} fallback={<span class="dim">Press keys…</span>}>
            <span class="kbd">{chordText(e()!.captured)}</span>
          </Show>
        </div>
        <button class="btn" onClick={() => { if (e()) setEditing({ ...e()!, captured: "" }); setReason(""); document.getElementById("kb-capture")?.focus(); }}>Clear</button>
      </div>
      <Show when={reason()}><p class="danger kb-reason">{reason()}</p></Show>
    </Dialog>
  );
}

// ------------------------------------------------------------------ phones

function Qr(props: { seed: number; expired: boolean }) {
  const N = 29;
  const cells = createMemo(() => {
    let s = props.seed * 2654435761 + 12345;
    const rnd = () => ((s = (s * 1103515245 + 12345) >>> 0) >>> 16) & 1;
    const out: [number, number][] = [];
    const finder = (x: number, y: number) => x < 8 && y < 8;
    for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
      if (finder(x, y) || finder(N - 1 - x, y) || finder(x, N - 1 - y)) continue;
      if (rnd()) out.push([x, y]);
    }
    return out;
  });
  const eye = (x: number, y: number) => (
    <>
      <rect x={x} y={y} width={7} height={7} class="qr-ink" />
      <rect x={x + 1} y={y + 1} width={5} height={5} class="qr-paper" />
      <rect x={x + 2} y={y + 2} width={3} height={3} class="qr-ink" />
    </>
  );
  return (
    <svg class="qr" classList={{ expired: props.expired }} viewBox={`-2 -2 ${N + 4} ${N + 4}`} role="img" aria-label="Pairing QR code">
      <rect x={-2} y={-2} width={N + 4} height={N + 4} class="qr-paper" />
      <For each={cells()}>{([x, y]) => <rect x={x} y={y} width={1} height={1} class="qr-ink" />}</For>
      {eye(0, 0)}{eye(N - 7, 0)}{eye(0, N - 7)}
    </svg>
  );
}

function PairPhoneDialog() {
  const [workspace, setWorkspace] = createSignal(PAIR_WORKSPACES[0]);
  const [left, setLeft] = createSignal(600);
  const [seed, setSeed] = createSignal(1);
  const [manual, setManual] = createSignal(false);
  createEffect(() => {
    if (dialog() !== "settings-phone-pair") return;
    setLeft(600);
    const t = setInterval(() => setLeft((v) => Math.max(0, v - 1)), 1000);
    onCleanup(() => clearInterval(t));
  });
  const code = () => `zp1.${(seed() * 7919).toString(36)}q8f2kx${workspace() === PAIR_WORKSPACES[0] ? "a" : "b"}9v4m1t`;
  const expired = () => left() === 0;
  const fresh = () => { setSeed(seed() + 1); setLeft(600); };
  return (
    <Dialog id="settings-phone-pair" title="Pair a phone" width="520px" icon="smartphone"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Close</button></>}>
      <div class="form-row">
        <label for="pp-ws">Workspace</label>
        <select id="pp-ws" class="field" value={workspace()} onChange={(e) => { setWorkspace(e.currentTarget.value); fresh(); }}>
          <For each={PAIR_WORKSPACES}>{(w) => <option value={w}>{w}</option>}</For>
        </select>
      </div>
      <Show when={demoState() !== "empty"} fallback={<div class="notice warn"><Icon name="triangle-alert" size={14} /><span>No workspace is available. Close this dialog and refresh Phones.</span></div>}>
        <div class="pp-body">
          <Qr seed={seed()} expired={expired()} />
          <div class="col" style={{ gap: "8px" }}>
            <p class="pp-ws">Workspace: {workspace()}<br /><span class="muted">End-to-end encrypted. Keep this computer online until the phone confirms pairing.</span></p>
            <p>In Zerus on your phone, open Machines → Pair another workspace and scan this code.</p>
            <div class="kv"><span class="muted">Server</span><span class="mono">{OFFICIAL_RELAY}</span></div>
            <p class="tabular" classList={{ warn: expired() }}>{expired() ? "This code expired. Choose New code to pair." : `Expires in ${Math.floor(left() / 60)}:${String(left() % 60).padStart(2, "0")}`}</p>
          </div>
        </div>
        <button class="btn ghost set-disclosure" aria-expanded={manual()} onClick={() => setManual(!manual())}>
          <Icon name={manual() ? "chevron-down" : "chevron-right"} size={14} />Enter the code manually
        </button>
        <Show when={manual()}>
          <input class="field mono pp-code" readOnly aria-label="Pairing code" value={expired() ? "" : code()} />
        </Show>
        <div class="set-actions">
          <button class="btn" disabled={expired()} onClick={() => toast("Copied", "Pairing code copied. It stays a credential until used or expired.")}><Icon name="copy" size={13} />Copy code</button>
          <button class="btn" disabled={expired()} onClick={() => toast("Copied", "zerus://pair?… link copied.")}><Icon name="link" size={13} />Copy link</button>
          <button class="btn" onClick={fresh}><Icon name="refresh-cw" size={13} />New code</button>
        </div>
      </Show>
    </Dialog>
  );
}

// ------------------------------------------------------------------ speech

function SpeechProviderDialog() {
  const editId = () => dialogCtx.speechEdit;
  const host = () => dialogCtx.speechHost;
  const existing = () => (speech[host()] ?? []).find((p) => p.id === editId());
  const [name, setName] = createSignal("");
  const [id, setId] = createSignal("");
  const [kind, setKind] = createSignal<SpeechKind>("gigaam");
  const [model, setModel] = createSignal("v3_e2e_rnnt");
  const [lang, setLang] = createSignal("ru");
  const [device, setDevice] = createSignal("cpu");
  const [url, setUrl] = createSignal("");
  const [http, setHttp] = createSignal(false);
  const [keyMode, setKeyMode] = createSignal("none");
  const [error, setError] = createSignal("");
  createEffect(() => {
    if (dialog() !== "settings-speech-provider") return;
    const p = existing();
    setName(p?.name ?? ""); setId(p?.id ?? ""); setKind(p?.kind ?? "gigaam"); setModel(p?.model ?? "v3_e2e_rnnt"); setLang(p?.language ?? "ru");
    setDevice(p?.device ?? "cpu"); setUrl(p?.baseUrl ?? ""); setHttp(p?.allowHttp ?? false); setKeyMode(p?.apiKey ? "keep" : "none"); setError("");
  });
  const ready = () => (speech[host()] ?? []).every((p) => p.ready);
  const save = () => {
    const list = speech[host()] ?? [];
    if (!existing() && list.some((p) => p.id === id().trim())) { setError("That profile ID already exists. Edit the existing provider or choose another ID."); return; }
    if (!name().trim() || !id().trim()) { setError("Enter a name and a profile ID."); return; }
    const entry = { id: id().trim(), name: name().trim(), kind: kind(), model: model(), language: lang(), device: kind() === "gigaam" ? device() : undefined, baseUrl: kind() === "openai_http" ? url() : undefined, allowHttp: http(), apiKey: keyMode() === "keep" || keyMode() === "replace", ready: true };
    setSpeech(host(), existing() ? list.map((p) => (p.id === entry.id ? entry : p)) : [...list, entry]);
    setSelectedProvider(entry.id);
    setSpeechResult(`Provider configuration saved on ${hostName(host())}.`);
    setDialog(undefined);
  };
  return (
    <Dialog id="settings-speech-provider" title={existing() ? "Edit speech provider" : "Add speech provider"} width="600px" icon="mic"
      footer={<>
        <button class="btn ghost" onClick={() => toast("Opening", kind() === "gigaam" ? "Local GigaAM documentation" : "HTTP transcription documentation")}>{kind() === "gigaam" ? "Local GigaAM documentation" : "HTTP transcription documentation"}<Icon name="external-link" size={12} /></button>
        <span class="spacer" />
        <button class="btn" onClick={() => setDialog(undefined)}>Cancel</button>
        <button class="btn primary" onClick={save}>Save</button>
      </>}>
      <p class="muted">Processing computer: {hostName(host())}</p>
      <p class={ready() ? "muted" : "warn"}>{ready() ? `Recognition service is available on ${hostName(host())}. Test the saved profile to verify the selected provider.` : `Recognition service is not ready on ${hostName(host())}. Prepare the speech service on this processing computer before testing.`}</p>
      <div class="form-row"><label for="voice-name">Name</label><input id="voice-name" class="field" maxLength={128} placeholder={kind() === "gigaam" ? `e.g. Russian on ${hostName(host())}` : "e.g. Team transcription server"} value={name()} onInput={(e) => setName(e.currentTarget.value)} /></div>
      <div class="form-row"><label for="voice-id">Profile ID</label><input id="voice-id" class="field mono" maxLength={64} placeholder="e.g. Russian speech" readOnly={!!existing()} value={id()} onInput={(e) => setId(e.currentTarget.value)} /></div>
      <div class="form-row"><label for="voice-type">Type</label>
        <select id="voice-type" class="field" value={kind()} disabled={!!existing()} onChange={(e) => setKind(e.currentTarget.value as SpeechKind)}>
          <option value="gigaam">Local GigaAM</option><option value="openai_http">OpenAI-compatible server</option>
        </select>
      </div>
      <div class="form-row"><label for="voice-model">Model</label><input id="voice-model" class="field mono" value={model()} onInput={(e) => setModel(e.currentTarget.value)} /></div>
      <div class="form-row"><label for="voice-lang">Language</label><input id="voice-lang" class="field mono voice-lang" maxLength={2} placeholder="Auto-detect" value={lang()} onInput={(e) => setLang(e.currentTarget.value)} /></div>
      <Show when={kind() === "gigaam"} fallback={<>
        <div class="form-row"><label for="voice-url">Base URL</label><input id="voice-url" class="field mono" maxLength={2048} placeholder="https://speech.example.com/v1" value={url()} onInput={(e) => setUrl(e.currentTarget.value)} /></div>
        <div class="form-row"><span /><label class="check"><input type="checkbox" checked={http()} onChange={(e) => setHttp(e.currentTarget.checked)} />Allow unencrypted HTTP for a local or private server</label></div>
        <div class="form-row"><label for="voice-key">API key</label>
          <select id="voice-key" class="field" value={keyMode()} onChange={(e) => setKeyMode(e.currentTarget.value)}>
            <Show when={existing()?.apiKey}><option value="keep">Keep stored API key</option></Show>
            <option value="none">No API key</option>
            <option value="replace">Replace API key</option>
            <Show when={existing()?.apiKey}><option value="remove">Remove API key</option></Show>
          </select>
        </div>
        <Show when={keyMode() === "replace"}><div class="form-row"><label for="voice-newkey">New API key</label><input id="voice-newkey" class="field" type="password" maxLength={8192} autocomplete="off" /></div></Show>
        <p class="hint voice-help">The processing computer sends audio to the server's /audio/transcriptions endpoint. Use the server's model ID and a two-letter language code, or leave language empty.<br /><br />The optional API key is saved on the processing computer. Prefer HTTPS; private HTTP requires explicit consent. Saving sends no audio.</p>
      </>}>
        <div class="form-row"><label for="voice-dev">Device</label>
          <select id="voice-dev" class="field" value={device()} onChange={(e) => setDevice(e.currentTarget.value)}><option value="cpu">cpu</option><option value="cuda">cuda</option><option value="mps">mps</option></select>
        </div>
        <p class="hint voice-help">Local inference runs on the processing computer named above. Saving creates a profile; it does not install the service or download model weights.<br /><br />v3_e2e_rnnt is the default for Russian with punctuation; use ru for Russian.<br /><br />CPU uses the processor. CUDA requires a supported NVIDIA GPU and CUDA-enabled PyTorch. MPS requires a supported Apple GPU/runtime. Use Test to verify the chosen model and device.</p>
      </Show>
      <Show when={error()}><p class="danger">{error()}</p></Show>
    </Dialog>
  );
}

// ------------------------------------------------------------------ analytics review

function AnalyticsReviewDialog() {
  const json = JSON.stringify({
    schema: 1, generated_utc: "2026-10-27T15:42:00Z", range: { from: "2026-10-14", through: "2026-10-27" },
    computers: [{ computer: 1, current: true, coverage: "complete" }, { computer: 2, current: true, coverage: "partial", gaps: ["2026-10-19"] }, { computer: 3, current: false }],
    totals: { completed_turns: 1284, work_ms: 220320000, approval_wait_ms: 15840000, input_wait_ms: 31260000 },
    providers: { claude: { turns: 611 }, codex: { turns: 402 }, kimi: { turns: 163 }, deepseek: { turns: 108 } },
  }, null, 2);
  return (
    <Dialog id="settings-analytics-review" title="Review aggregate diagnostics" width="620px" icon="file-text"
      footer={<><span class="spacer" /><button class="btn" autofocus onClick={() => setDialog(undefined)}>Cancel</button>
        <button class="btn primary" onClick={() => { setDialog(undefined); setAnalyticsNote("Aggregate diagnostics saved locally."); toast("Saved", "zerus-aggregate-diagnostics.json"); }}>Save file…</button></>}>
      <p class="muted">Review these aggregate fields before saving a local JSON file. Computer numbers are anonymous within this file. This export includes no logs, message content, account details or native identifiers, and is never uploaded automatically.</p>
      <pre class="an-json selectable">{json}</pre>
    </Dialog>
  );
}

// ------------------------------------------------------------------ misc settings dialogs

function LoginItemDialog() {
  return (
    <Dialog id="settings-login-item" title="Fix the Zerus login item" width="520px" icon="triangle-alert"
      footer={<><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><span class="spacer" />
        <button class="btn" onClick={() => { setDialog(undefined); toast("Restarting Zerus", "The login item is updated. Drafts are saved; agents keep running."); }}>Update and Restart</button>
        <button class="btn primary" autofocus onClick={() => { setDialog(undefined); toast("Login item", "The Zerus login item is updated; it applies at your next login."); }}>Update Only</button></>}>
      <p><b>macOS runs Zerus at background priority.</b></p>
      <p class="muted">Zerus was started by its login item, which macOS runs at background priority, so Zerus can stop responding while other programs keep the computer busy. Updating the login item makes macOS start Zerus like other apps.</p>
      <p class="muted">Update Only applies this at your next login; Zerus keeps running.<br />Update and Restart applies it now: Zerus quits and opens again. Saved drafts, sessions and agents stay; a voice or video message that is still recording stops and is not kept.</p>
    </Dialog>
  );
}

function StartupNotice() {
  return (
    <Dialog id="settings-startup-notice" title="Starting Zerus" width="360px" icon="loader-circle">
      <div class="set-startup"><span class="set-spinner" aria-hidden="true" /><div><b>Starting Zerus…</b><br /><span class="muted">Updating the hgs command line for local sessions</span></div></div>
      <p class="hint">Windows only: a frameless, non-activating notice shown after 500 ms while Zerus chooses its hgs.</p>
    </Dialog>
  );
}

// ------------------------------------------------------------------ machines

function MachineColorDialog() {
  const alias = () => (selectedMachine() === "@local" ? "workstation" : selectedMachine());
  const [hex, setHex] = createSignal("");
  const [vivid, setVivid] = createSignal(false);
  createEffect(() => {
    if (dialog() !== "machines-color") return;
    setHex(labels[alias()]?.hex ?? LABEL_SWATCHES[6]);
    setVivid(labels[alias()]?.vivid ?? false);
  });
  const valid = () => /^#[0-9a-f]{6}$/i.test(hex());
  return (
    <Dialog id="machines-color" title={`Label color for ${alias()}`} width="380px" icon="palette"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button>
        <button class="btn primary" disabled={!valid()} onClick={() => { setLabels(alias(), { hex: hex(), vivid: vivid() }); setDialog(undefined); }}>Apply</button></>}>
      <div class="cd-preview"><span class="entity mach-label" classList={{ vivid: vivid() }} style={{ "--entity": valid() ? hex() : labelColor(alias()) }}>{alias()}</span></div>
      <div class="cd-swatches" role="radiogroup" aria-label="Color">
        <For each={LABEL_SWATCHES}>
          {(c) => <button class="cd-swatch" role="radio" aria-checked={hex().toLowerCase() === c} aria-label={c} classList={{ on: hex().toLowerCase() === c }} style={{ "--sw": c }} onClick={() => setHex(c)} />}
        </For>
      </div>
      <div class="form-row"><label for="cd-hex">Hex</label><input id="cd-hex" class="field mono" maxLength={7} value={hex()} aria-invalid={!valid()} onInput={(e) => setHex(e.currentTarget.value)} /></div>
      <div class="form-row"><label for="cd-style">Style</label>
        <select id="cd-style" class="field" value={vivid() ? "bright" : "soft"} onChange={(e) => setVivid(e.currentTarget.value === "bright")}><option value="soft">Soft fill</option><option value="bright">Bright fill</option></select>
      </div>
      <label class="btn ghost cd-more">More colors…<input type="color" value={valid() ? hex() : "#2b97ff"} onInput={(e) => setHex(e.currentTarget.value)} /></label>
    </Dialog>
  );
}

function MachineInstallDialog() {
  const alias = () => selectedMachine();
  const [source, setSource] = createSignal("");
  return (
    <Dialog id="machines-install" title={`Install HGS on ${alias()}`} width="520px" icon="download"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button>
        <button class="btn primary" onClick={() => { setDialog(undefined); installHgs(alias()); }}>Install HGS</button></>}>
      <p>Build and install the HGS CLI on {alias()} using SSH. The newest stable Rust and tmux must already be installed there. Existing sessions and configuration are preserved.</p>
      <div class="row" style={{ "margin-top": "12px" }}>
        <input class="field mono grow" aria-label="HGS source folder" placeholder="Default: ~/.local/src/hgs on this device" value={source()} onInput={(e) => setSource(e.currentTarget.value)} />
        <button class="btn" onClick={() => setSource("/home/example/src/zerus")}>Choose HGS source folder…</button>
      </div>
    </Dialog>
  );
}

// ------------------------------------------------------------------ accounts

const SAVED_PROFILES: Record<string, string[]> = { "workstation:claude": ["Old team (team@example.com)"], "macbook:codex": [] };

function AddAccountDialog() {
  const hosts = () => accountHosts();
  const [host, setHost] = createSignal("workstation");
  const [agent, setAgent] = createSignal("claude");
  const [saved, setSaved] = createSignal("");
  const [name, setName] = createSignal("");
  const savedFor = () => SAVED_PROFILES[`${host()}:${agent()}`] ?? [];
  const reuse = () => !!saved();
  const ocListed = () => accounts.some((a) => a.provider === "opencode" && a.members.some((m) => m.host === host()));
  const hint = () => (!online(host()) ? `${host()} is offline.`
    : reuse() && agent() === "opencode" ? `Use OpenCode's native configuration on ${host()} for new sessions again.`
    : reuse() ? `Use the saved sign-in on ${host()}. The account will be available for new sessions again.`
    : agent() === "deepseek" ? "Configure a DeepSeek API key on the selected machine."
    : "Create a separate account profile on the selected machine, then sign in.");
  const action = () => (reuse() ? "Add account" : agent() === "deepseek" ? "Enter API key…" : "Create and sign in");
  const can = () => online(host()) && agent() !== "opencode" && (reuse() || (name().trim().length > 0 && name().length <= 160));
  const AGENTS = ["codex", "claude", "kimi", "deepseek", "opencode"] as const;
  const detail = (a: string) => a === "deepseek" ? "DeepSeek API key" : a === "opencode" ? (ocListed() ? "Native configuration, already added" : `Not installed on ${host()}`)
    : (SAVED_PROFILES[`${host()}:${a}`] ?? []).length ? "Saved account or a new profile" : "New profile with its own sign-in";
  const create = () => {
    const label = reuse() ? saved().replace(/ \(.*\)$/, "") : name().trim();
    setDialog(undefined);
    if (agent() === "deepseek" && !reuse()) { setDialog("accounts-api-key"); return; }
    const key = `a-new-${Date.now()}`;
    setAccounts((list) => [...list, { key, label, provider: agent() as never, members: [{ host: host(), id: `${agent()}-${key}`, state: reuse() ? "ready" : "signed-out", isDefault: false, permission: "provider" }], usage: { windows: [], state: "Loading account details…" } }]);
    setSelectedAccount(key); setSelectedHost(host());
    setAccountNotice(reuse() ? `Account added on ${host()} using its saved sign-in.` : `Complete ${PROVIDER_NAME[agent()]} sign-in in the terminal on ${host()}, this page updates automatically.`);
  };
  return (
    <Dialog id="accounts-add" title="Add agent account" width="520px" icon="user-round-cog"
      footer={<><span class="hint">{hint()}</span><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button>
        <button class="btn primary acct-add-btn" disabled={!can()} onClick={create}>{action()}</button></>}>
      <div class="form-row"><label for="aa-host">Machine</label>
        <select id="aa-host" class="field" value={host()} onChange={(e) => { setHost(e.currentTarget.value); setSaved(""); }}>
          <For each={hosts()}>{(h) => <option value={h}>{h === "workstation" ? `${h} (this machine)` : h}{online(h) ? "" : " (offline)"}</option>}</For>
        </select>
      </div>
      <div class="form-row acct-agents-row"><label>Agent</label>
        <div class="acct-agents" role="listbox" aria-label="Agent">
          <For each={AGENTS}>
            {(a) => {
              const disabled = () => a === "opencode";
              return (
                <button role="option" class="acct-agent" classList={{ on: agent() === a }} aria-selected={agent() === a} disabled={disabled()}
                  title={a === "opencode" ? (ocListed() ? `Configure providers, models and permissions with OpenCode on ${host()}. Zerus uses that native configuration.` : `Install OpenCode on ${host()}, then refresh Accounts.`) : undefined}
                  style={{ "--agent": `var(--z-agent-${a})` }} onClick={() => { setAgent(a); setSaved(""); }}>
                  <AgentGlyph agent={a} size={16} />
                  <span class="col"><b>{PROVIDER_NAME[a]}</b><span class="muted">{detail(a)}</span></span>
                </button>
              );
            }}
          </For>
        </div>
      </div>
      <div class="form-row"><label for="aa-saved">Account</label>
        <select id="aa-saved" class="field" value={saved()} disabled={!savedFor().length} onChange={(e) => setSaved(e.currentTarget.value)}>
          <For each={savedFor()}>{(s) => <option value={s}>{s}</option>}</For>
          <option value="">New account</option>
        </select>
      </div>
      <div class="form-row"><label for="aa-name">Account name</label>
        <input id="aa-name" class="field" maxLength={160} placeholder="Work, Personal, Team…" disabled={reuse()} value={reuse() ? "" : name()} onInput={(e) => setName(e.currentTarget.value)} />
      </div>
    </Dialog>
  );
}

function AddToMachineDialog() {
  const a = () => currentAccount();
  const targets = () => accountHosts().filter((h) => online(h) && !a()?.members.some((m) => m.host === h));
  const [host, setHost] = createSignal("");
  const [how, setHow] = createSignal<"saved" | "copy" | "signin">("copy");
  createEffect(() => { if (dialog() === "accounts-add-to-machine") { setHost(addTarget() !== "*" ? addTarget() : targets()[0] ?? ""); setHow("copy"); } });
  const sources = () => (a()?.members ?? []).filter((m) => online(m.host) && m.state === "ready");
  const add = () => {
    const ai = accounts.findIndex((x) => x.key === a()!.key);
    const member: AccountMember = { host: host(), id: `${a()!.provider}-${host()}`, state: how() === "signin" ? "signed-out" : "ready", isDefault: false, permission: "provider" };
    setAccounts(ai, "members", (list) => [...list, member]);
    setSelectedHost(host());
    setAccountNotice(how() === "copy" ? "Account copied into a new profile. It is available in New session on that machine." : how() === "saved" ? `Account added on ${host()} using its saved sign-in.` : `Complete ${PROVIDER_NAME[a()!.provider]} sign-in in the terminal on ${host()}, this page updates automatically.`);
    setDialog(undefined);
  };
  return (
    <Dialog id="accounts-add-to-machine" title="Add account to a machine" width="520px" icon="user-round-cog"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" disabled={!host()} onClick={add}>Add</button></>}>
      <p>Use {a()?.label} on another machine.</p>
      <div class="form-row"><label for="atm-host">Machine</label>
        <select id="atm-host" class="field" value={host()} onChange={(e) => setHost(e.currentTarget.value)}><For each={targets()}>{(h) => <option value={h}>{h}</option>}</For></select>
      </div>
      <div class="form-row"><label for="atm-src">Copy from</label>
        <select id="atm-src" class="field" disabled={how() !== "copy" || !sources().length}><For each={sources()}>{(m) => <option>{m.host} / {a()?.label}</option>}</For></select>
      </div>
      <div class="col atm-how" role="radiogroup" aria-label="Sign-in">
        <Show when={host() === "workstation" && a()?.provider === "claude"}>
          <label class="check"><input type="radio" name="atm" checked={how() === "saved"} onChange={() => setHow("saved")} />Use saved sign-in on this machine</label>
        </Show>
        <label class="check"><input type="radio" name="atm" checked={how() === "copy"} disabled={!sources().length} onChange={() => setHow("copy")} />Copy existing sign-in over SSH</label>
        <label class="check"><input type="radio" name="atm" checked={how() === "signin"} onChange={() => setHow("signin")} />Sign in on this machine</label>
      </div>
      <p class="hint">A separate profile keeps other accounts and sessions on that machine intact. After sign-in, the provider identity determines which account it belongs to.</p>
    </Dialog>
  );
}

function ApiKeyDialog() {
  const host = () => (addTarget() !== "*" ? addTarget() : currentMember()?.host ?? "build-01");
  const [key, setKey] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  createEffect(() => { if (dialog() === "accounts-api-key") { setKey(""); setSaving(false); } });
  const save = () => {
    setSaving(true);
    setTimeout(() => { setSaving(false); setDialog(undefined); setAccountNotice(`DeepSeek API key saved on ${host()}.`); }, 700);
  };
  return (
    <Dialog id="accounts-api-key" title={`DeepSeek API key on ${host()}`} width="460px" icon="key-round"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button>
        <button class="btn primary" disabled={!key().trim() || saving()} onClick={save}>{saving() ? `Saving on ${host()}…` : `Save on ${host()}`}</button></>}>
      <p>Save an API key on {host()}. DeepSeek on that machine will use this key.</p>
      <div class="form-row"><label for="ak-key">API key</label><input id="ak-key" class="field" type="password" maxLength={4096} autocomplete="off" value={key()} onInput={(e) => setKey(e.currentTarget.value)} /></div>
    </Dialog>
  );
}

function PermissionsDialog() {
  const a = () => currentAccount();
  const m = () => currentMember();
  const [mode, setMode] = createSignal("provider");
  createEffect(() => { if (dialog() === "accounts-permissions") setMode(m()?.permission ?? "provider"); });
  const explain = () => mode() === "provider"
    ? `Inherit ${PROVIDER_NAME[a()?.provider ?? "claude"]} settings on ${m()?.host}. ${m()?.permission === "bypass" ? "" : m()?.permissionDetail ?? ""}`
    : a()?.provider === "kimi" ? "Run tools and answer questions automatically (Never Ask)."
    : a()?.provider === "codex" || a()?.provider === "deepseek" ? "Full access without approval prompts or a sandbox."
    : "Run tools without permission prompts.";
  const save = () => {
    const ai = accounts.findIndex((x) => x.key === a()!.key);
    const mi = a()!.members.findIndex((x) => x.host === m()!.host);
    if (mode() !== m()!.permission) {
      setAccounts(ai, "members", mi, "permission", mode() as "provider" | "bypass");
      setAccountNotice(`Permission mode saved for new and resumed sessions on ${m()!.host}.`);
    }
    setDialog(undefined);
  };
  return (
    <Dialog id="accounts-permissions" title="Account permissions" width="480px" icon="shield-check"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" onClick={save}>Save</button></>}>
      <p>{a()?.label} on {m()?.host}</p>
      <select class="field acct-perm-select" aria-label="Permission mode" value={mode()} onChange={(e) => setMode(e.currentTarget.value)}>
        <option value="provider">Default (provider settings)</option><option value="bypass">Bypass permissions</option>
      </select>
      <p classList={{ danger: mode() === "bypass" }}>{explain()}</p>
      <p class="hint">New and resumed sessions only. Running sessions keep their mode.</p>
    </Dialog>
  );
}

function ModelDialog() {
  const a = () => currentAccount();
  const m = () => currentMember();
  const models = () => ACCOUNT_MODELS[a()?.provider ?? "claude"] ?? [];
  const [model, setModel] = createSignal("");
  const [effort, setEffort] = createSignal("");
  createEffect(() => { if (dialog() === "accounts-model") { setModel(m()?.model ?? ""); setEffort(m()?.effort ?? ""); } });
  const chosen = () => models().find((x) => x.id === model());
  const kimi = () => a()?.provider === "kimi";
  const save = () => {
    const ai = accounts.findIndex((x) => x.key === a()!.key);
    const mi = a()!.members.findIndex((x) => x.host === m()!.host);
    if (model() !== (m()!.model ?? "") || effort() !== (m()!.effort ?? "")) {
      setAccounts(ai, "members", mi, { model: model() || undefined, effort: effort() || undefined });
      setAccountNotice(`Model defaults saved for new sessions on ${m()!.host}.`);
    }
    setDialog(undefined);
  };
  return (
    <Dialog id="accounts-model" title="Default model and effort" width="480px" icon="sparkles"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" onClick={save}>Save</button></>}>
      <p>{a()?.label} on {m()?.host}</p>
      <div class="form-row"><label for="md-model">Model</label>
        <select id="md-model" class="field" value={model()} onChange={(e) => { setModel(e.currentTarget.value); setEffort(""); }}>
          <option value="">Provider settings</option>
          <For each={models()}>{(x) => <option value={x.id}>{x.id}</option>}</For>
        </select>
      </div>
      <div class="form-row"><label for="md-effort">Effort</label>
        <select id="md-effort" class="field" value={effort()} disabled={kimi() || !model() || !chosen()?.efforts.length} onChange={(e) => setEffort(e.currentTarget.value)}>
          <option value="">Provider default</option>
          <For each={chosen()?.efforts ?? []}>{(x) => <option value={x}>{x}</option>}</For>
        </select>
      </div>
      <Show when={a()?.provider === "claude"}><p class="muted">Account settings: claude-opus-4-6 · high</p></Show>
      <p class="hint">{kimi() ? "Kimi manages effort in its native settings. This choice sets the model for new conversations." : !models().length ? "Open this account's agent once, then refresh to load its models." : "Choose the reasoning effort for the selected model, or keep its provider default."}</p>
      <p class="hint">New conversations with this account on this machine. Existing conversations keep their model and effort. Explicit launch choices take precedence.</p>
    </Dialog>
  );
}

function RenameDialog() {
  const a = () => currentAccount();
  const [name, setName] = createSignal("");
  createEffect(() => { if (dialog() === "accounts-rename") { setName(a()?.label ?? ""); queueMicrotask(() => (document.getElementById("rn-name") as HTMLInputElement | null)?.select()); } });
  const save = () => {
    const ai = accounts.findIndex((x) => x.key === a()!.key);
    if (name().trim() !== a()!.label) { setAccounts(ai, "label", name().trim()); setAccountNotice("Account name updated."); }
    setDialog(undefined);
  };
  return (
    <Dialog id="accounts-rename" title="Rename account" width="420px" icon="pencil"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" disabled={!name().trim()} onClick={save}>Save</button></>}>
      <div class="form-row"><label for="rn-name">Account name</label><input id="rn-name" class="field" maxLength={160} value={name()} onInput={(e) => setName(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter" && name().trim()) save(); }} /></div>
    </Dialog>
  );
}

// ------------------------------------------------------------------ all

export function SettingsDialogs() {
  const phone = () => phones().find((p) => p.id === (dialogCtx.phone ?? "ph1")) ?? PHONES[0];
  const removeHost = () => currentMember()?.host ?? "";
  const shortcutOthers = () => editing()?.conflicts.map(commandTitle) ?? [];
  return (
    <>
      <SkinImportDialog />
      <Confirm id="settings-cache-limit" title="Limit downloaded files?" confirm="Apply limit"
        body={<p>Zerus will automatically delete its oldest downloaded copies when they exceed {dialogCtx.cacheDraftGiB.toFixed(1)} GiB. Files you added or edited will be kept.</p>}
        onConfirm={() => { setPrefs("cacheLimitGiB", dialogCtx.cacheDraftGiB); toast("Files", `Downloaded copies are limited to ${dialogCtx.cacheDraftGiB.toFixed(1)} GiB.`); }} />
      <Confirm id="settings-approvals-enable" title="Enable automatic approvals?" confirm="Enable automatic approvals" danger
        body={<p>Agents on this computer and connected machines will be allowed to run tools without your review, including commands that delete files or consume paid usage. Resuming after an expired cache may process the full conversation again. Zerus will grant one-time permissions and record each attempt in this computer's private journal. Enable this dangerous mode?</p>}
        onConfirm={() => setPrefs("approvals", true)} />
      <ShortcutEditDialog />
      <Ask id="settings-shortcut-conflict" title="Shortcut in use" body={<p>Already used by {list(shortcutOthers())}. Reassign?</p>} yes="Yes"
        onYes={() => { const e = editing(); if (e) assignChord(e.id, e.slot, e.captured, e.conflicts); }}
        onNo={() => setDialog("settings-shortcut-edit")} />
      <Ask id="settings-shortcut-reset-all" title="Reset all shortcuts" body={<p>Restore the default chords of every command?</p>} yes="Yes" onYes={resetAllChords} />
      <Confirm id="settings-analytics-shorten" title="Shorten local analytics history?" confirm="Shorten retention" danger
        body={<p>Keep only the latest {dialogCtx.retentionDraft} UTC days of agent analytics on this computer. Older daily aggregates will be deleted. Speech statistics and durable delivery receipts are unaffected.</p>}
        onConfirm={() => { setDialogCtx("analyticsRetention", dialogCtx.retentionDraft); setAnalyticsNote("Applying local collection settings…"); setTimeout(() => setAnalyticsNote(`Retention is ${dialogCtx.retentionDraft} days on this computer.`), 600); }} />
      <Confirm id="settings-analytics-purge" title="Purge local agent analytics?" confirm="Purge local history" danger
        body={<p>Permanently delete all retained agent analytics on this computer. Speech statistics, other computers and durable delivery receipts are unaffected. Collection keeps its current enabled/disabled setting.</p>}
        onConfirm={() => setAnalyticsNote("Local agent analytics were purged. A new epoch started.")} />
      <AnalyticsReviewDialog />
      <SpeechProviderDialog />
      <Confirm id="settings-speech-remove" title="Remove speech provider?" confirm="Remove provider" danger
        body={<p>Remove {(speech[dialogCtx.speechHost] ?? []).find((p) => p.id === selectedProvider())?.name ?? "this provider"} from {hostName(dialogCtx.speechHost)}? Existing voice messages and recordings are kept. A recording using this provider will need an explicit replacement before sending.</p>}
        onConfirm={() => { const h = dialogCtx.speechHost; const list = (speech[h] ?? []).filter((p) => p.id !== selectedProvider()); setSpeech(h, list); setSelectedProvider(list[0]?.id); }} />
      <Confirm id="settings-workspace-change" title="Change workspace" confirm={workspaceChange()?.mode === "join" ? "Change and join workspace" : "Change and create workspace"} danger
        body={<p>This computer will leave its current workspace. Phone access through this computer will end. Local agents, projects and saved conversations stay on this computer.</p>}
        onConfirm={() => { setLegacyPending(demoState() === "error"); toast("Workspace", demoState() === "error" ? "The new encrypted workspace is ready." : "Workspace connected."); }} />
      <Confirm id="settings-workspace-legacy" title="Disconnect legacy locally" confirm="Disconnect legacy locally and switch" danger
        body={<p>The old relay has not confirmed retirement. Its old records and credentials may remain active on that relay. This only disconnects the saved legacy configuration on this computer and switches to the new encrypted workspace. Local agents, projects and conversations stay on this computer.</p>}
        onConfirm={() => { setLegacyPending(false); toast("Workspace", "The encrypted workspace is connected. Legacy relay retirement remains unconfirmed; its old records or credentials may remain on that relay."); }} />
      <PairPhoneDialog />
      <Confirm id="settings-phone-revoke" title={`Revoke ${phone().name}?`} confirm="Revoke" danger
        body={<p>{phone().name} loses access to every computer of {phone().group.split(" · ")[0]} at once, and its queued requests are cancelled. Pair it again to restore access.</p>}
        onConfirm={() => {
          const p = phone();
          if (demoState() === "error") { setRevokedNote(`Could not confirm revoking ${p.name}: the relay did not answer. Refresh to check whether it is still listed before trying again.`); return; }
          setPhones((list) => list.filter((x) => x.id !== p.id));
          setRevokedNote(`${p.name} revoked.`);
        }} />
      <LoginItemDialog />
      <StartupNotice />

      <MachineColorDialog />
      <Confirm id="machines-remove" title="Remove machine?" confirm="Remove machine" danger
        body={<p>Remove {selectedMachine()} from this Zerus? Its agents keep running and its files stay unchanged.</p>}
        onConfirm={removeSelectedMachine} />
      <Ask id="machines-unsaved" title="Unsaved connection" body={<p>Discard the unsaved changes to this connection?</p>} yes="Discard" no="Cancel" danger
        onYes={() => commitSelection(pendingMachine() ?? selectedMachine())} />
      <Confirm id="machines-enable-local" title="Enable local sessions?" confirm="Enable local sessions"
        body={<p>Run agents on this computer too? Zerus stages the HGS CLI and a session host in your user profile and enables local sessions. To disable local sessions again, run hgs session-host uninstall.</p>}
        onConfirm={enableLocal} />
      <MachineInstallDialog />

      <AddAccountDialog />
      <AddToMachineDialog />
      <ApiKeyDialog />
      <PermissionsDialog />
      <ModelDialog />
      <RenameDialog />
      <Confirm id="accounts-remove" title="Remove account from Zerus" confirm={`Remove from ${removeHost()}`} danger
        body={<>
          <p><b>Remove “{currentAccount()?.label}” from {removeHost()}?</b></p>
          <p class="muted" style={{ "white-space": "pre-line" }}>{currentAccount()?.provider === "opencode"
            ? `OpenCode will disappear from Accounts and New session for ${removeHost()} in every Zerus connected to ${removeHost()}, including Zerus running on ${removeHost()}.\n\nIts native configuration, provider credentials and existing sessions on ${removeHost()} are kept.\n\nTo use it here again, choose Add account.`
            : `This account will disappear from Accounts and New session for ${removeHost()} in every Zerus connected to ${removeHost()}, including Zerus running on ${removeHost()}.\n\nSign-in credentials and existing sessions on ${removeHost()} are kept. The account stays available on its other machines.\n\nTo use it here again, choose Add account or Add to ${removeHost()}. Zerus can reuse the saved sign-in.`}</p>
        </>}
        onConfirm={() => {
          const a = currentAccount();
          if (!a) return;
          const host = removeHost();
          const ai = accounts.findIndex((x) => x.key === a.key);
          if (a.members.length > 1) { setAccounts(ai, "members", (list) => list.filter((m) => m.host !== host)); setSelectedHost(accounts[ai].members[0].host); }
          else { setAccounts((list) => list.filter((x) => x.key !== a.key)); setSelectedAccount(accounts[0]?.key ?? ""); }
          setAccountNotice(`Account removed from ${host}. Add it again whenever you need it.`);
        }} />
    </>
  );
}
