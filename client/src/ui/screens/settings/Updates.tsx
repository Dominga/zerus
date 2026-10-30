import { Show, createEffect, createSignal, onMount } from "solid-js";
import { refreshUpdates, updateCommands, updates } from "../../../store/updates";
import { toast } from "../../../store/ui";
import type { UpdateChannel } from "../../../types";
import { Icon } from "../../lib/icons";
import { Actions, Group, Lines, Row, SectionHead, Select, Status, ToggleRow } from "./ui";
import { lastCheckedText } from "./updates-time";

function failed(error: unknown) {
  toast("Updates", String(error), "error", true);
}

function LoginItemDialog() {
  const [open, setOpen] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  let dialog!: HTMLDialogElement;
  createEffect(() => {
    if (open()) {
      if (!dialog.open) dialog.showModal();
      queueMicrotask(() => dialog.querySelector<HTMLElement>("[data-initial]")?.focus());
    } else if (dialog.open) dialog.close();
  });
  const fix = (restart: boolean) => {
    setBusy(true);
    void updateCommands.fixLoginItem(restart).then((restartError) => {
      setOpen(false);
      if (restartError) {
        toast("Login item", "The Zerus login item is updated; it applies at your next login.");
        toast("Fix the Zerus login item", `Zerus could not restart:\n${restartError}`, "error", true);
      } else if (restart) {
        toast("Restarting Zerus", "The login item is updated. Drafts are saved; agents keep running.");
      } else {
        toast("Login item", "The Zerus login item is updated; it applies at your next login.");
      }
    }).catch((error) => {
      toast("Fix the Zerus login item", `The login item was not changed.\n\n${String(error)}`, "error", true);
    }).finally(() => setBusy(false));
  };
  return (
    <>
      <Show when={updates().login_item_deprioritized}>
        <div class="notice warn upd-login">
          <Icon name="triangle-alert" size={14} />
          <span class="grow">macOS starts Zerus at background priority, so it can stop responding while the computer is busy.</span>
          <button type="button" class="btn small" onClick={() => setOpen(true)}>Fix login item</button>
        </div>
      </Show>
      <dialog id="login-item-dialog" class="dlg" ref={dialog} style={{ width: "520px" }} role="alertdialog" aria-labelledby="login-item-title" aria-describedby="login-item-text"
        onCancel={(event) => { event.preventDefault(); setOpen(false); }} onClose={() => setOpen(false)}>
        <div class="dlg-head"><Icon name="triangle-alert" size={18} class="warn" /><h2 id="login-item-title">Fix the Zerus login item</h2></div>
        <div class="dlg-body confirm-body" id="login-item-text">
          <p><b>macOS runs Zerus at background priority.</b></p>
          <p class="muted">Zerus was started by its login item, which macOS runs at background priority, so Zerus can stop responding while other programs keep the computer busy. Updating the login item makes macOS start Zerus like other apps.</p>
          <p class="muted">Update Only applies this at your next login; Zerus keeps running.<br />Update and Restart applies it now: Zerus quits and opens again. Saved drafts, sessions and agents stay; a voice or video message that is still recording stops and is not kept.</p>
        </div>
        <div class="dlg-foot">
          <button type="button" class="btn" disabled={busy()} onClick={() => setOpen(false)}>Cancel</button>
          <span class="spacer" />
          <button type="button" class="btn" disabled={busy()} onClick={() => fix(true)}>Update and Restart</button>
          <button type="button" class="btn primary" data-initial autofocus disabled={busy()} onClick={() => fix(false)}>Update Only</button>
        </div>
      </dialog>
    </>
  );
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

      <LoginItemDialog />

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
