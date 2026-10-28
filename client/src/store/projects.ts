import { createSignal } from "solid-js";
import { projectsView } from "../bridge";
import { emptyProjects, type ProjectsView } from "../types";
import { latest } from "./latest";

export const [projectsPage, setProjectsPage] = createSignal<ProjectsView>(emptyProjects());
const pull = latest(setProjectsPage);
export const loadProjects = () => pull(() => projectsView());
