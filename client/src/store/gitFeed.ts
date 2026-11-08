import { createEffect, on, onCleanup } from "solid-js";
import { gitStatusRequest, onGitStatus, type GitStatusAnswer } from "../bridge";
import { computerFresh, refreshGit } from "./catalog";
import { markAsked, storeAnswer, visibleFolders } from "./gitStatus";
import type { Session } from "../ui/sessions/types";

// Task 252: the session list's visible rows ask for their folders' git status
// (Qt SessionsWindowRows' gitReads), again every 15 s while they show, which
// also lets an answer age into "not checked" as Qt's badge does. The desktop
// throttles, bounds and caches the reads (core git_status.rs).

const AGAIN_MS = 15_000;

function received(answer: GitStatusAnswer) {
  storeAnswer(answer.computer, answer.path, answer.status);
  refreshGit(answer.computer, answer.path);
}

let listening = false;
function listenOnce() {
  if (listening) return;
  listening = true;
  void onGitStatus(received).catch(() => { listening = false; });
}

/** Asks for the folders these rows show; their cached answers apply at once. */
export function askGit(rows: readonly Session[]) {
  listenOnce();
  for (const [computer, paths] of visibleFolders(rows, computerFresh)) {
    markAsked(computer, paths);
    refreshGit(computer);
    void gitStatusRequest(computer, paths).then((known) => known.forEach(received), () => undefined);
  }
}

/** Follows the rows a list shows: asks when they change and every 15 s while it shows. */
export function followVisibleGit(visible: () => readonly Session[]) {
  createEffect(on(() => visible().map((row) => row.key).join("\n"), () => askGit(visible())));
  const timer = setInterval(() => askGit(visible()), AGAIN_MS);
  onCleanup(() => clearInterval(timer));
}
