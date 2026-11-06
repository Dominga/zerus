import { For, Show } from "solid-js";
import { flag, setSetting } from "../../../store/settings";
import { Group, SectionHead, ToggleRow } from "./ui";
import { tr, trNoop } from "../../../i18n";

// WhiteWh fork: Settings > Extensions (tray/src/SettingsPage.h 300-317,
// tray/src/Extensions.h). Each optional integration is off unless turned on
// here, and a fork build (this client) turns extensions on by default. The
// names are Qt's QSettings names, so the switch seeds the owner's choice from
// Qt; the folder checkboxes belong to the extension's own options.

interface Extension {
  id: string;
  name: string;
  description: string;
}

const EXTENSIONS: readonly Extension[] = [
  {
    id: "socraticode",
    name: "SocratiCode",
    description: trNoop("Semantic code search for agents (github.com/giancarloerra/socraticode). Details shows the index of a session's folder and installs, starts and connects SocratiCode. It needs an hgs built with the socraticode feature."),
  },
];

export function ExtensionsSection() {
  const enabled = (id: string) => flag(`extensions/${id}`) === true;
  return (
    <>
      <SectionHead title={trNoop("Extensions")}
        intro={trNoop("Integrations with optional third-party tools. They are not part of Zerus and stay off until you turn them on.")} />
      <For each={EXTENSIONS}>{(extension) => (
        <Group>
          <ToggleRow label={extension.name} hint={extension.description} checked={enabled(extension.id)}
            onChange={(on) => void setSetting(`extensions/${extension.id}`, on)} />
          <Show when={extension.id === "socraticode"}>
            <ToggleRow label={trNoop("Run SocratiCode on a native Qdrant (no Docker)")} checked={flag("socraticode/nativeQdrant") === true}
              disabled={!enabled(extension.id)} onChange={(on) => void setSetting("socraticode/nativeQdrant", on)}
              hint={trNoop("Off by default: SocratiCode keeps each folder's index in its own Qdrant container in Docker. On: Zerus uses a Qdrant program on 127.0.0.1:6333, which it can install and start, with Ollama for embeddings. Folders whose Claude Code settings name a Qdrant always use that one.")} />
          </Show>
        </Group>
      )}</For>
      <p class="set-foot">{tr("Details shows an enabled extension's panel for the selected session.")}</p>
    </>
  );
}
