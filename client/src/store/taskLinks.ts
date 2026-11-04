import { tr } from "../i18n";

/** A short task reference: `T-` and six base-36 characters, alone or as a task's `ref` (`PROJECT/T-xxxxxx`). */
export const SHORT_REFERENCE = /^(?:[^\n/]+(?:\/[^\n/]+)*\/)?T-[0-9a-z]{6}$/;

/** One key for the link's notice: a later link's notice takes its place instead of stacking. */
export const LINK_NOTICE = "task-link";

export interface LinkedReferencePort {
  /** One bounded read that returns the UUID of the task the reference names, or fails. */
  resolve(project: string, reference: string): Promise<string>;
  open(project: string, uuid: string): void;
  notify(tone: "error", text: string, sticky: boolean, key: string): void;
  /** Whether the board is still as the link left it: no other task or project opened meanwhile. */
  unchanged(): boolean;
}

/**
 * A deep link names a task by its short reference beyond a windowed list.
 * It opens only when the bounded read confirms that task, and only while the
 * board is still as the link left it, so it never opens later by surprise.
 * Otherwise nothing opens and the board stays without it.
 */
export async function openLinkedReference(project: string, reference: string, port: LinkedReferencePort): Promise<void> {
  let uuid: string;
  try { uuid = await port.resolve(project, reference); }
  catch { port.notify("error", tr("Task %1 was not found in this project.", reference), false, LINK_NOTICE); return; }
  if (port.unchanged()) port.open(project, uuid);
}
