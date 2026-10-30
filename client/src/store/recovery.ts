import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { computerOffline, catalog } from "./catalog";
import { errorText, followedFor } from "./actions";
import { composer } from "./composer";
import { selected } from "./ui";
import type { Outcome } from "../types";
import { recoveryInspection, type RecoveryView } from "../ui/sessions/recovery-ui";
import type { Session } from "../ui/sessions/types";

// One guarded recovery edit at a time, as Qt's m_recoveryAction. A rejected
// call is an unknown outcome and is never repeated by this store.
const [pending, setPending] = createSignal("");
const [failure, setFailure] = createSignal<{ key: string; detail: string }>();
const identity = (s: Session, jobId: string) => `${s.key}\n${jobId}`;

export const recoveryPending = (s: Session, jobId: string) => pending() === identity(s, jobId);
export const recoveryError = (s: Session, jobId: string) => failure()?.key === identity(s, jobId) ? failure()!.detail : "";

export async function runRecoveryAction(s: Session, view: RecoveryView, action: "now" | "cancel"): Promise<Outcome | undefined> {
  const key = identity(s, view.jobId);
  if (pending()) return undefined;
  const current = selected();
  const listed = catalog.rows[s.key];
  if (!current || current.key !== s.key || !listed || listed.runId !== s.runId || listed.conversationId !== s.conversationId || computerOffline(s.computer)) return undefined;
  const details = recoveryInspection(current, followedFor(current.key));
  const job = (details.recovery ?? {}) as Record<string, unknown>;
  if (!view.jobId || job.id !== view.jobId || job.state !== "waiting") return undefined;

  setFailure(undefined);
  setPending(key);
  let outcome: Outcome | undefined;
  try {
    outcome = await bridge.runAction(
      { computer: s.computer, name: s.name, run_id: s.runId ?? "", conversation_id: s.conversationId ?? "" },
      { kind: "recovery", job_id: view.jobId, action },
    );
    if (outcome.outcome === "done") setFailure(undefined);
    else setFailure({ key, detail: outcome.detail });
  } catch (error) {
    setFailure({ key, detail: errorText(error) });
  } finally {
    if (pending() === key) setPending("");
    if (selected()?.key === s.key) void composer.refresh();
  }
  return outcome;
}
