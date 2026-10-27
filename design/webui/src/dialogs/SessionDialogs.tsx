import { For, Show, createMemo, createSignal } from "solid-js";
import { Dialog, Confirm } from "../components/Overlays";
import { Icon, AgentGlyph } from "../lib/icons";
import { catalog, dialog, setDialog, selected, setSelectedKey, setPage, toast, type Page } from "../state";
import { projects, machines, accounts } from "../mock/data";
import { SessionCard } from "../screens/sessions/SessionCard";
import { projectColor } from "../screens/setup/store";

/** detail: separate facts, laid out with space between them (never joined with dots). */
type GoItem = { kind: "Session" | "Project" | "Account" | "Machine" | "Section" | "Command"; title: string; detail: string | string[]; kbd?: string; run: () => void };
const details = (d: string | string[]) => (Array.isArray(d) ? d.filter(Boolean) : d ? [d] : []);

// Go to ranking from GoToDialog.cpp: subsequence match, +10 per character,
// +100 for a match at position 0, +20 at a word start, +10 adjacent, gap penalty.
function score(text: string, query: string): number {
  if (!query) return 0;
  const t = text.toLowerCase(), q = query.toLowerCase();
  let s = 0, pos = -1;
  for (const ch of q) {
    const at = t.indexOf(ch, pos + 1);
    if (at < 0) return -1;
    s += 10;
    if (at === 0) s += 100;
    else if (/[\s/-]/.test(t[at - 1])) s += 20;
    if (at === pos + 1) s += 10;
    else s -= Math.min(at - pos - 1, 8);
    pos = at;
  }
  return s;
}

function GoTo() {
  const [q, setQ] = createSignal("");
  const [i, setI] = createSignal(0);
  const go = (p: Page) => () => setPage(p);
  const items = createMemo<GoItem[]>(() => [
    ...catalog.rows.slice(0, 400).map((s) => ({ kind: "Session" as const, title: s.name, detail: [projects.find((p) => p.id === s.project)?.name ?? "", s.machine, s.status], run: () => { setPage("sessions"); setSelectedKey(s.key); } })),
    ...projects.map((p) => ({ kind: "Project" as const, title: p.name, detail: "Project", run: go("projects") })),
    ...accounts.map((a) => ({ kind: "Account" as const, title: a.label, detail: [a.agent, a.machine], run: go("accounts") })),
    ...machines.map((m) => ({ kind: "Machine" as const, title: m.alias, detail: m.thisComputer ? ["this computer"] : [], run: go("machines") })),
    { kind: "Section", title: "Overview", detail: "Section", kbd: "Ctrl+1", run: go("dashboard") },
    { kind: "Section", title: "Sessions", detail: "Section", kbd: "Ctrl+2", run: go("sessions") },
    { kind: "Section", title: "Projects", detail: "Section", kbd: "Ctrl+3", run: go("projects") },
    { kind: "Section", title: "Machines", detail: "Section", kbd: "Ctrl+4", run: go("machines") },
    { kind: "Section", title: "Accounts", detail: "Section", kbd: "Ctrl+5", run: go("accounts") },
    { kind: "Section", title: "Settings", detail: "Section", kbd: "Ctrl+,", run: go("settings") },
    { kind: "Command", title: "New session", detail: "Command", kbd: "Ctrl+N", run: () => setDialog("new-session") },
    { kind: "Command", title: "Keyboard shortcuts", detail: "Command", kbd: "F1", run: () => setDialog("shortcuts") },
    { kind: "Command", title: "Refresh", detail: "Command", kbd: "Ctrl+R", run: () => toast("Refresh", "Fleet, inspection and machines refreshed") },
    { kind: "Command", title: "Show or hide the side panel", detail: "Command", kbd: "Ctrl+I", run: () => undefined },
  ]);
  const order = ["Session", "Project", "Account", "Machine", "Section", "Command"];
  const results = createMemo(() => {
    const raw = q().trim();
    const commandsOnly = raw.startsWith(">");
    const query = commandsOnly ? raw.slice(1).trim() : raw;
    return items()
      .filter((it) => !commandsOnly || it.kind === "Section" || it.kind === "Command")
      .map((it, idx) => ({ it, idx, s: score(`${it.title} ${details(it.detail).join(" ")}`, query) }))
      .filter((r) => r.s >= 0)
      .sort((a, b) => b.s - a.s || order.indexOf(a.it.kind) - order.indexOf(b.it.kind) || a.idx - b.idx)
      .slice(0, 12)
      .map((r) => r.it);
  });
  const activate = (it?: GoItem) => { if (!it) return; setDialog(undefined); it.run(); };
  return (
    <Show when={dialog() === "goto"}>
      <div class="goto-scrim" onClick={() => setDialog(undefined)}>
        <div class="goto" role="dialog" aria-label="Go to" onClick={(e) => e.stopPropagation()}>
          <div class="goto-field">
            <Icon name="search" size={15} />
            <input autofocus placeholder="Go to… (type > for commands)" value={q()} onInput={(e) => { setQ(e.currentTarget.value); setI(0); }}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") { e.preventDefault(); setI(Math.min(results().length - 1, i() + 1)); }
                else if (e.key === "ArrowUp") { e.preventDefault(); setI(Math.max(0, i() - 1)); }
                else if (e.key === "Enter") activate(results()[i()]);
                else if (e.key === "Escape") setDialog(undefined);
              }} />
            <span class="kbd">Esc</span>
          </div>
          <ul class="goto-list" role="listbox">
            <For each={results()} fallback={<li class="goto-empty">Nothing matches</li>}>{(it, k) => (
              <li role="option" aria-selected={k() === i()} classList={{ on: k() === i() }} onMouseEnter={() => setI(k())} onClick={() => activate(it)}>
                <span class="goto-kind">{it.kind}</span>
                <span class="ell parts goto-text"><b>{it.title}</b><For each={details(it.detail)}>{(d) => <span class="dim">{d}</span>}</For></span>
                <span class="grow" />
                <Show when={it.kbd}><span class="kbd">{it.kbd}</span></Show>
              </li>
            )}</For>
          </ul>
        </div>
      </div>
    </Show>
  );
}

