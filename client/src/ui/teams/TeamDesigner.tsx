import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount, untrack } from "solid-js";
import type { JSX } from "solid-js";
import { tr } from "../../i18n";
import { Icon } from "../lib/icons";
import { ActivityView } from "../sessions/Activity";
import { teamAction, teamRoleModels } from "../../bridge";
import { orderedRows } from "../../store/catalog";
import { groupOf, headerOf, INBOX } from "../../store/organization";
import { loadProjects, projectsPage } from "../../store/projects";
import { machineCards } from "../../store/machines";
import { activity, designerActivity } from "../../store/session";
import { selected, setDetailTab, setSelectedKey } from "../../store/ui";
import { showCatalog, teams } from "../../store/teams";
import { catalogPort } from "../../store/teamsCatalog";
import { teamConfirm } from "../../store/teamConfirm";
import { createCatalogEditor } from "../../store/catalogEditor";
import { designerChoices, designerProjects, designerRow, sessionContext, type DesignerContext } from "../../store/teamDesigner";
import { parseCatalog, proposalCounts } from "./catalog";
import { TeamCatalog } from "./TeamCatalog";
import { TeamCatalogEditor } from "./TeamCatalogEditor";
import { DESIGNER_AGENTS, designerModelLabel, effortAllowed, effortLabel } from "./designerModels";
import { createModelCatalogs, effortOptions, modelOptions } from "./catalogControls";
import { normalizeModelCatalog, selectModel } from "../provider-selection";
import { TextArea } from "../lib/TextField";

// Task #44: Qt's TeamDesignerPage. The designer's conversation (the running
// "…/team-designer" session) shows here and follows the selected local
// session's project as in Qt; without a designer, the owner picks a project,
// the agent and describes the work, and Start begins it through the guarded
// `hgs team catalog designer`. Its draft's proposals are kept or rejected
// here; the catalog editor sits beside it.

const AGENTS = DESIGNER_AGENTS;
/** Qt waits this long for a started designer to reach the session list. */
const START_WAIT_MS = 30_000;

