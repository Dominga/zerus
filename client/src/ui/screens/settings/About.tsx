import { createSignal, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../../bridge";
import { Icon } from "../../lib/icons";
import { SectionHead } from "./ui";

// Settings > About (tray/src/SettingsPage.h:146-155): the installed Zerus,
// the hgs CLI, checked whenever the section opens, and the repository.

const [cli, setCli] = createSignal("hgs CLI version unavailable");
// HgsClient::requestVersion: one `hgs --version` at a time.
let checking = false;

/** Qt's refreshCliVersion: "checking" until `hgs --version` answers. */
export function refreshAbout() {
  if (!inTauri()) return;
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
  return (
    <>
      <SectionHead title="About" />
      <p class="selectable">{`Zerus ${__APP_VERSION__}`}</p>
      <p class="selectable">{cli()}</p>
      <div>
        <button class="btn" title="https://github.com/ufna/zerus" aria-label="Open Zerus repository on GitHub"
          onClick={() => { if (inTauri()) void invoke("open_repository").catch(() => {}); }}>
          <Icon name="github" size={16} />ufna/zerus
        </button>
      </div>
    </>
  );
}
