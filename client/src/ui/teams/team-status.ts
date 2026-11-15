import { tr } from "../../i18n";

// The TeamStrip vocabulary, shared by every badge for a team's status.
// The strip was adapted from WhiteWh's SessionCard/TeamBar.
export const TEAM_STATUS: Readonly<Record<string, () => string>> = {
  running: () => tr("Running"), planning: () => tr("Planning"), paused: () => tr("Paused"),
  stuck: () => tr("Stuck"), finished: () => tr("Finished"), stopped: () => tr("Stopped"), archived: () => tr("Archived"),
};

export const teamStatusLabel = (status: string): string => TEAM_STATUS[status]?.() ?? status;