export function TeamDesigner(props: { onClose?: () => void }): JSX.Element {
  const hide = showCatalog("");
  onCleanup(hide);
  onMount(() => void loadProjects());

  const designer = createMemo(() => designerRow(orderedRows()));
  const catalogView = createMemo(() => { const view = teams.catalog(""); return view ? parseCatalog(view) : undefined; });
  const counts = createMemo(() => { const view = catalogView(); return view ? proposalCounts(view) : { kept: 0, undecided: 0, rejected: 0 }; });

  // The project: the selected session's, unless a running designer keeps its own.
  const [context, setContext] = createSignal<DesignerContext>({ folder: "", project: "" });
  createEffect(() => {
    const row = selected();
    const group = row ? groupOf(row.key) : undefined;
    const name = group && group !== INBOX ? headerOf(group)?.name : undefined;
    const current = untrack(context);
    const next = sessionContext(row, name, !!designer(), current);
    if (next && (next.folder !== current.folder || next.project !== current.project)) setContext(next);
  });
  const choices = createMemo(() => designerChoices(designerProjects(projectsPage().projects, machineCards[""]?.host ?? ""), context()));
  const chosen = () => { const { choices: all, chosen: index } = choices(); return index >= 0 ? all[index] : undefined; };

  // The conversation: its own follow unless it is the selected session.
  const own = () => !!designer() && designer()!.key === selected()?.key;
  createEffect(() => {
    const row = designer();
    designerActivity.follow(row && !own() ? { id: row.key, computer: row.computer, key: row.catalogKey, name: row.name, runId: row.runId, conversationId: row.conversationId } : undefined);
  });
  onCleanup(() => designerActivity.follow(undefined));
  const openDesigner = () => { const row = designer(); if (row) { setSelectedKey(row.key); setDetailTab("activity"); } };

  // Start: the guarded call, then the session list shows the designer (Qt waits 30 s).
  const [agent, setAgent] = createSignal("claude");
  const [model, setModel] = createSignal("");
  const [effort, setEffort] = createSignal("");
  const [modelMenu, setModelMenu] = createSignal(false);
  const [effortMenu, setEffortMenu] = createSignal(false);
  const [modelTick, setModelTick] = createSignal(0);
  const modelCatalogs = createModelCatalogs((computer, agentId, account) => teamRoleModels(computer, agentId, account), () => setModelTick((n) => n + 1));
  const modelRole = () => ({ computer: "", agent: agent(), account: "", model: model() });
  const accountModels = () => {
    modelTick();
    const role = modelRole();
    modelCatalogs.request(role);
    const options = modelOptions(role, modelCatalogs.get(role));
    return options.length ? options : [{ id: "", label: tr("Default model") }];
  };
  const accountEfforts = () => effortAllowed(agent()) ? effortOptions(model(), modelCatalogs.get(modelRole()), effort()) : [];
  const [goal, setGoal] = createSignal("");
  const [starting, setStarting] = createSignal(false);
  const [startError, setStartError] = createSignal("");
  let waiting: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(waiting));
  createEffect(() => { if (designer() && starting()) { setStarting(false); clearTimeout(waiting); setGoal(""); } });
  const start = async () => {
    const project = chosen();
    if (!project?.folder) { setStartError(tr("Choose a project with a folder on this computer first.")); return; }
    setStartError("");
    setStarting(true);
    try {
      const outcome = await teamAction({
        computer: "",
        confirmed: true,
        action: { kind: "catalog_designer", folder: project.folder, goal: goal().trim(), agent: agent(), model: model().trim(), effort: effort().trim(), project: project.name },
      });
      if (outcome.outcome === "done") {
        waiting = setTimeout(() => { if (!designer()) { setStarting(false); setStartError(tr("The Team designer did not start.")); } }, START_WAIT_MS);
        return;
      }
      setStarting(false);
      setStartError(tr("The Team designer did not start: %1", outcome.outcome === "confirm" ? outcome.confirmation.text : outcome.detail));
    } catch (error) {
      setStarting(false);
      setStartError(tr("The Team designer did not start: %1", error instanceof Error ? error.message : String(error)));
    }
  };

  const status = () => {
    const row = designer(), view = catalogView();
    if (!row) return tr("Not started");
    if (row.status === "attention") return tr("Needs your answer");
    if (row.status === "working") return tr("Working");
    if (view && view.proposals.length && view.draftBase && view.draftBase !== view.version) return tr("Draft is out of date");
    if (counts().undecided > 0) return tr("Waiting for your decision");
    return tr("Idle");
  };

  // The editor's Apply and Discard draft, asked as Qt asks them.
  const editor = createCatalogEditor();
  const draft = createMemo(() => {
    const view = catalogView();
    if (!view || view.proposals.length === 0) return undefined;
    const { kept, undecided } = counts();
    return {
      kept,
      undecided,
      apply: async () => {
        const text = tr("Write %1 kept changes as the next catalog version? %2 undecided changes are dropped with the draft.", kept, undecided);
        if (await teamConfirm.ask({ name: "teamCatalogApplyConfirm", title: tr("Apply the catalog draft?"), text, action: tr("Apply") })) await catalogPort.apply();
      },
      discard: async () => {
        if (await teamConfirm.ask({ name: "teamCatalogDiscardConfirm", title: tr("Discard draft"), text: tr("Drop every proposal in the designer's draft? The catalog stays as it is."), action: tr("Discard draft") })) await catalogPort.discard();
      },
    };
  });

  return (
    <section class="designer" aria-label={tr("Team designer")}>
      <header class="designer-head">
        <h2>{tr("Team designer")}</h2>
        <span class="designer-status">{status()}</span>
        <label class="designer-project">
          <span>{tr("Project")}</span>
          <select aria-label={tr("Project for the Team designer")} disabled={!!designer()} title={designer() ? tr("The running designer keeps its original project") : tr("Choose the project the designer reads")}
            value={String(choices().chosen)} onChange={(event) => {
              const pick = choices().choices[Number(event.currentTarget.value)];
              setContext(pick ? { folder: pick.folder, project: pick.name } : { folder: "", project: "" });
            }}>
            <option value="-1">{choices().choices.length === 0 ? tr("No local project folders") : ""}</option>
            <For each={choices().choices}>{(choice, index) => (
              <option value={String(index())} title={choice.folder}>{choice.folderName ? tr("%1 (%2)", choice.name || choice.folder, choice.folderName) : choice.name || choice.folder}</option>
            )}</For>
          </select>
        </label>
        <span class="dim ell">{tr("Reads %1 (project)", chosen()?.folder || tr("no folder"))}</span>
        <span class="grow" />
        <span class="dim">{catalogView()?.builtin ? tr("Built-in catalog") : tr("Catalog v%1", catalogView()?.version ?? 0)}</span>
        <span class="designer-chip"><Icon name="shield-check" size={12} />{tr("Proposes only")}</span>
        <Show when={designer() && !own()}>
          <button type="button" class="btn small ghost" onClick={openDesigner}><Icon name="external-link" size={12} />{tr("Open in Activity")}</button>
        </Show>
        <Show when={props.onClose}>
          <button type="button" class="icon-btn small" aria-label={tr("Close")} onClick={() => props.onClose?.()}><Icon name="x" size={14} /></button>
        </Show>
      </header>
      <div class="designer-body">
        <div class="designer-conversation">
          <Show when={designer()} fallback={
            <div class="designer-empty">
              <h3>{tr("Design roles and teams with an agent")}</h3>
              <p class="dim">{tr("Choose a project and describe the work. The designer reads the project and the existing catalog, then proposes roles, templates and rules; nothing changes until you keep and apply them.")}</p>
              <div class="designer-agent">
                <label><span>{tr("Agent")}</span>
                  <select value={agent()} onChange={(event) => { setAgent(event.currentTarget.value); setModel(""); setEffort(""); }}>
                    <For each={AGENTS}>{([id, name]) => <option value={id}>{name}</option>}</For>
                  </select>
                </label>
                <div class="designer-model">
                  <button type="button" class="btn small designer-model-button" data-action="designer-model"
                    aria-haspopup="menu" aria-expanded={modelMenu()}
                    title={tr("The designer's agent, model and effort")}
                    onClick={() => { setModelMenu(!modelMenu()); setEffortMenu(false); }}>
                    <span>{designerModelLabel(agent(), model(), effort())}</span>
                    <Icon name="chevron-down" size={12} />
                  </button>
                  <Show when={modelMenu()}>
                    <div class="designer-model-menu" role="menu" aria-label={tr("Agent, model and effort")}>
                      <Show when={!effortMenu()} fallback={
                        <>
                          <button type="button" role="menuitem" class="designer-model-back" data-action="designer-effort-back"
                            onClick={() => setEffortMenu(false)}>‹ {effortLabel(effort())}</button>
                          <For each={accountEfforts()}>
                            {(choice) => (
                              <button type="button" role="menuitem" classList={{ on: effort() === choice }} data-designer-effort={choice}
                                onClick={() => { setEffort(choice); setModelMenu(false); setEffortMenu(false); }}>
                                {choice || tr("Provider default")}
                              </button>
                            )}
                          </For>
                        </>
                      }>
                        <For each={accountModels()}>
                          {(option) => (
                            <button type="button" role="menuitem" classList={{ on: model() === option.id }}
                              data-designer-agent={agent()} data-designer-model={option.id}
                              onClick={() => {
                                const next = selectModel(normalizeModelCatalog(modelCatalogs.get(modelRole()) ?? []), option.id, effort());
                                setModel(next.modelId);
                                setEffort(next.effortId);
                                setModelMenu(false);
                              }}>
                              {option.id ? option.label : tr("Default model")}
                            </button>
                          )}
                        </For>
                        <button type="button" role="menuitem" class="designer-model-effort" data-action="designer-effort"
                          disabled={!effortAllowed(agent())} onClick={() => setEffortMenu(true)}>
                          {effortLabel(effort())}
                        </button>
                        <div class="designer-model-note">{tr("Runs on this computer")}</div>
                      </Show>
                    </div>
                  </Show>
                </div>
              </div>
              <TextArea rows={5} value={goal()} placeholder={chosen()?.folder ? tr("Describe the project tasks…") : tr("Choose a project above…")} onInput={(event) => setGoal(event.currentTarget.value)} />
              <div class="designer-start">
                <Show when={startError()}><span class="cat-error" role="alert">{startError()}</span></Show>
                <span class="grow" />
                <button type="button" class="btn primary small" disabled={starting() || !goal().trim() || !chosen()?.folder} onClick={() => void start()}>
                  {starting() ? tr("Starting…") : tr("Start")}
                </button>
              </div>
            </div>
          }>
            <div class="activity-wrap designer-activity" style={{ "--col": "100%" } as JSX.CSSProperties}>
              <ActivityView source={own() ? activity : designerActivity} session={designer} />
            </div>
            <Show when={!own()}>
              <div class="designer-reply">
                <span class="dim">{tr("Ask for changes in the designer's own session.")}</span>
                <button type="button" class="btn small" onClick={openDesigner}>{tr("Reply in Activity")}</button>
              </div>
            </Show>
          </Show>
          <Show when={catalogView() && catalogView()!.proposals.length > 0}>
            <div class="designer-draft">
              <TeamCatalog catalog={catalogView()!} port={catalogPort} />
            </div>
          </Show>
        </div>
        <div class="designer-editor">
          <TeamCatalogEditor editor={editor} savedVersion={catalogView()?.version} draft={draft()} />
        </div>
      </div>
    </section>
  );
}
