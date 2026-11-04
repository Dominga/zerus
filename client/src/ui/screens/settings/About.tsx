import { DesktopServiceStatus } from "../../components/DesktopServiceStatus";
import { MigrationReportDialog } from "../../components/MigrationReportDialog";
import { Show, createSignal, onMount } from "solid-js";
import { desktopService, checkDesktopServiceStatus } from "../../../store/desktopService";
import { toast } from "../../../store/ui";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../../bridge";
import { Icon } from "../../lib/icons";
import { SectionHead } from "./ui";
import { ServiceModeReview } from "./ServiceModeReview";
import { installedVersion, type BuildInfo } from "./about-version";
import { tr, trNoop } from "../../../i18n";

// Settings > About (tray/src/SettingsPage.h:146-155): the installed Zerus
// as UpdateController::installedVersion names it, the hgs CLI, checked
// whenever the section opens, and the repository.

const [zerus, setZerus] = createSignal(`Zerus ${__APP_VERSION__}`);
// The build's identity does not change while it runs: asked for once.
let identified = false;
const [cli, setCli] = createSignal("hgs CLI version unavailable");
// HgsClient::requestVersion: one `hgs --version` at a time.
let checking = false;

/** Qt's refreshCliVersion: "checking" until `hgs --version` answers. */
export function refreshAbout() {
  if (!inTauri()) return;
  if (!identified) {
    identified = true;
    invoke<BuildInfo>("build_info")
      .then((info) => { const version = installedVersion(info); if (version) setZerus(`Zerus ${version}`); })
      .catch(() => { identified = false; });
  }
  setCli("hgs CLI: checking…");
  if (checking) return;
  checking = true;
  invoke<string | null>("hgs_version")
    .then((version) => setCli(version ? `hgs CLI ${version}` : "hgs CLI version unavailable"))
    .catch(() => setCli("hgs CLI version unavailable"))
    .finally(() => { checking = false; });
}

export function AboutSection() {
  onMount(refreshAbout);
  // The same read-only Migration report the service status offers, in every
  // mode: without the background service it is otherwise unreachable.
  const [reportOrigin, setReportOrigin] = createSignal<HTMLElement>();
  return (
    <>
      <SectionHead title={trNoop("About")} />
      <p class="selectable">{zerus()}</p>
      <p class="selectable">{cli()}</p>
      <DesktopServiceStatus counts />
      <ServiceModeReview />
      <Show when={desktopService.state()?.executable_upgrade?.state === "manual_refresh_required"}>
        <section aria-label={tr("Service refresh required")}>
          <h2>{tr("Service refresh required")}</h2>
          <p>{tr("The CLI was updated. The Windows background service needs a manual refresh.")}</p>
          <p>{tr("Check the service registration from a foreground Command Prompt outside Zerus, using the same CLI and configuration:")}</p>
          <p class="selectable"><code>hgs desktop registration status --json</code></p>
          <p>{tr("Keep the selected service mode and any original pending request. Refresh controls are not available in this build.")}</p>
          <p>{tr("Check service status only reconnects. It does not refresh the service or resume background access.")}</p>
          <button type="button" class="btn" disabled={!desktopService.state()?.epoch} onClick={() => {
            const epoch = desktopService.state()?.epoch;
            if (epoch) void checkDesktopServiceStatus(epoch).catch(error => toast(tr("Background service"), String(error), "error", true));
          }}>{tr("Check service status")}</button>
        </section>
      </Show>
      <div>
        <button type="button" class="btn" onClick={(event) => setReportOrigin(event.currentTarget)}>
          <Icon name="file-text" size={16} />{tr("Migration report…")}
        </button>
      </div>
      <div>
        <button class="btn" title="https://github.com/ufna/zerus" aria-label={tr("Open Zerus repository on GitHub")}
          onClick={() => { if (inTauri()) void invoke("open_repository").catch(() => {}); }}>
          <Icon name="github" size={16} />ufna/zerus
        </button>
      </div>
      <Show when={reportOrigin()}>{(origin) => <MigrationReportDialog origin={origin()} close={() => setReportOrigin(undefined)} />}</Show>
    </>
  );
}
