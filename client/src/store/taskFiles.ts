import { createSignal, onCleanup, type Accessor } from "solid-js";
import { readTaskAttachment } from "../bridge";
import type { Fetched, TaskFileSource } from "../types";

type Read = (project: string, task: string, source: TaskFileSource) => Promise<Fetched>;
type TaskFiles = ReturnType<typeof createTaskFiles>;

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

/** What a file card uses: the reads, the system opener and the notices. */
export interface TaskFileCardPort {
  files: TaskFiles;
  open(path: string): Promise<string | null>;
  notify(tone: "info" | "error", text: string): void;
}

/**
 * The reads of one file card, owned by the component that shows it. A copy
 * already in the cache shows at once, an image is read for its preview and
 * any other file when it is opened. A read that finishes after the card was
 * removed changes nothing: a preview is not applied, and an open neither keeps
 * the copy nor opens the file.
 */
export function taskFileCard(file: Fetched, project: string, task: string, port: TaskFileCardPort): {
  copy: Accessor<Fetched | undefined>; reading: Accessor<boolean>; open(): Promise<void>;
} {
  const source = file.source;
  const [copy, setCopy] = createSignal<Fetched | undefined>(file.path ? file : undefined);
  const [reading, setReading] = createSignal(false);
  let shown = true;
  onCleanup(() => { shown = false; });
  if (!copy() && source && file.mime.startsWith("image/")) {
    void port.files.preview(project, task, source, () => shown).then((read) => { if (read && shown) setCopy(read); }, () => {});
  }
  const open = async () => {
    if (reading()) return;
    try {
      let read = copy();
      if (!read) {
        if (!source) return;
        setReading(true);
        read = await port.files.read(project, task, source);
        if (!shown) return;
        setCopy(read);
      }
      const notice = await port.open(read.path); if (notice) port.notify("info", notice);
    } catch (error) { port.notify("error", String(error)); }
    finally { setReading(false); }
  };
  return { copy, reading, open };
}
