import { batch } from "solid-js";
import { setFolderFilter, setProjectFilter, setAccountFilter, setStateFilter, setMachineFilter, setSearch, setPage, toast } from "./ui";
import type { FolderScope } from "../ui/sessions/scope-filters";

/** SessionsWindow::filterFolder: a folder action replaces project/account scopes. */
export function filterFolder(folder: FolderScope) {
  batch(() => { setProjectFilter([]); setAccountFilter([]); setFolderFilter(folder); setPage("sessions"); });
  toast("Folder filter", `The list shows the sessions in ${folder.path}`);
}
/** The mockup's Worktrees All sessions action. */
export function clearFolderFilter() {
  batch(() => { setFolderFilter(undefined); setProjectFilter([]); setAccountFilter([]); setStateFilter("all"); setMachineFilter([]); setSearch(""); setPage("sessions"); });
  toast("All sessions", "The folder filter was cleared");
}

/** Qt's selected-folder chip clears only that scope. */
export function clearFolderScope() {
  setFolderFilter(undefined);
  toast("All sessions", "The folder filter was cleared");
}
