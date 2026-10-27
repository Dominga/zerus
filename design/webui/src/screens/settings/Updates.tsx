import { Show, createSignal } from "solid-js";
import { Icon } from "../../lib/icons";
import { demoState, setDialog, toast } from "../../state";
import { FEEDS, INSTALLED_DISPLAY, LAST_CHECK, type Channel } from "../../mock/settings-extra";
import { prefs, setPrefs, platform } from "./store";
import { SectionHead, Group, Check, Select, Actions, Lines } from "./ui";

// Settings → Updates (UpdatesWidget.h, UpdateController.*). There is no
// in-place self-update: the page explains how this installation is owned and
// links to the verified release or build.

interface Guide { title: string; comparison: string; ownership: string; command?: string; copyName?: string; instructions?: string; download?: string; available: boolean }

function guideFor(channel: Channel): Guide {
  const feed = FEEDS[channel];
  if (platform === "macos") {
    const cask = channel === "stable" ? "zerus" : `zerus@${channel}`;
    const command = channel === "dev" ? "brew upgrade --cask zerus@dev" : `brew uninstall --cask zerus@dev && brew install --cask ufna/zerus/${cask}`;
    return {
      title: channel === "dev" ? "Update available" : "Release status",
      available: channel === "dev",
      comparison: channel === "dev" ? `Update available. Run ${command}; the Homebrew tap may still be propagating.` : `This installation follows dev builds. To switch to ${channel}, run: ${command}`,
      ownership: "Homebrew cask: zerus@dev\nInstalled version: 0.48.0-dev.912",
      command: `Homebrew: ${command}`, copyName: "Copy Homebrew command",
      instructions: "Run this in Terminal. Homebrew replaces Zerus.app without quitting Zerus, agents or tmux; then choose Restart to finish update.",
      download: "Download DMG",
    };
  }
  if (platform === "windows") {
    return {
      title: channel === "dev" ? "Release status" : "Update available",
      available: channel !== "dev",
      comparison: channel === "dev" ? "Dev builds do not include a Windows build yet. Choose the stable or nightly channel to hear about newer Windows builds."
        : channel === "stable" ? `Zerus ${feed.version} is available. Download its Windows zip from the release page.`
        : `Zerus nightly ${feed.version} is available. Download its Windows zip from the release page.`,
      ownership: "Portable Windows build 0.47.2. Download newer builds from the release page.",
    };
  }
  // Linux: a local build in a Git checkout on an Arch-based system with yay.
  const pkg = channel === "nightly" ? "zerus-ade-nightly-bin" : "zerus-ade-bin";
  return {
    title: channel === "dev" ? "Update available" : "Release status",
    available: channel === "dev",
    comparison: channel === "dev" ? `Verified main build ${feed.buildNumber} is published. Use its source or platform package to update this installation.`
      : "Local build: release freshness is unconfirmed. Update or rebuild deliberately.",
    ownership: "Local build in a Git checkout; update and rebuild deliberately.\nSystem: Arch-based Linux",
    command: channel === "dev" ? undefined : `Optional AUR installation: yay -Syu ${pkg}`,
    copyName: "Copy optional installation command",
    instructions: channel === "dev" ? "No supported update command is available for this installation. See the installation instructions."
      : "This installs an AUR package; the current local build remains separate. Review the full system upgrade and your executable path.",
    download: channel === "dev" ? "Download dev package" : undefined,
  };
}

export function UpdatesSection() {
  const [busy, setBusy] = createSignal(false);
  const [details, setDetails] = createSignal(false);
  const checking = () => busy() || demoState() === "loading";
  const never = () => demoState() === "empty";
  const failed = () => demoState() === "error";
  const g = () => guideFor(prefs.channel);
  const feed = () => FEEDS[prefs.channel];
  const title = () => (checking() ? "Checking for updates…" : never() ? "Updates have not been checked" : g().title);
  const check = () => { setBusy(true); setTimeout(() => setBusy(false), 1300); };
  return (
    <>
      <SectionHead title="Updates" />
      <Group class="upd-status">
        <div class="upd-title">
          <Show when={checking()} fallback={<Icon name={never() ? "circle-question-mark" : g().available ? "arrow-up-to-line" : "check-check"} size={18} class={g().available && !never() ? "accent" : "muted"} />}>
            <span class="set-spinner" aria-hidden="true" />
          </Show>
          <h3>{title()}</h3>
        </div>
        <dl class="upd-versions">
          <dt>Installed</dt><dd class="selectable">{INSTALLED_DISPLAY}</dd>
          <dt>Latest release</dt><dd class="selectable">{never() ? "Not checked" : feed().version}</dd>
        </dl>
        <Show when={!checking()}>
          <p class="upd-comparison">{never() ? "No verified release catalog yet." : g().comparison}</p>
        </Show>
        <p class="muted">Last successful check: {never() ? "Never" : LAST_CHECK}</p>
        <Show when={failed()}>
          <p class="muted">Showing the last verified catalog.</p>
          <p class="danger">Check failed. Try again later.</p>
        </Show>
        <Actions>
          <button class="btn" disabled={never()} onClick={() => toast("Opening", feed().release)}>
            {prefs.channel === "dev" ? "View build" : "View release"}<Icon name="external-link" size={12} />
          </button>
          <Show when={g().download && !never()}>
            <button class="btn" onClick={() => toast("Download", `${g().download} from the verified catalog.`)}><Icon name="download" size={14} />{g().download}</button>
          </Show>
        </Actions>
      </Group>

      <Show when={platform === "macos"}>
        <div class="notice warn upd-login">
          <Icon name="triangle-alert" size={14} />
          <span class="grow">macOS starts Zerus at background priority, so it can stop responding while the computer is busy.</span>
          <button class="btn small" onClick={() => setDialog("settings-login-item")}>Fix login item</button>
        </div>
      </Show>

      <Group title="Update checks">
        <Check label="Check for updates automatically" checked={prefs.updatesAutomatic} onChange={(v) => setPrefs("updatesAutomatic", v)}
          hint="Available updates appear in the tray and workspace, with a system notification." />
        <div class="set-row">
          <div class="set-row-text"><label class="set-label" for="upd-channel">Release channel</label></div>
          <div class="set-row-control row">
            <Select id="upd-channel" label="Release channel" value={prefs.channel} onChange={(v) => setPrefs("channel", v as Channel)}
              options={[{ value: "stable", label: "Stable" }, { value: "nightly", label: "Nightly" }, { value: "dev", label: "Dev" }]} />
            <button class="btn" disabled={checking()} onClick={check}><Icon name="refresh-cw" size={13} />Check now</button>
          </div>
        </div>
      </Group>

      <div class="upd-install">
        <button class="btn ghost" aria-expanded={details()} onClick={() => setDetails(!details())}>
          <Icon name={details() ? "chevron-down" : "chevron-right"} size={14} />{details() ? "Hide installation details" : "Show installation details"}
        </button>
        <Show when={details()}>
          <Group class="upd-details">
            <Lines text={g().ownership} />
            <Show when={g().command}>
              <code class="upd-command selectable">{g().command}</code>
              <Actions><button class="btn small" aria-label={g().copyName} onClick={() => toast("Copied", g().command!.replace(/^[^:]+: /, ""))}><Icon name="copy" size={12} />Copy command</button></Actions>
            </Show>
            <Show when={g().instructions}><p class="muted">{g().instructions}</p></Show>
            <Actions><button class="btn small" onClick={() => toast("Opening", "Installation instructions on GitHub")}>Installation instructions<Icon name="external-link" size={12} /></button></Actions>
          </Group>
        </Show>
      </div>
    </>
  );
}
