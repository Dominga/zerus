import { createSignal } from "solid-js";
import { tr } from "../../i18n";
import { trText, type Text } from "../../text";
import type { WslFields, WslFilePreview, WslMapped, WslOpened, WslScope, WslTarget } from "../../bridge";
import type { Session } from "../sessions/types";

// Task 73: the state of Map Windows path… and Open mapped Windows original…
// (Qt WslPathMappingDialog and WslFileOpenDialog). client/core does every
// check through the Tauri commands; these keep the fields, the busy state,
// which preview the buttons may act on, and the notice Qt shows.

export const emptyFields = (): WslFields => ({ windows_root: "", mount_root: "", workspace_id: "" });
const complete = (f: WslFields) => !!f.windows_root && !!f.mount_root && !!f.workspace_id;
const same = (a: WslFields, b: WslFields) => a.windows_root === b.windows_root && a.mount_root === b.mount_root && a.workspace_id === b.workspace_id;

/** A core Text error, an Error or anything else, as words. */
export function problem(error: unknown): string {
  if (error && typeof error === "object" && "source" in error && typeof (error as Text).source === "string") return trText(error as Text);
  return error instanceof Error ? error.message : String(error);
}

/** Map Windows path…: the folder picker's explicit WSL branch. */
export interface MappingPort {
  open(alias: string): Promise<WslOpened>;
  preview(handle: string, source: string, fields: WslFields): Promise<WslMapped>;
  use(handle: string, source: string, fields: WslFields, remember: boolean): Promise<string>;
  close(handle: string): Promise<void>;
}

export function createMappingDialog(port: MappingPort) {
  const [target, setTarget] = createSignal<WslTarget>();
  const [source, setSourceSignal] = createSignal("");
  const [fields, setFieldsSignal] = createSignal<WslFields>(emptyFields());
  const [remember, setRemember] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [mapped, setMapped] = createSignal<WslMapped>();
  const [notice, setNotice] = createSignal(tr("Enter every mapping field, then preview the exact target path."));
  let handle = "";
  let previewed: { source: string; fields: WslFields } | undefined;
  const invalidate = () => { setMapped(undefined); previewed = undefined; setNotice(tr("The selection changed. Preview its exact mapping before using it.")); };
  return {
    target, source, fields, remember, busy, mapped, notice, setRemember,
    canPreview: () => !busy() && !!target() && !!source() && complete(fields()),
    canUse: () => !busy() && !!mapped(),
    /** Reads the connection; a refusal (not a local WSL connection) is the error. */
    async open(alias: string) {
      setBusy(true);
      try {
        const opened = await port.open(alias);
        handle = opened.handle; setTarget(opened.target); setFieldsSignal({ ...opened.target.saved });
      } finally { setBusy(false); }
    },
    setSource(value: string) { if (value === source()) return; setSourceSignal(value); invalidate(); },
    setField(name: keyof WslFields, value: string) { if (fields()[name] === value) return; setFieldsSignal({ ...fields(), [name]: value }); invalidate(); },
    async preview() {
      if (!handle || busy()) return;
      const sent = { source: source(), fields: { ...fields() } };
      setMapped(undefined); previewed = undefined; setBusy(true); setNotice(tr("Validating the explicit mapping…"));
      try {
        const result = await port.preview(handle, sent.source, sent.fields);
        if (sent.source !== source() || !same(sent.fields, fields())) return;
        previewed = sent; setMapped(result);
        setNotice(tr("Windows: %1\nWSL: %2\nCanonical identity: %3\nThe folder browser will verify the native directory before it can be selected.", result.windows, result.wsl, result.canonical));
      } catch (error) { setNotice(problem(error)); }
      finally { setBusy(false); }
    },
    /** Use mapped path: the WSL folder to browse, or undefined when refused. */
    async use(): Promise<string | undefined> {
      if (!handle || busy() || !previewed) return undefined;
      setBusy(true); setNotice(tr("Rechecking the selected WSL connection…"));
      try { const path = await port.use(handle, previewed.source, previewed.fields, remember()); handle = ""; return path; }
      catch (error) { setMapped(undefined); previewed = undefined; setNotice(problem(error)); return undefined; }
      finally { setBusy(false); }
    },
    /** Cancel waits for a running request, as Qt's reject does. */
    close() { if (busy()) return false; if (handle) void port.close(handle).catch(() => {}); handle = ""; return true; },
  };
}
export type MappingDialogState = ReturnType<typeof createMappingDialog>;

