import { createSignal } from "solid-js";
import { reviewTaskCopy } from "../bridge";
import { tasks } from "./tasks";

export interface CopyReviewReply {
  abi: string; project: { id: string }; uuid: string;
  state: "review" | "moved" | "refused" | "unknown";
  fingerprint?: string; backup?: string; reason?: string;
}
export interface ReviewingCopy { project: string; uuid: string; fingerprint: string; origin: HTMLElement }
export const [reviewingCopy, setReviewingCopy] = createSignal<ReviewingCopy>();
export const [copyReviewBusy, setCopyReviewBusy] = createSignal(false);
export const [copyReviewOutcomes, setCopyReviewOutcomes] = createSignal<Record<string, { state: "moved" | "refused" | "unknown"; reason: string }>>({});
const keyOf = (project: string, uuid: string) => `${project}\n${uuid}`;
function outcome(project: string, uuid: string, state: "moved" | "refused" | "unknown", reason = "") {
  const key = keyOf(project, uuid), entries = Object.entries(copyReviewOutcomes()).filter(([k]) => k !== key).slice(-15);
  setCopyReviewOutcomes({ ...Object.fromEntries(entries), [key]: { state, reason } });
}
export const copyReviewOutcome = (project: string, uuid: string) => copyReviewOutcomes()[keyOf(project, uuid)];
export function dismissCopyReview(project: string, uuid: string) {
  const key = keyOf(project, uuid);
  if (copyReviewOutcomes()[key]?.state !== "moved") return;
  const next = { ...copyReviewOutcomes() }; delete next[key]; setCopyReviewOutcomes(next);
}
export function copyReviewCards(project: string, live: { uuid: string; error: string }[]) {
  const finished = Object.keys(copyReviewOutcomes()).filter((key) => key.startsWith(`${project}\n`))
    .map((key) => ({ uuid: key.slice(project.length + 1), error: "" }));
  return [...finished, ...live.filter((row) => !finished.some((done) => done.uuid === row.uuid))].slice(0, 5);
}
function matches(reply: CopyReviewReply, project: string, uuid: string) {
  return reply.abi === "task-copy-review-v1" && reply.project?.id === project && reply.uuid === uuid;
}
export async function openCopyReview(project: string, uuid: string, origin: HTMLElement) {
  if (copyReviewBusy() || reviewingCopy() || ["unknown", "moved"].includes(copyReviewOutcome(project, uuid)?.state ?? "")) return;
  setCopyReviewBusy(true);
  try {
    const reply = await reviewTaskCopy(project, uuid);
    if (tasks.project() !== project) return;
    if (!matches(reply, project, uuid) || reply.state !== "review" || !/^[0-9a-f]{64}$/.test(reply.fingerprint ?? "")) throw new Error(reply.reason || "Invalid local task-copy review.");
    setReviewingCopy({ project, uuid, fingerprint: reply.fingerprint!, origin });
  } catch (error) { outcome(project, uuid, "refused", String(error)); }
  finally { setCopyReviewBusy(false); }
}
export async function confirmCopyReview() {
  const current = reviewingCopy();
  if (!current || copyReviewBusy()) return;
  setCopyReviewBusy(true);
  try {
    const reply = await reviewTaskCopy(current.project, current.uuid, current.fingerprint);
    if (!matches(reply, current.project, current.uuid) || !["moved", "refused", "unknown"].includes(reply.state)) throw new Error("Invalid local task-copy review outcome.");
    outcome(current.project, current.uuid, reply.state as "moved" | "refused" | "unknown", reply.reason || "");
  } catch (error) { outcome(current.project, current.uuid, "unknown", String(error)); }
  finally { setCopyReviewBusy(false); if (reviewingCopy() === current) setReviewingCopy(undefined); }
}