const SHORTCUTS: [string, [string, string][]][] = [
  ["Navigation", [["Go to Overview … Accounts", "Ctrl+1 … Ctrl+5"], ["Open Settings", "Ctrl+,"], ["Go to…", "Ctrl+K"], ["Keyboard shortcuts", "F1, Ctrl+/"], ["Next / previous session", "Alt+Down / Alt+Up"], ["Next / previous that needs attention", "Alt+Shift+Down / Up"], ["Next / previous area", "F6 / Shift+F6"]]],
  ["Sessions", [["New session", "Ctrl+N"], ["Rename session", "F2"], ["Search sessions", "Ctrl+F"], ["Refresh", "Ctrl+R, F5"], ["Next / previous tab", "Ctrl+Tab / Ctrl+Shift+Tab"], ["Activity ↔ Terminal", "Ctrl+`"], ["Show or hide the side panel", "Ctrl+I"]]],
  ["Composer", [["Send message", "Ctrl+Return"], ["Send queued messages now", "Ctrl+Shift+Return"], ["Send and push now", "Ctrl+X, Ctrl+S"], ["Content scale ±10% / reset", "Ctrl+= / Ctrl+- / Ctrl+0"]]],
  ["Fixed keys", [["Send / new line", "Enter / Shift+Enter"], ["Clear search, stop turn, close dialogs", "Escape"], ["Context menu", "Menu, Shift+F10"], ["Select several", "Ctrl+click"], ["Stop voice capture", "Space, Esc"], ["Copy / paste in Terminal", "Ctrl+Shift+C / V"]]],
];

function Shortcuts() {
  const [f, setF] = createSignal("");
  return (
    <Dialog id="shortcuts" title="Keyboard shortcuts" width="580px" icon="keyboard"
      footer={<><span class="hint">Chords marked ✓ also work while Terminal has focus.</span><span class="spacer" /><button class="btn" onClick={() => { setDialog(undefined); setPage("settings"); }}>Customize…</button></>}>
      <input class="field" style={{ width: "100%", "margin-bottom": "8px" }} placeholder="Filter shortcuts" value={f()} onInput={(e) => setF(e.currentTarget.value)} />
      <For each={SHORTCUTS}>{([group, rows]) => (
        <section class="sc-group">
          <div class="eyebrow">{group}</div>
          <For each={rows.filter(([t]) => t.toLowerCase().includes(f().toLowerCase()))}>{([t, k]) => (
            <div class="kv"><span>{t}</span><span class="kbd">{k}</span></div>
          )}</For>
        </section>
      )}</For>
    </Dialog>
  );
}

function Drafts() {
  const drafts = () => catalog.rows.filter((s) => s.draft);
  const [i, setI] = createSignal(0);
  return (
    <Dialog id="drafts" title="Saved drafts" width="720px" icon="pencil"
      footer={<><button class="btn">Copy text</button><button class="btn">Restore in current session</button><span class="spacer" />
        <button class="btn primary" onClick={() => { const d = drafts()[i()]; if (d) { setSelectedKey(d.key); setPage("sessions"); } setDialog(undefined); }}>Go to session</button></>}>
      <p class="hint" style={{ "margin-bottom": "8px" }}>Drafts stay on this desktop, including those from ended or unavailable sessions. Restoring never sends a message.</p>
      <div class="draft-list">
        <For each={drafts()}>{(d, k) => (
          <div class="draft-item" classList={{ on: k() === i() }} onClick={() => setI(k())}>
            <div style={{ position: "relative", height: "104px" }}><SessionCard session={{ ...d, status: "ready", unread: false }} selected={k() === i()} expandable={false} expanded={false} onToggleChildren={() => undefined} /></div>
          </div>
        )}</For>
      </div>
      <pre class="act-code mono" style={{ "margin-top": "8px" }}>Also check the tab order inside the date picker and…</pre>
    </Dialog>
  );
}

