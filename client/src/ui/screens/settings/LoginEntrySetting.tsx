import { Show, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { Group, Status, ToggleRow } from "./ui";
import { createLoginEntry, loginEntryHint, type LoginEntry } from "./login-entry";
import { trNoop } from "../../../i18n";

export const LOGIN_ENTRY_LABEL = trNoop("Start Zerus at login");

/** Windows only: the native side reports it unavailable everywhere else. */
export default function LoginEntrySetting() {
  const entry = createLoginEntry({
    read: () => invoke<LoginEntry>("login_entry_view"),
    write: (enabled) => invoke<LoginEntry>("login_entry_set", { enabled }),
  });
  onMount(() => void entry.load());
  return (
    <Show when={entry.available()}>
      <Group title={trNoop("Startup")}>
        <ToggleRow label={LOGIN_ENTRY_LABEL} hint={loginEntryHint(entry.state())} checked={entry.checked()} disabled={entry.disabled()} onChange={(on) => void entry.choose(on)} />
        <Show when={entry.error()}>{(text) => <Status kind="error">{text()}</Status>}</Show>
      </Group>
    </Show>
  );
}
