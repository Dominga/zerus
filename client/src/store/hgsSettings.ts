import { createRoot, createSignal } from "solid-js";
import { hgsSetting } from "../bridge";
import type { HgsSettingName, HgsSettingOutcome } from "../types";

// Settings > Sessions' two hgs-owned toggles (SettingsPage.h:242-250): the
// value hgs confirmed, whether a call runs and hgs's own words when it did
// not. As in Qt, the box shows the confirmed value (the default until the
// first read) and a failed read or write changes nothing.

/** What the model needs: one `hgs <command> get|set --json`. */
export interface HgsSettingPort {
  call(call: { setting: HgsSettingName; value?: boolean; request_id?: string | null }): Promise<HgsSettingOutcome>;
}

export interface HgsSettingModel {
  name: HgsSettingName;
  /** The value hgs confirmed, or the default before the first read. */
  value: () => boolean;
  busy: () => boolean;
  /** hgs's own failure words, empty when all is well. */
  error: () => string;
  load: () => Promise<void>;
  set: (on: boolean) => Promise<void>;
}

const requestId = () => globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;

export function createHgsSetting(name: HgsSettingName, fallback: boolean, port: HgsSettingPort = { call: hgsSetting }): HgsSettingModel {
  const [value, setValue] = createSignal(fallback);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");

  async function ask(call: { setting: HgsSettingName; value?: boolean; request_id?: string | null }) {
    if (busy()) return;
    setBusy(true);
    try {
      const outcome = await port.call(call);
      if (outcome.outcome === "done") {
        setValue(outcome.view.value);
        setError("");
      } else {
        setError(outcome.detail);
      }
    } catch (reason) {
      setError(String(reason instanceof Error ? reason.message : reason));
    } finally {
      setBusy(false);
    }
  }

  return {
    name,
    value,
    busy,
    error,
    load: () => ask({ setting: name }),
    set: (on) => ask({ setting: name, value: on, request_id: requestId() }),
  };
}

/** The two toggles of Settings > Sessions, shared by the page and its tests. */
export const subagentsConfirm = createRoot(() => createHgsSetting("subagents-confirm", true));
export const sessionUploadBlock = createRoot(() => createHgsSetting("session-upload-block", true));
