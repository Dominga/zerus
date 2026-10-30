import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { toast } from "./notices";
export interface Consents { local_files: boolean; discard_changes: boolean; unchecked_processes: boolean; merged_branches: false }
export interface CleanupRow { id: string; label: string; path: string; machine: string; computer: string; branch: string; size: string; status: "removed" | "kept" | "unknown" | "ready" | "checking" | "removing"; reason: string; details: string; can_check: boolean; ready: boolean }
export interface CleanupView { revision: number; rows: CleanupRow[]; busy: boolean; removed: number; kept: number; unknown: number; required: Consents; text: string; title: string }
export interface Inspection {
  path: string; common_dir: string; files?: { path: string; status: string }[]; files_partial?: boolean;
  changes?: { text?: string; error?: string }; comparison?: { text?: string; error?: string }; commits?: { text?: string; error?: string };
  review?: { sessions?: string[] }; file?: { path: string; entries?: { path: string; directory: boolean; symlink: boolean }[]; next_offset?: number; binary?: boolean; text?: string; truncated?: boolean };
}
export const emptyConsents = (): Consents => ({ local_files: false, discard_changes: false, unchecked_processes: false, merged_branches: false });
export const emptyCleanup = (): CleanupView => ({ revision: 0, rows: [], busy: false, removed: 0, kept: 0, unknown: 0, required: emptyConsents(), text: "", title: "" });
const [cleanup, setCleanup] = createSignal(emptyCleanup());
const [cleanupDialog, setCleanupDialog] = createSignal<{ origin?: HTMLElement }>();
export { cleanup, cleanupDialog };
export function applyCleanup(view: CleanupView) { if (view.revision >= cleanup().revision) setCleanup(view); }
let reading = false, again = false;
export async function refreshCleanup() {
  if (reading) { again = true; return; }
  reading = true;
  try { do { again = false; applyCleanup(await bridge.terminationCleanupView()); } while (again); }
  catch (error) { toast("Working folders", String(error), "warn", true); }
  finally { reading = false; }
}
if (bridge.inTauri()) {
  void bridge.onTerminationCleanupChanged(() => { void refreshCleanup(); }).then(() => refreshCleanup());
  void bridge.onTerminationFolderRemoved((label) => toast("Folder removed", label));
}
export const openCleanup = () => setCleanupDialog({ origin: document.activeElement instanceof HTMLElement ? document.activeElement : undefined });
export const closeCleanup = () => setCleanupDialog(undefined);
async function update(call: Promise<CleanupView>) { try { applyCleanup(await call); } catch (error) { toast("Working folders", String(error), "warn", true); } }
export const checkCleanup = (id: string) => update(bridge.terminationCleanupCheck(id));
export const removeCleanup = (consents: Consents) => update(bridge.terminationCleanupRemove(cleanup().revision, consents));
export async function keepCleanup() { try { applyCleanup(await bridge.terminationCleanupKeep(cleanup().revision)); closeCleanup(); } catch (error) { toast("Working folders", String(error), "warn", true); } }
