Team customization: models, context, stages, branches, scale (hgs guide team-custom)

Every field is opt-in; the owner sets roles, templates, presets, settings and
the catalog, and you set a task's models, role and pipeline.

Models and a task for a role (U45): a task may name "models" (a model, or
model@effort; default: any) and "role" instead of an assignee - never both, and
a role task is not pinned. A member's "models" (default: its launch model) are
copied from its role when it joins. Zerus gives the task to a free member of
the role whose models allow one, switches model and effort with hgs settings and
waits for the confirmation; a refused or unconfirmed switch leaves the task
waiting (retry or reassign), never on the wrong model. A plan naming a role with
no member is an error. keep_assignees (off) pins every planned assignee.

The context before a task (U46): context_before_task on a role (or a member) is
none (default), compact or clear; before each hand-out, a stage's included,
Zerus compacts or clears the context and waits. A clear starts a new
conversation, so task texts must stand alone.

Stages Zerus runs (U47, U48): a pipeline may hold command stages (a command,
expect_exit 0 and timeout 30; no member - the exit decides) and one last land
stage (an exit map to accepted, returned or held; needs the owner's
land_allowed). Only the owner's catalog writes them; the plan picks the preset.
branch_per_task {template} and base_branch are the owner's settings: with a
template each task gets its own branch from base_branch (default: the lead's
branch); without one a member keeps one branch. A dirty folder waits.
Landing by hand: {"manual_line":"BRANCH"} | hgs team settings --json (the lead
may set it). Accept then queues nothing and runs no suites; you merge into
BRANCH and record it, {"task":N,"commit":"SHA"} | hgs team landed --json;
members merge BRANCH (git-merge-lead) and start from it.

Scale and drain are cards (hgs guide team-propose). restart_crashed (on) restarts a
crashed member 3 times an hour - also one whose exit interrupted a held task, and one
you resume after its agent exited; an exit typed in the member's terminal counts, so turn
it off before stopping a member by hand.
