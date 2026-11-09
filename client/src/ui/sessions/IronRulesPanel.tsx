import { For, Show, createEffect, createSignal, untrack } from "solid-js";
import { tr } from "../../i18n";
import { trText } from "../../text";
import { ironRules } from "../../bridge";
import { teamConfirm } from "../../store/teamConfirm";
import { MAX_LENGTH, MAX_RULES, createIronRules, rulesHint, type RulesContext } from "../../store/ironRules";
import { Icon } from "../lib/icons";
import "../styles/ironrules.css";
import { TextInput } from "../lib/TextField";

// Task 68: Qt's IronRulesPanel, in the session's Details and on Projects.
// Numbered rules, each removable (confirmed, Cancel focused); a new one is
// added from the field. With a selected session the owner picks the level:
// the project's rules or this session's additions.

export function IronRulesPanel(props: { context: RulesContext; onCount?: (count: number) => void }) {
  const rules = createIronRules({
    call: ironRules,
    confirmRemove: (rule, number) => teamConfirm.ask({
      name: "ironRuleRemove",
      title: tr("Remove iron rule?"),
      text: tr("Remove rule %1: %2? Agents stop receiving it from their next session start or message.", number, rule),
      action: tr("Remove rule"),
    }),
  });
  const [field, setField] = createSignal("");
  createEffect(() => { const next = { ...props.context }; untrack(() => rules.setContext(next)); });
  createEffect(() => props.onCount?.(rules.rules().length));
  const add = async () => { if (rules.canAdd(field()) && (await rules.add(field()))) setField(""); };
  return (
    <Show when={rules.visible()}>
      <div class="iron-rules">
        <div class="row iron-head">
          <Show when={rules.scopes()}>
            <select class="field iron-scope" aria-label={tr("Iron rules level")} value={rules.scope()} disabled={rules.loading()}
              onChange={(event) => rules.setScope(event.currentTarget.value === "session" ? "session" : "project")}>
              <option value="project">{tr("Project")}</option>
              <option value="session">{tr("Session additions")}</option>
            </select>
          </Show>
          <span class="grow" />
          <button type="button" class="icon-btn small" aria-label={tr("Refresh")} title={tr("Refresh")} disabled={rules.loading()} onClick={() => void rules.reload()}><Icon name="refresh-cw" size={13} /></button>
        </div>
        <Show when={rules.error()}>{(error) => <p class="iron-error" role="alert">{trText(error())}</p>}</Show>
        <ol class="iron-list">
          <For each={rules.rules()}>{(rule, index) => (
            <li class="iron-rule">
              <span class="iron-text selectable">{rule}</span>
              <button type="button" class="icon-btn small" aria-label={tr("Remove rule %1", index() + 1)} title={tr("Remove this rule")}
                disabled={!rules.editable()} onClick={() => void rules.remove(index())}><Icon name="x" size={12} /></button>
            </li>
          )}</For>
        </ol>
        <form class="row iron-add" onSubmit={(event) => { event.preventDefault(); void add(); }}>
          <TextInput class="field grow" type="text" value={field()} maxLength={MAX_LENGTH} placeholder={tr("Add a rule…")} aria-label={tr("New iron rule")}
            disabled={!rules.editable() || rules.rules().length >= MAX_RULES} onInput={(event) => setField(event.currentTarget.value)} />
          <button type="submit" class="btn small" disabled={!rules.canAdd(field())}>{tr("Add")}</button>
        </form>
        <p class="hint">{rulesHint()}</p>
      </div>
    </Show>
  );
}
