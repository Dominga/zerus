import { createSignal } from "solid-js";
import { tr } from "../i18n";
import type { RulesCall, RulesOutcome, RulesTarget } from "../bridge";
import type { Text } from "../text";

// Task 68: project and session iron rules (Qt's IronRulesPanel). hgs owns
// them; client/core builds each guarded call and projects its reply (see
// client/core/src/iron_rules.rs). The panel shows one level at a time: the
// project's rules, or the selected session's own additions. A write sends
// the whole list at the revision it was read at; a refused write leaves the
// rules as they were, and one that may have run reads them again.

export const MAX_RULES = 20;
export const MAX_LENGTH = 300;

export type RulesScope = "project" | "session";
export interface RulesSession { name: string; run_id: string; conversation_id: string }
export interface RulesContext {
  /** The selected session's computer; "" for this one. */
  computer: string;
  /** The project's id; "" when none. */
  project: string;
  /** The selected session, when its additions can be shown. */
  session?: RulesSession;
}
interface Port {
  call(call: RulesCall): Promise<RulesOutcome>;
  /** Asks the owner before a rule is removed; Cancel is the default. */
  confirmRemove(rule: string, number: number): Promise<boolean>;
}

const same = (a: RulesContext | undefined, b: RulesContext) =>
  !!a && a.computer === b.computer && a.project === b.project && a.session?.name === b.session?.name
  && a.session?.run_id === b.session?.run_id && a.session?.conversation_id === b.session?.conversation_id;

const message = (error: unknown): Text => ({ source: "%1", args: [{ kind: "verbatim", value: String(error) }] });

export function createIronRules(port: Port) {
  const [context, setContextSignal] = createSignal<RulesContext>();
  const [scope, setScopeSignal] = createSignal<RulesScope>("project");
  const [rules, setRules] = createSignal<string[]>([]);
  const [revision, setRevision] = createSignal(0);
  const [loading, setLoading] = createSignal(false);
  const [failed, setFailed] = createSignal(false);
  const [error, setError] = createSignal<Text>();
  let generation = 0;

  const target = (): RulesTarget | undefined => {
    const now = context();
    if (!now) return undefined;
    if (scope() === "session") return now.session?.name && now.session.run_id ? { scope: "session", ...now.session } : undefined;
    return now.project ? { scope: "project", project: now.project } : undefined;
  };

  async function reload(keep?: Text) {
    const current = ++generation;
    setRules([]); setError(undefined); setFailed(false);
    const where = target();
    if (!where) { setLoading(false); return; }
    setLoading(true);
    let outcome: RulesOutcome;
    try { outcome = await port.call({ computer: context()!.computer, target: where, op: { op: "read" } }); }
    catch (thrown) { outcome = { outcome: "failed", detail: message(thrown) }; }
    if (current !== generation) return;
    setLoading(false);
    if (outcome.outcome === "done") { setRules(outcome.view.rules); setRevision(outcome.view.revision); setError(keep); return; }
    setFailed(true); setError(outcome.detail);
  }

  async function save(list: string[]): Promise<boolean> {
    const where = target();
    if (!where || loading() || failed()) return false;
    const current = generation;
    setLoading(true); setError(undefined);
    let outcome: RulesOutcome;
    try { outcome = await port.call({ computer: context()!.computer, target: where, op: { op: "save", rules: list, expected_revision: revision() } }); }
    catch (thrown) { outcome = { outcome: "failed", detail: message(thrown) }; }
    if (current !== generation) return false;
    setLoading(false);
    switch (outcome.outcome) {
      case "done": setRules(outcome.view.rules); setRevision(outcome.view.revision); return true;
      case "uncertain": void reload(outcome.detail); return false;
      case "unsupported": setFailed(true); setError(outcome.detail); return false;
      default: setError(outcome.detail); return false;
    }
  }

  const editable = () => !loading() && !failed() && !!target();
  return {
    context, scope, rules, revision, loading, failed, error, editable,
    /** Whether the panel shows at all (Qt: a project or a session). */
    visible: () => !!context()?.project || !!context()?.session,
    /** The scope choice shows only with a session to hold additions. */
    scopes: () => !!context()?.session,
    setContext(next: RulesContext) {
      if (same(context(), next)) return;
      setContextSignal(next);
      if (!next.session) setScopeSignal("project");
      void reload();
    },
    setScope(next: RulesScope) {
      if (next === scope() || loading()) return;
      setScopeSignal(next);
      void reload();
    },
    reload: () => reload(),
    canAdd: (text: string) => editable() && text.trim().length > 0 && rules().length < MAX_RULES,
    /** Adds the field's rule; true when it was saved (the field clears). */
    add: (text: string) => (editable() && text.trim() && rules().length < MAX_RULES ? save([...rules(), text]) : Promise.resolve(false)),
    async remove(at: number) {
      const list = rules();
      if (at < 0 || at >= list.length || !editable()) return false;
      if (!(await port.confirmRemove(list[at], at + 1))) return false;
      if (rules() !== list) return false;
      return save(list.filter((_, index) => index !== at));
    },
  };
}
export type IronRules = ReturnType<typeof createIronRules>;

/** The hint under the panel (Qt's render()). */
export const rulesHint = () => tr("Rules add to inherited project, team and member rules. Delivered at session start and each team task. Agents without startup delivery also receive them with messages.");