/** Open mapped Windows original…: a session file's explicit action. */
export interface FilePort {
  open(alias: string, scope: WslScope): Promise<WslOpened>;
  preview(handle: string, fields: WslFields): Promise<WslFilePreview>;
  openOriginal(handle: string, fields: WslFields, remember: boolean): Promise<Text>;
  close(handle: string): Promise<void>;
}

export function createFileDialog(port: FilePort) {
  const [target, setTarget] = createSignal<WslTarget>();
  const [fields, setFieldsSignal] = createSignal<WslFields>(emptyFields());
  const [remember, setRemember] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [verified, setVerified] = createSignal<WslFilePreview>();
  const [notice, setNotice] = createSignal(tr("Reading the selected local WSL connection…"));
  const idle = () => tr("Preview checks access and contents without opening anything. No original file or downloaded-file marker is modified.");
  let handle = "";
  let previewed: WslFields | undefined;
  return {
    target, fields, remember, busy, verified, notice, setRemember,
    loaded: () => !!target(),
    canPreview: () => !busy() && !!target() && complete(fields()),
    canOpen: () => !busy() && !!verified(),
    async open(alias: string, scope: WslScope) {
      setBusy(true);
      try {
        const opened = await port.open(alias, scope);
        handle = opened.handle; setTarget(opened.target); setFieldsSignal({ ...opened.target.saved }); setNotice(idle());
      } catch (error) { setNotice(problem(error)); }
      finally { setBusy(false); }
    },
    setField(name: keyof WslFields, value: string) {
      if (fields()[name] === value) return;
      setFieldsSignal({ ...fields(), [name]: value }); setVerified(undefined); previewed = undefined; setNotice(idle());
    },
    async preview() {
      if (!handle || busy()) return;
      const sent = { ...fields() };
      setVerified(undefined); previewed = undefined; setBusy(true);
      setNotice(tr("Reading the file through its original conversation in the selected WSL environment…"));
      try {
        const result = await port.preview(handle, sent);
        if (!same(sent, fields())) return;
        previewed = sent; setVerified(result);
        setNotice(tr("WSL: %1\nWindows original: %2\nSHA-256: %3\nAccess and contents match. Opening rechecks them; runnable files are shown, not executed.", result.wsl, result.windows, result.sha256));
      } catch (error) { setNotice(problem(error)); }
      finally { setBusy(false); }
    },
    /** Open mapped original: the notice to show after the dialog closes, or undefined when refused. */
    async openOriginal(): Promise<string | undefined> {
      if (!handle || busy() || !previewed) return undefined;
      setBusy(true); setNotice(tr("Rechecking the exact conversation, connection and file before opening…"));
      try { const told = await port.openOriginal(handle, previewed, remember()); handle = ""; return trText(told); }
      catch (error) { setVerified(undefined); previewed = undefined; setNotice(problem(error)); return undefined; }
      finally { setBusy(false); }
    },
    close() { if (busy()) return false; if (handle) void port.close(handle).catch(() => {}); handle = ""; return true; },
  };
}
export type FileDialogState = ReturnType<typeof createFileDialog>;

/** The proof's scope for a session's file (Qt SessionsWindow): its exact
 * run and conversation, an absolute WSL path, and the open subagent. None
 * when Qt would not offer the action. */
export function fileScope(session: Pick<Session, "name" | "runId" | "conversationId" | "archiveId"> | undefined, path: string, agentId?: string): WslScope | undefined {
  if (!session || !session.runId || !session.conversationId || !path.startsWith("/")) return undefined;
  const scope: WslScope = { session: session.name, run_id: session.runId, conversation_id: session.conversationId, path };
  if (session.archiveId) scope.archive_id = session.archiveId;
  if (agentId) scope.agent_id = agentId;
  return scope;
}
