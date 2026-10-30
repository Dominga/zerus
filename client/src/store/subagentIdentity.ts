import { createSignal } from "solid-js";
import type { Target } from "../types";
import type { ChildIdentity } from "../ui/sessions/actions-ui";

interface ChildInspection { agent_id: string; conversation_id: string; cwd?: string | null }
export interface ChildTarget { target: Target; archiveId: string | null; agentId: string }

/** Only the open child's identity is retained; late reads never cross a selection. */
export function createSubagentIdentity(read: (target: Target, archiveId: string | null, agentId: string) => Promise<ChildInspection | null>) {
  const [child, setChild] = createSignal<ChildIdentity>();
  let scope = "", generation = 0;
  let wanted: ChildTarget | undefined, reading = false;
  const start = () => {
    const next = wanted;
    if (reading || !next || !next.target.run_id || !next.target.conversation_id) return;
    const mine = generation;
    reading = true;
    void read(next.target, next.archiveId, next.agentId).then((answer) => {
      if (mine !== generation || answer?.agent_id !== next.agentId || !answer.conversation_id) return;
      setChild({ id: next.agentId, conversationId: answer.conversation_id, ...(answer.cwd != null ? { cwd: answer.cwd } : {}) });
    }).catch(() => { /* Keep the agent-ID fallback; reopening may read again. */ }).finally(() => {
      reading = false;
      if (mine !== generation) start();
    });
  };
  return {
    child,
    follow(next: ChildTarget | undefined) {
      const identity = next ? JSON.stringify([next.target, next.archiveId, next.agentId]) : "";
      if (identity === scope) return;
      scope = identity;
      generation++; wanted = next;
      setChild(next ? { id: next.agentId } : undefined);
      start();
    },
  };
}
