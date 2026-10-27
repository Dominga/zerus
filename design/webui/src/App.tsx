import { Show, Switch, Match, For, createEffect, onMount, onCleanup, createSignal, on } from "solid-js";
import { Rail } from "./components/Rail";
import { MenuLayer, Toasts } from "./components/Overlays";
import { SessionList } from "./screens/sessions/SessionList";
import { SessionDetail } from "./screens/sessions/SessionDetail";
import { SessionDialogs } from "./dialogs/SessionDialogs";
import { SetupDialogs } from "./dialogs/SetupDialogs";
import { TaskDialogs } from "./dialogs/TaskDialogs";
import { SettingsDialogs } from "./dialogs/SettingsDialogs";
import { lazy, Suspense } from "solid-js";

// Pages other than Sessions load on first visit (route-level code splitting).
const DashboardScreen = lazy(() => import("./screens/Dashboard").then((m) => ({ default: m.DashboardScreen })));
const ProjectsScreen = lazy(() => import("./screens/Projects").then((m) => ({ default: m.ProjectsScreen })));
const MachinesScreen = lazy(() => import("./screens/Machines").then((m) => ({ default: m.MachinesScreen })));
const AccountsScreen = lazy(() => import("./screens/Accounts").then((m) => ({ default: m.AccountsScreen })));
const SettingsScreen = lazy(() => import("./screens/Settings").then((m) => ({ default: m.SettingsScreen })));
import { Icon } from "./lib/icons";
import { skinList, skin, setSkin, applySkin } from "./skin/runtime";
import { setSection } from "./screens/settings/store";
import {
  page, setPage, selected, setDialog, stress, setStressMode, demoState, setDemoState, setInspectorOpen, inspectorOpen, logoMotion, setLogoMotion, type LogoMotion,
  setContentScale, contentScale, toast, setDetailTab, detailTab,
} from "./state";
import { accounts } from "./mock/data";

const params = new URLSearchParams(location.search);

function StatusBar() {
  const acc = () => accounts.find((a) => a.id === selected()?.account);
  return (
    <footer class="statusbar">
      <Show when={demoState() === "error"} fallback={<span class="conn-ok"><span class="state-dot ready" />4 of 5 machines connected · build-02 offline</span>}>
        <span class="danger">build-02: SSH request timed out (retrying…); win-lab: connecting…</span>
        <button class="btn small ghost" onClick={() => toast("Retry", "Reconnecting now")}>Retry now</button>
      </Show>
      <Show when={params.get("update") === "ready"} fallback={<button class="chip-accent" onClick={() => { setSection("updates"); setPage("settings"); }}>New version</button>}>
        <button class="chip-accent" title="Drafts are saved; agents keep running" onClick={() => toast("Restarting", "Zerus restarts to finish the update")}>Restart to finish update</button>
      </Show>
      <Show when={params.get("platform") === "macos"}>
        <button class="chip-accent warn-chip" onClick={() => { setSection("updates"); setPage("settings"); }} title="macOS limits background priority for this login item">Fix login item</button>
      </Show>
      <span class="grow" />
      <Show when={acc() && acc()!.fiveHour > 0}>
        <button class="sb-usage" title="5-hour and weekly limits; at the current pace the limit runs out in about 3 h" onClick={() => setPage("accounts")}>
          <span>{acc()!.label}</span>
          <span class="meter" classList={{ warn: acc()!.fiveHour > 50 }}><i style={{ width: `${acc()!.fiveHour}%` }} /></span><span class="tabular">5h {acc()!.fiveHour}%</span>
          <span class="meter"><i style={{ width: `${acc()!.week}%` }} /></span><span class="tabular">7d {acc()!.week}%</span>
        </button>
        <button class="icon-btn small" title="Refresh usage"><Icon name="refresh-cw" size={12} /></button>
      </Show>
    </footer>
  );
}

