import type { DetailTab } from "../../store/ui";
import type { InspectorTab } from "../../store/inspector";

export interface DetailFrame {
  key: string;
  catalogBound: boolean;
  sessionId?: string;
  route: { tab: DetailTab; inspector: InspectorTab | "closed"; child: boolean };
  element: Pick<HTMLElement, "isConnected">;
}

/** One pending frame check, no history or content, and at most one report. */
export function watchDetailRemoval(port: {
  listed(key: string): boolean;
  schedule(check: () => void): () => void;
  report(line: string): void;
}) {
  let current: DetailFrame | undefined, cancel: (() => void) | undefined, reported = false;
  return {
    update(next: DetailFrame | undefined) {
      const previous = current;
      current = next;
      if (!previous?.catalogBound || port.listed(previous.key) || reported) return;
      cancel?.();
      cancel = port.schedule(() => {
        cancel = undefined;
        if (reported || !previous.element.isConnected || port.listed(previous.key)) return;
        // A retained frame may now show a different selected row normally.
        if (current?.element === previous.element && current.key !== previous.key) return;
        reported = true;
        const id = /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(previous.sessionId ?? "")
          ? previous.sessionId! : "unavailable";
        const tab = ["activity", "processes", "terminal", "native", "taskview"].includes(previous.route.tab) ? previous.route.tab : "unknown";
        const inspector = ["plan", "details", "worktrees", "tasks", "closed"].includes(previous.route.inspector) ? previous.route.inspector : "unknown";
        port.report(`Zerus detail invariant: route=sessions/${tab}/${inspector}/${previous.route.child ? "child" : "parent"} session_id=${id} reason=removed_catalog_row_still_mounted`);
      });
    },
    dispose() { cancel?.(); cancel = undefined; current = undefined; },
  };
}
