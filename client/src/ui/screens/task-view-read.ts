import { readTaskDetails } from "../../bridge";
import { object, taskItem, type Task } from "./tasks-ui";
import { tr } from "../../i18n";

const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * A selected task beyond the loaded rows of a windowed list, read on its own
 * (`project\nuuid`). The inspector's own read may hold the one-read permit
 * briefly, so that refusal is waited out; any other failure is returned.
 */
export async function readSelectedTask(key: string, read = readTaskDetails): Promise<Task> {
  const [project, uuid] = key.split("\n");
  for (let attempt = 0; ; attempt++) {
    try {
      const task = taskItem(object(await read(project, uuid, null)));
      if (task.uuid !== uuid) throw new Error(tr("This task could not be read."));
      return task;
    } catch (e) {
      if (attempt >= 4 || !String(e).includes("already loading")) throw e;
      await pause(250);
    }
  }
}
