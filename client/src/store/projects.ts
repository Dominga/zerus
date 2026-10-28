import { createSignal } from "solid-js";
import { projectsView } from "../bridge";
import { emptyProjects, type ProjectsView } from "../types";
import { latest } from "./latest";

export const [projectsPage, setProjectsPage] = createSignal<ProjectsView>(emptyProjects());
const pull = latest(setProjectsPage);
export const loadProjects = () => pull(() => projectsView());

/** A project another screen asks Projects to select (New session's Manage this project's folders); Projects takes it once. */
export const [requestedProject, setRequestedProject] = createSignal<string | undefined>();
export const requestProject = (id: string) => setRequestedProject(id);
