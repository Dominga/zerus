// The provider editor's form (tray/src/speech/SpeechSettings.cpp
// ProfileEditor::validate), mirrored from the core (speech_settings.rs
// EditorForm::valid), which checks it again before anything is sent.

export interface AdapterView { id: string; label: string; models: string[]; devices: string[] }

export interface EditorForm {
  name: string;
  id: string;
  adapter: string;
  model: string;
  language: string;
  device: string;
  base_url: string;
  allow_insecure_http: boolean;
  /** keep, replace or remove. */
  credential: string;
  key: string;
}

const HEX = /^[0-9A-Fa-f]{2}$/;

/** A strict URL with a host and no user, query or fragment, over HTTPS or consented HTTP. */
export function endpointValid(url: string, insecure: boolean): boolean {
  const at = url.indexOf("://");
  if (at < 0) return false;
  const scheme = url.slice(0, at).toLowerCase();
  if (!(scheme === "https" || (scheme === "http" && insecure))) return false;
  if (/[\s\x00-\x1f\x7f<>\\^`{|}"?#]/.test(url)) return false;
  for (let i = url.indexOf("%"); i >= 0; i = url.indexOf("%", i + 1)) if (!HEX.test(url.slice(i + 1, i + 3))) return false;
  const authority = url.slice(at + 3).split("/")[0];
  if (authority.includes("@")) return false;
  let host: string, port: string | undefined;
  if (authority.startsWith("[")) {
    const close = authority.indexOf("]");
    const after = close < 0 ? "x" : authority.slice(close + 1);
    if (close < 0 || (after && !after.startsWith(":"))) return false;
    host = authority.slice(1, close);
    if (!host || !/^[0-9A-Fa-f:.]+$/.test(host)) return false;
    port = after ? after.slice(1) : undefined;
  } else {
    const colon = authority.indexOf(":");
    host = colon < 0 ? authority : authority.slice(0, colon);
    port = colon < 0 ? undefined : authority.slice(colon + 1);
    if (!host) return false;
  }
  return port === undefined || port === "" || (/^[0-9]+$/.test(port) && Number(port) <= 65535);
}

/** Whether Save may send the form, for the adapters the editor offered. */
export function formValid(form: EditorForm, adapters: AdapterView[]): boolean {
  const remote = form.adapter === "openai_http";
  const replace = remote && form.credential === "replace";
  const offered = adapters.find((adapter) => adapter.id === form.adapter);
  const target = remote
    ? endpointValid(form.base_url.trim(), form.allow_insecure_http)
    : !!offered && offered.models.includes(form.model) && offered.devices.includes(form.device);
  return [...form.name].length <= 128 && [...form.model].length <= 128 && [...form.base_url].length <= 2048 && [...form.key].length <= 8192
    && form.name.trim() !== ""
    && /^[A-Za-z0-9_-]{1,64}$/.test(form.id)
    && form.model.trim() !== ""
    && /^([a-z]{2})?$/.test(form.language)
    && form.adapter !== ""
    && ["keep", "replace", "remove"].includes(form.credential)
    && target
    && (!replace || form.key !== "");
}
