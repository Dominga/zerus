import { readTaskAttachment } from "../bridge";
import type { Fetched, TaskFileSource } from "../types";

type Read = (project: string, task: string, source: TaskFileSource) => Promise<Fetched>;

/**
 * Task files read on their own. A page names each file by its metadata; a
 * card reads it when it is opened, and an image card reads it for its
 * preview. Previews take turns, one read at a time, and a card that is no
 * longer shown when its turn comes reads nothing. Two cards of one file share
 * one read.
 */
export function createTaskFiles(read: Read = readTaskAttachment) {
  const reading = new Map<string, Promise<Fetched>>();
  let turn: Promise<unknown> = Promise.resolve();
  const file = (project: string, task: string, source: TaskFileSource) => {
    const key = [project, task, source.entry ?? "", source.file, source.meta.sha256].join("\n");
    let pending = reading.get(key);
    if (!pending) {
      pending = read(project, task, source).finally(() => reading.delete(key));
      reading.set(key, pending);
    }
    return pending;
  };
  return {
    read: file,
    preview(project: string, task: string, source: TaskFileSource, shown: () => boolean): Promise<Fetched | undefined> {
      const mine = turn.then(() => shown() ? file(project, task, source) : undefined);
      turn = mine.catch(() => undefined);
      return mine;
    },
  };
}
export const taskFiles = createTaskFiles();
