import { Show, createSignal, onMount } from "solid-js";
import { refreshUpdates, updateCommands, updates } from "../../../store/updates";
import { toast } from "../../../store/ui";
import type { UpdateChannel } from "../../../types";
import { Actions, Group, Lines, Row, SectionHead, Select, Status, ToggleRow } from "./ui";
import { lastCheckedText } from "./updates-time";

function failed(error: unknown) {
  toast("Updates", String(error), "error", true);
}

export function UpdatesSection() {
  const [details, setDetails] = createSignal(false);
  onMount(() => { void refreshUpdates(); });
  const open = (target: "release" | "download" | "instructions") => {
    void updateCommands.open(target).catch(failed);
  };
  return (
    <>
      <SectionHead title="Updates" />
      <Group title={updates().heading}>
        <div class="updates-card" data-available={updates().available ? "1" : "0"}>
          <Row label="Installed"><span class="selectable">{updates().installed}</span></Row>
          <Row label="Latest release"><span class="selectable">{updates().latest}</span></Row>
          <Show when={!updates().busy && updates().comparison}>
            <p class="set-group-hint selectable updates-comparison">{updates().comparison}</p>
          </Show>
          <p class="set-group-hint selectable">Last successful check: {lastCheckedText(updates().last_checked)}</p>
          <Show when={updates().cached}><p class="set-group-hint">Showing the last verified catalog.</p></Show>
          <Show when={updates().error}><Status kind="error">{updates().error}</Status></Show>
          <Actions>
            <button type="button" class="btn" disabled={!updates().release_enabled} onClick={() => open("release")}>{updates().release_label}</button>
            <Show when={updates().download_label}>
              {(label) => <button type="button" class="btn" onClick={() => open("download")}>{label()}</button>}
            </Show>
          </Actions>
        </div>
      </Group>

      <Group title="Update checks">
        <ToggleRow label="Check for updates automatically" checked={updates().automatic}
          hint="Available updates appear in the tray and workspace, with a system notification."
          onChange={(on) => { void updateCommands.setAutomatic(on).catch(failed); }} />
        <Row label="Release channel" for="updates-channel">
          <Select id="updates-channel" label="Release channel" value={updates().channel}
            options={[{ value: "stable", label: "Stable" }, { value: "nightly", label: "Nightly" }, { value: "dev", label: "Dev" }]}
            onChange={(channel) => { void updateCommands.setChannel(channel as UpdateChannel).catch(failed); }} />
          <button type="button" class="btn" disabled={updates().busy} onClick={() => { void updateCommands.check().catch(failed); }}>Check now</button>
        </Row>
      </Group>

      <Group>
        <Actions>
          <button type="button" class="btn" aria-expanded={details()} aria-controls="updates-installation-details"
            onClick={() => setDetails(!details())}>{details() ? "Hide installation details" : "Show installation details"}</button>
        </Actions>
        <Show when={details()}>
          <div id="updates-installation-details" class="updates-details">
            <Lines class="selectable" text={updates().details.ownership} />
            <Show when={updates().details.command}>
              <Lines class="selectable mono updates-command" text={updates().details.command} />
              <Actions>
                <button type="button" class="btn" aria-label={updates().details.copy_name}
                  onClick={() => { void updateCommands.copyCommand().catch(failed); }}>Copy command</button>
              </Actions>
            </Show>
            <Show when={updates().details.instructions}>
              <Lines class="selectable" text={updates().details.instructions} />
            </Show>
            <Actions>
              <button type="button" class="btn" onClick={() => open("instructions")}>Installation instructions</button>
            </Actions>
          </div>
        </Show>
      </Group>
    </>
  );
}
