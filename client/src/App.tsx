import { For, Match, Show, Switch, createEffect, createMemo, onCleanup, onMount } from "solid-js";
import { Rail } from "./ui/components/Rail";
import { MenuLayer, Toasts } from "./ui/components/Overlays";
import { ActionDialogs, ActionNotices } from "./ui/sessions/ActionDialogs";
import { SessionList } from "./ui/sessions/SessionList";
import { SessionDetail } from "./ui/sessions/SessionDetail";
import { Icon } from "./ui/lib/icons";
import { PerfHost } from "./ui/PerfHost";
import { OverviewScreen } from "./ui/screens/Overview";
import { MachinesScreen } from "./ui/screens/Machines";
import { SettingsScreen } from "./ui/components/SettingsScreen";
import { applySkin, skin } from "./skin/runtime";
import { catalog } from "./store/catalog";
import { contentScale, page, setContentScale, setPage, toast, type Page } from "./store/ui";
import type { BridgeMode } from "./bridge";

// The first screen of the client: the rail, the live session list, the
// selected session's Activity and the status bar. Other screens keep their
// place in the rail and move in with later phases.

const SCREEN: Record<Exclude<Page, "sessions">, string> = {
  dashboard: "Overview",
  projects: "Projects",
  machines: "Machines",
  accounts: "Accounts",
  settings: "Settings",
};

function LaterScreen(props: { page: Exclude<Page, "sessions"> }) {
  return (
    <div class="empty later-screen">
      <Icon name="sparkles" size={30} />
      <h3>{SCREEN[props.page]}</h3>
      <p>This screen moves to the new client in a later phase. Sessions is live now.</p>
      <button class="btn small" onClick={() => setPage("sessions")}>Back to Sessions</button>
    </div>
  );
}

const STATE_TEXT: Record<string, string> = {
  probing: "checking",
  unsupported: "no event stream (update hgs)",
  reconnecting: "reconnecting",
  ended: "stream ended",
};

function StatusBar(props: { mode: () => BridgeMode | "starting" }) {
  const computers = createMemo(() => Object.values(catalog.computers));
  const live = () => computers().filter((c) => c.status === "live").length;
  const problems = () => computers().filter((c) => c.status !== "live");
  const label = (id: string) => id || "this computer";
  return (
    <footer class="statusbar">
      <Show when={computers().length} fallback={<span>{props.mode() === "starting" ? "Starting…" : "Connecting…"}</span>}>
        <Show when={problems().length} fallback={<span class="conn-ok"><span class="state-dot ready" />{computers().length === 1 ? "Connected" : `${live()} of ${computers().length} machines connected`}</span>}>
          <span classList={{ danger: problems().some((c) => c.status !== "probing") }}>
            <For each={problems()}>{(c, i) => <>{i() ? "; " : ""}{label(c.id)}: {STATE_TEXT[c.status] ?? c.status}{c.status === "reconnecting" && c.attempt ? ` (attempt ${c.attempt})` : ""}</>}</For>
          </span>
        </Show>
      </Show>
      <Show when={props.mode() === "replay"}>
        <span class="chip-accent" title="Synthetic data from the replay fixture; no hgs is running">Replay</span>
      </Show>
      <span class="grow" />
    </footer>
  );
}

export function App(props: { mode: () => BridgeMode | "starting" }) {
  createEffect(() => applySkin(skin()));
  const keys = (e: KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod) return;
    if (/^[1-5]$/.test(e.key)) { e.preventDefault(); setPage((["dashboard", "sessions", "projects", "machines", "accounts"] as const)[Number(e.key) - 1]); }
    else if (e.key === ",") { e.preventDefault(); setPage("settings"); }
    else if (e.key === "=" || e.key === "+") { e.preventDefault(); setContentScale(Math.min(2, +(contentScale() + 0.1).toFixed(2))); toast("Content scale", `${Math.round(contentScale() * 100)}%`); }
    else if (e.key === "-") { e.preventDefault(); setContentScale(Math.max(0.75, +(contentScale() - 0.1).toFixed(2))); toast("Content scale", `${Math.round(contentScale() * 100)}%`); }
    else if (e.key === "0") { e.preventDefault(); setContentScale(1); }
  };
  onMount(() => addEventListener("keydown", keys));
  onCleanup(() => removeEventListener("keydown", keys));
  return (
    <div class="app" classList={{ "rail-labeled": skin().layout.rail === "labeled" }}>
      <Rail />
      <main class="page">
        <Switch>
          <Match when={page() === "sessions"}>
            <div class="sessions-page" classList={{ "layout-right": skin().layout["sessions-pane"] === "right" }}>
              <SessionList />
              <SessionDetail />
            </div>
          </Match>
          <Match when={page() === "dashboard"}><OverviewScreen /></Match>
          <Match when={page() === "machines"}><MachinesScreen /></Match>
          <Match when={page() === "settings"}><SettingsScreen /></Match>
          <Match when={page() !== "sessions"}><LaterScreen page={page() as Exclude<Page, "sessions">} /></Match>
        </Switch>
      </main>
      <StatusBar mode={props.mode} />
      <MenuLayer />
      <Toasts />
      <ActionDialogs />
      <ActionNotices />
      <PerfHost />
    </div>
  );
}
