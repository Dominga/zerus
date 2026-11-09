import { Show, createMemo, createSignal, type JSX } from "solid-js";
import { tr } from "../../../i18n";
import { grouping } from "../../../store/organization";
import { ProjectMemoryPanel } from "../ProjectMemoryPanel";
import { Row, Select } from "./ui";

/**
 * Settings > Memory (task #397): the project's RustCodeLore store, its search
 * over the project and its team sub-stores, and the model-download notice that
 * search brings. Projects > Memory has the same panel beside the project.
 */
export function MemorySection(): JSX.Element {
  const projects = createMemo(() => grouping().headers.filter((header) => header.id && header.name).map((header) => ({ value: header.id, label: header.name })));
  const [chosen, setChosen] = createSignal("");
  const project = () => projects().find((row) => row.value === chosen())?.value ?? projects()[0]?.value ?? "";
  return (
    <div class="set-section">
      <h2>{tr("Memory")}</h2>
      <p class="set-note">{tr("Project memory (RustCodeLore): the store, its search and the model it downloads. A search finds the project's own records and its teams' workspaces, each hit naming its store.")}</p>
      <Show when={projects().length} fallback={<p class="set-note">{tr("No projects yet: make one on Projects to give it memory.")}</p>}>
        <Row label={tr("Project")} hint={tr("The project whose memory this shows.")}>
          <Select id="set-memory-project" value={project()} options={projects()} onChange={setChosen} />
        </Row>
        <Show when={project()}><ProjectMemoryPanel project={project()} /></Show>
      </Show>
    </div>
  );
}
