import { NewSessionDialogs } from "./setup/NewSession";
import { CreationDialog } from "./setup/Creation";
import { CloneDialogs } from "./setup/Clone";
import { ProjectDialogs } from "./setup/ProjectDialogs";
import { WorktreeDialogs } from "./setup/WorktreeDialogs";
import { SwarmDialogs } from "./setup/Swarm";

// Setup dialogs: New session and everything it opens, session creation
// progress, clone and folder pickers, project dialogs, worktree overview and
// cleanup review, and the project swarm. Ids: "new-session",
// "worktree-cleanup" and "setup-*" (see each module).
export function SetupDialogs() {
  return (
    <>
      <NewSessionDialogs />
      <CreationDialog />
      <CloneDialogs />
      <ProjectDialogs />
      <WorktreeDialogs />
      <SwarmDialogs />
    </>
  );
}
