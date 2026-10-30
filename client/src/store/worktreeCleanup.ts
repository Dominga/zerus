import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { toast } from "./notices";
export interface WorktreeReviewRow { id: number; path: string; branch: string; state: string; brief: string; details: string; selectable: boolean; checked: boolean }
export interface WorktreeReview { id: string; revision: number; computer: string; machine: string; repository: string; focus: string; rows: WorktreeReviewRow[]; busy: boolean; summary: string; unknown: boolean }
export interface WorktreeConfirmation { id: string; paths: string[]; branches: string[]; ignored: string[]; missing: boolean; consent: string | null; process_warning: string | null; button: string; machine: string }
export const [worktreeReviewDialog, setWorktreeReviewDialog] = createSignal<{ origin?: HTMLElement; focus: string; machine: string }>();
export const [worktreeReview, setWorktreeReview] = createSignal<WorktreeReview>();
export const [worktreeReviewLoading, setWorktreeReviewLoading] = createSignal(false);
export const [worktreeReviewIssuing, setWorktreeReviewIssuing] = createSignal(false);
export const [worktreeReviewError, setWorktreeReviewError] = createSignal("");
let opening = 0, fetching = false, again = false;
const failure = (error: unknown) => { setWorktreeReviewError(String(error)); toast("Worktrees", String(error), "warn", true); };
export function applyWorktreeReview(view: WorktreeReview) { const old = worktreeReview(); if (worktreeReviewDialog() && (!old || old.id === view.id && view.revision > old.revision)) setWorktreeReview(view); }
export async function openWorktreeReview(computer: string, path: string, focus = "", origin?: HTMLElement) {
  if (worktreeReviewDialog()) return;
  const ticket = ++opening;
  setWorktreeReview(undefined); setWorktreeReviewError(""); setWorktreeReviewLoading(true);
  setWorktreeReviewDialog({ origin, focus, machine: computer || "this computer" });
  try { const view = await bridge.worktreeCleanupOpen(computer, path, focus); if (ticket === opening) applyWorktreeReview(view); else void bridge.worktreeCleanupClose(view.id).catch(() => {}); }
  catch (error) { if (ticket === opening) failure(error); }
  finally { if (ticket === opening) setWorktreeReviewLoading(false); }
}
export function closeWorktreeReview() {
  const view = worktreeReview(); if (view?.busy || worktreeReviewIssuing()) return;
  ++opening; setWorktreeReviewDialog(undefined); setWorktreeReview(undefined); setWorktreeReviewLoading(false);
  if (view) void bridge.worktreeCleanupClose(view.id).catch(() => {});
}
export async function refreshWorktreeView() {
  if (fetching) { again = true; return; } fetching = true;
  try { do { again = false; const id = worktreeReview()?.id; if (id) { const reply = await bridge.worktreeCleanupView(id); if (worktreeReview()?.id === id) applyWorktreeReview(reply); } } while (again); }
  catch (error) { if (worktreeReviewDialog()) failure(error); } finally { fetching = false; }
}
export async function checkWorktreesAgain() {
  const view = worktreeReview(); if (!view || view.busy || worktreeReviewLoading()) return;
  setWorktreeReviewLoading(true); setWorktreeReviewError("");
  try { const reply = await bridge.worktreeCleanupRefresh(view.id); if (worktreeReview()?.id === view.id) applyWorktreeReview(reply); }
  catch (error) { failure(error); await refreshWorktreeView(); }
  finally { if (worktreeReview()?.id === view.id) setWorktreeReviewLoading(false); }
}
export async function prepareWorktreeRemoval(selected: number[], branches: boolean) {
  const view = worktreeReview(); if (!view || view.busy || worktreeReviewLoading()) return;
  try { const reply = await bridge.worktreeCleanupPrepare(view.id, view.revision, selected, branches); if (worktreeReview()?.id === view.id && worktreeReview()?.revision === view.revision) return reply; } catch (error) { failure(error); await refreshWorktreeView(); }
}
export async function confirmWorktreeRemoval(confirmation: string, consent: boolean) {
  const view = worktreeReview(); if (!view || worktreeReviewIssuing()) return;
  setWorktreeReviewIssuing(true);
  try { applyWorktreeReview(await bridge.worktreeCleanupConfirm(view.id, confirmation, consent)); }
  catch (error) { failure(error); await refreshWorktreeView(); }
  finally { setWorktreeReviewIssuing(false); }
}
export async function cancelWorktreeConfirmation() {
  const view = worktreeReview(); if (!view || view.busy) return;
  setWorktreeReviewLoading(true);
  try { await bridge.worktreeCleanupClose(view.id); await refreshWorktreeView(); }
  catch (error) { if (worktreeReview()?.id === view.id) failure(error); }
  finally { if (worktreeReview()?.id === view.id) setWorktreeReviewLoading(false); }
}
if (bridge.inTauri()) void bridge.onWorktreeCleanupChanged(() => { void refreshWorktreeView(); });
