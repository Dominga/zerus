import { Show, createSignal, onMount, onCleanup } from "solid-js";
import { Icon } from "../../lib/icons";
import { setDialog, toast, demoState } from "../../state";
import { DOWNLOAD_FOLDER, DOWNLOAD_USAGE, HGS_VERSION, INSTALLED_VERSION } from "../../mock/settings-extra";
import { prefs, setPrefs, platform, dialogCtx, setDialogCtx } from "./store";
import { SectionHead, Group, Row, ToggleRow, NumberField, Select, Actions, Status, Spinner } from "./ui";

// Settings → Sessions (SettingsPage.h:97-107)
export function SessionsSection() {
  return (
    <>
      <SectionHead title="Sessions" />
      <Group>
        <ToggleRow label="Expand the collapsed session list on hover" checked={prefs.expandOnHover} onChange={(v) => setPrefs("expandOnHover", v)} />
        <ToggleRow label="Hide projects without active sessions" checked={prefs.hideEmptyProjects} onChange={(v) => setPrefs("hideEmptyProjects", v)} />
        <ToggleRow label="Animate working sessions" checked={prefs.animateActivity} onChange={(v) => setPrefs("animateActivity", v)} />
        <ToggleRow label="Show completed subagents" checked={prefs.showCompletedSubagents} onChange={(v) => setPrefs("showCompletedSubagents", v)}
          hint="Include agents whose latest turn has finished in the session list and its counts. Plan & agents always shows all reported agents. Agents with unknown status remain visible." />
        <ToggleRow label="Notify about optional questions" checked={prefs.optionalQuestions} onChange={(v) => setPrefs("optionalQuestions", v)}
          hint="Off by default. Shows a system notification once for each new optional question, which the agent keeps working without. It is withdrawn when the questions are answered or skipped, or the session is marked as read. Questions that need an answer always notify." />
      </Group>
      <p class="set-foot">Closing Zerus keeps agents running. Projects and the arrangement of your workspace are saved on this device.</p>
    </>
  );
}

// Settings → Processes (SettingsPage.h:109-121, ProcessSettings.h)
export function ProcessesSection() {
  const off = () => !prefs.processesEnabled;
  const timing = (label: string, key: "procActive" | "procBackground" | "procUnknown" | "procLoading", min: number, max: number) => (
    <Row label={label} disabled={off()}>
      <NumberField value={prefs[key]} min={min} max={max} step={0.5} decimals={1} suffix="s" label={label} disabled={off()} onChange={(v) => setPrefs(key, v)} />
    </Row>
  );
  return (
    <>
      <SectionHead title="Processes" />
      <Group>
        <ToggleRow label="Enable Processes tab" checked={prefs.processesEnabled} onChange={(v) => setPrefs("processesEnabled", v)} />
      </Group>
      <Group title="Refresh timings">
        {timing("Open Processes tab", "procActive", 1, 60)}
        {timing("Background process count", "procBackground", 5, 600)}
        {timing("Unconfirmed process output", "procUnknown", 1, 600)}
        {timing("Show loading indicator after", "procLoading", 0, 5)}
      </Group>
      <p class="set-foot">Off by default. When disabled, the tab is hidden and processes are not polled. Enable it to inspect running shells and background commands, read recorded output and stop selected processes. Only the selected session is inspected; opening Processes refreshes immediately. Output is read only while the tab is visible. Timings apply to this Zerus. Background refresh also controls session inspection while Terminal or Native UI is open; session overview and account limits refresh separately.</p>
    </>
  );
}

// Settings → Files (DownloadCacheSettings.cpp)
export function FilesSection() {
  const [mode, setMode] = createSignal(prefs.cacheLimitGiB > 0 ? "limit" : "never");
  const [checking, setChecking] = createSignal(true);
  onMount(() => { const t = setTimeout(() => setChecking(false), 700); onCleanup(() => clearTimeout(t)); });
  const draftBytes = () => (mode() === "limit" ? dialogCtx.cacheDraftGiB : 0);
  const changed = () => draftBytes() !== prefs.cacheLimitGiB;
  const apply = () => {
    if (mode() === "limit" && prefs.cacheLimitGiB === 0) { setDialog("settings-cache-limit"); return; }
    setPrefs("cacheLimitGiB", draftBytes());
    toast("Files", mode() === "limit" ? `Downloaded copies are limited to ${dialogCtx.cacheDraftGiB.toFixed(1)} GiB.` : "Downloaded copies are never cleared.");
  };
  return (
    <>
      <SectionHead title="Files" intro="Download and open reuses a verified saved copy from the same machine and path. Show file selects it in your file manager. Download again gets a fresh copy; Download… lets you choose where to save. Existing files are preserved." />
      <Group>
        <Row label="Downloaded files" for="set-dl-folder">
          <input id="set-dl-folder" class="field mono set-path" readOnly value={DOWNLOAD_FOLDER[platform]} />
        </Row>
        <Row label="Keep copies" for="set-dl-mode">
          <Select id="set-dl-mode" value={mode()} onChange={(v) => { setMode(v); if (v === "limit" && dialogCtx.cacheDraftGiB <= 0) setDialogCtx("cacheDraftGiB", 1); }}
            options={[{ value: "never", label: "Never clear" }, { value: "limit", label: "Limit size" }]} />
        </Row>
        <Row label="Maximum size" disabled={mode() !== "limit"}>
          <NumberField value={dialogCtx.cacheDraftGiB} min={0.1} max={1024} step={0.1} decimals={1} suffix="GiB" label="Maximum size" disabled={mode() !== "limit"}
            onChange={(v) => setDialogCtx("cacheDraftGiB", v)} />
        </Row>
        <p class="set-group-hint">Never clear is the default. With a limit, Zerus removes its least recently used downloaded copies first. Files you add or edit are kept. Downloads in progress and copies used in the last minute are protected, so usage can briefly exceed the limit. This setting applies to this computer.</p>
        <Actions>
          <button class="btn primary" disabled={!changed()} onClick={apply}>Apply</button>
          <Show when={!checking()} fallback={<Status kind="busy">Checking downloaded files…</Status>}>
            <Status>{DOWNLOAD_USAGE}</Status>
          </Show>
        </Actions>
      </Group>
    </>
  );
}

// Settings → About (SettingsPage.h:145-155)
export function AboutSection() {
  const [cli, setCli] = createSignal("hgs CLI: checking…");
  onMount(() => {
    const t = setTimeout(() => setCli(demoState() === "error" ? "hgs CLI version unavailable" : `hgs CLI ${HGS_VERSION}`), 650);
    onCleanup(() => clearTimeout(t));
  });
  return (
    <>
      <SectionHead title="About" />
      <Group class="about">
        <div class="about-head">
          <span class="about-mark"><Icon name="sparkles" size={20} /></span>
          <div class="col" style={{ gap: "2px" }}>
            <span class="about-version selectable">Zerus {INSTALLED_VERSION}</span>
            <span class="muted selectable row" style={{ gap: "6px" }}>
              <Show when={cli().endsWith("…")}><Spinner size={12} /></Show>{cli()}
            </span>
          </div>
        </div>
        <Actions>
          <button class="btn" title="https://github.com/ufna/zerus" aria-label="Open Zerus repository on GitHub"
            onClick={() => toast("Opening GitHub", "https://github.com/ufna/zerus")}>
            <Icon name="git-fork" size={16} />ufna/zerus<Icon name="external-link" size={12} class="dim" />
          </button>
        </Actions>
      </Group>
    </>
  );
}