function Rename() {
  const s = () => selected();
  const [v, setV] = createSignal("");
  return (
    <Dialog id="rename" title="Rename session" width="480px" icon="pencil"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" onClick={() => { toast("Renamed", `Now ${s()?.agent}/${s()?.project}/${v() || s()?.name}`); setDialog(undefined); }}>Rename</button></>}>
      <div class="hint">{s()?.machine} / {s()?.agent}/{s()?.project}</div>
      <div class="form-row"><label for="rn">Session name</label><input id="rn" class="field" autofocus value={s()?.name} onInput={(e) => setV(e.currentTarget.value)} /></div>
      <div class="hint">Full name: <span class="mono">{s()?.agent}/{s()?.project}/{v() || s()?.name}</span>. No spaces at the ends and none of / : . characters.</div>
    </Dialog>
  );
}

function Fork() {
  const s = () => selected();
  return (
    <Dialog id="fork" title="Fork session" width="480px" icon="git-fork"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" onClick={() => { toast("Forked", "The fork is placed after its original and opens Terminal"); setDialog(undefined); }}>Fork</button></>}>
      <p class="muted">The fork continues this conversation in a new session. The original keeps running.</p>
      <div class="form-row"><label for="fk">New session name</label><input id="fk" class="field" autofocus value={`${s()?.name}-fork`} /></div>
    </Dialog>
  );
}

function ModelPicker() {
  const s = () => selected();
  return (
    <Dialog id="model" title="Model and effort" width="420px" icon="sparkles"
      footer={<><button class="btn ghost">Open Terminal</button><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" onClick={() => { toast("Saved", "Applies when the agent is ready"); setDialog(undefined); }}>Apply</button></>}>
      <div class="form-row"><label>Model</label><select class="field"><option>{s()?.model}</option><option>claude-sonnet-4-6</option><option>claude-haiku-4-6</option></select></div>
      <div class="form-row"><label>Reasoning effort</label><select class="field"><option>high</option><option>medium</option><option>low</option></select></div>
      <div class="hint">Current: {s()?.model} {s()?.effort}. Applies to this session only.</div>
      <div class="notice warn" style={{ "margin-top": "10px" }}><Icon name="info" size={14} /><div>Claude re-reads the conversation with the new model on its next turn.</div></div>
    </Dialog>
  );
}

function ReferenceFile() {
  return (
    <Dialog id="reference-file" title="Reference an existing file" width="520px" icon="file-text"
      footer={<><span class="spacer" /><button class="btn" onClick={() => setDialog(undefined)}>Cancel</button><button class="btn primary" onClick={() => setDialog(undefined)}>Insert reference</button></>}>
      <p class="muted">Inserts a quoted path into your message without uploading the file.</p>
      <div class="form-row"><label>Path on {selected()?.machine}</label><input class="field" value="~/src/aurora/docs/pagination.md" /></div>
      <div class="hint">Absolute, ~/ or C:\ paths on that machine.</div>
    </Dialog>
  );
}

function MoveProject() {
  return (
    <Dialog id="move-project" title="Move to project" width="380px" icon="folder">
      <div class="col" style={{ gap: "4px" }}>
        <For each={projects}>{(p) => (
          <button class="menu-item entity" style={{ "--entity": projectColor(p) }} onClick={() => { toast("Moved", `Session moved to ${p.name}. Its folder and process are unchanged.`); setDialog(undefined); }}>
            <Icon name="folder" size={14} />{p.name}<Show when={p.id === selected()?.project}><span class="grow" /><Icon name="check" size={14} /></Show>
          </button>
        )}</For>
        <div class="menu-sep" />
        <button class="menu-item"><Icon name="plus" size={14} />New project…</button>
      </div>
    </Dialog>
  );
}

export function SessionDialogs() {
  const s = () => selected();
  return (
    <>
      <GoTo />
      <Shortcuts />
      <Drafts />
      <Rename />
      <Fork />
      <ModelPicker />
      <ReferenceFile />
      <MoveProject />
      <Confirm id="terminate" title="Terminate session?" danger confirm="Stop and archive"
        body={<>
          <p><b>{s()?.name}</b> on {s()?.machine} stops now. Its conversation is archived and can be restored later.</p>
          <label class="check" style={{ "margin-top": "10px" }}><input type="checkbox" /> Remove the working folder after stopping</label>
          <p class="hint" style={{ "margin-top": "6px" }}>Removal runs in the background after exact successful termination, only for a verified generated worktree. Unknown local data keeps the folder for review. Computer: {s()?.machine}</p>
        </>} />
      <Confirm id="clear-session" title="Clear session?" confirm="Clear session"
        body={<p>The agent stops seeing the activity above. Your draft is kept. <span class="muted">Activity stays visible here with a “Session cleared” boundary.</span></p>} />
      <Confirm id="forget" title="Forget saved session?" danger confirm="Forget saved session"
        body={<p>Zerus forgets <b>{s()?.name}</b>. The agent's own files and transcripts on {s()?.machine} are not deleted.</p>} />
    </>
  );
}

export { AgentGlyph };