/** Mockup-only review tools: skin, demo state, logo motion, 20k rows. Not part of the product UI. */
function ReviewTools() {
  const [open, setOpen] = createSignal(false);
  return (
    <>
      <button class="review-launch" classList={{ on: open() }} title="Mockup review tools (not part of the product)" onClick={() => setOpen(!open())}>MOCK<br />UP</button>
      <Show when={open()}>
        <div class="review-pop" role="dialog" aria-label="Mockup review tools">
          <span class="tag">MOCKUP</span>
          <label>Skin
            <select value={skin().id} onChange={(e) => setSkin(e.currentTarget.value)}>
              <For each={skinList()}>{(s) => <option value={s.id}>{s.name}</option>}</For>
            </select>
          </label>
          <label>Data
            <select value={demoState()} onChange={(e) => setDemoState(e.currentTarget.value as never)}>
              <option value="normal">Live data</option>
              <option value="loading">Loading</option>
              <option value="empty">Empty fleet</option>
              <option value="error">Offline / errors</option>
            </select>
          </label>
          <label>Zerus mark motion (while sessions need attention)
            <select value={logoMotion()} onChange={(e) => setLogoMotion(e.currentTarget.value as LogoMotion)}>
              <option value="qt">As in Qt: one turn, pause</option>
              <option value="blades">Blades: staggered spring + glow</option>
              <option value="ripple">Ripple: calm turn + rings</option>
            </select>
          </label>
          <div class="rp-row">
            <button class="rp-btn" classList={{ on: stress() }} onClick={() => { setStressMode(!stress()); toast(stress() ? "20 000 sessions" : "Demo fleet", stress() ? "Virtualized list: only visible rows are in the DOM" : "Back to the demo fleet"); }}>20 000 sessions</button>
            <a href="./index.html">Review page ↗</a>
          </div>
        </div>
      </Show>
    </>
  );
}

export function App() {
  createEffect(() => applySkin(skin()));
  // L2 layout variant: the skin chooses whether the side panel starts docked.
  createEffect(on(() => skin().layout.inspector, (mode) => {
    if (!params.has("inspector")) setInspectorOpen(mode === "docked");
  }));
  const keys = (e: KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "k") { e.preventDefault(); setDialog("goto"); }
    else if (mod && e.key.toLowerCase() === "n") { e.preventDefault(); setDialog("new-session"); }
    else if (mod && e.key.toLowerCase() === "i") { e.preventDefault(); setInspectorOpen(!inspectorOpen()); }
    else if (mod && e.key === ",") { e.preventDefault(); setPage("settings"); }
    else if (mod && /^[1-5]$/.test(e.key)) { e.preventDefault(); setPage((["dashboard", "sessions", "projects", "machines", "accounts"] as const)[Number(e.key) - 1]); }
    else if (mod && e.key === "`") { e.preventDefault(); setDetailTab(detailTab() === "terminal" ? "activity" : "terminal"); }
    else if (mod && (e.key === "=" || e.key === "+")) { e.preventDefault(); setContentScale(Math.min(2, +(contentScale() + 0.1).toFixed(2))); toast("Content scale", `${Math.round(contentScale() * 100)}%`); }
    else if (mod && e.key === "-") { e.preventDefault(); setContentScale(Math.max(0.75, +(contentScale() - 0.1).toFixed(2))); toast("Content scale", `${Math.round(contentScale() * 100)}%`); }
    else if (mod && e.key === "0") { e.preventDefault(); setContentScale(1); }
    else if (e.key === "F1" || (mod && e.key === "/")) { e.preventDefault(); setDialog("shortcuts"); }
  };
  onMount(() => addEventListener("keydown", keys));
  onCleanup(() => removeEventListener("keydown", keys));
  return (
    <div class="app" classList={{ "rail-labeled": skin().layout.rail === "labeled" }}>
      <Rail />
      <main class="page">
        <Suspense>
        <Switch>
          <Match when={page() === "sessions"}>
            <div class="sessions-page" classList={{ "layout-right": skin().layout["sessions-pane"] === "right" }}>
              <SessionList />
              <SessionDetail />
            </div>
          </Match>
          <Match when={page() === "dashboard"}><DashboardScreen /></Match>
          <Match when={page() === "projects"}><ProjectsScreen /></Match>
          <Match when={page() === "machines"}><MachinesScreen /></Match>
          <Match when={page() === "accounts"}><AccountsScreen /></Match>
          <Match when={page() === "settings"}><SettingsScreen /></Match>
        </Switch>
        </Suspense>
      </main>
      <StatusBar />
      <MenuLayer />
      <Toasts />
      <SessionDialogs />
      <SetupDialogs />
      <TaskDialogs />
      <SettingsDialogs />
      <ReviewTools />
    </div>
  );
}
