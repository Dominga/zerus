# Agent teams: how they work and how to use them

Zerus can run a team of agents instead of one session. A **lead** agent plans
the work; **members** take the tasks. The lead and the members may be different
agents (Claude Code, Codex, DeepSeek Harness, Kimi, OpenCode), on different
models and on different computers. You are the owner: nothing runs until you
approve a plan, and every decision that needs you is waiting in one place.

This guide is the English original. It describes teams as they are today. A
[Russian translation](ru/teams-guide.md) is available.

## What a team is

| Part | What it does |
|---|---|
| **Owner** (you) | Approves plans and phases, answers what waits for you, sets the rules (catalog, settings), and can stop the team. |
| **Lead** | Turns the goal into phases, tasks and dependencies; hands work to members; accepts, returns or reassigns reports; asks you for changes. |
| **Members** | Take one task at a time, work in their own agent session, report what changed and how they checked it, and ask the lead when blocked. |
| **Roles** | Named kinds of member (Backend, Frontend, Tester, Reviewer, Docs, ...). A task may ask for a role instead of a named member. |
| **Accounts** | Each session runs on one signed-in account of its agent. A team member's account changes only with **Swap member**; a session's account can be changed in its composer. |
| **Models and effort** | A role or a task may allow several models, and an effort for each. Zerus switches the member's model before the hand-out and waits for the confirmation. |
| **Computers** | A member may run on this computer or another machine Zerus knows. Work in a folder on another computer happens there. |
| **Folders** | Where members work: **Worktree per member**, one **Shared folder**, or **Auto** (the lead picks per member). |
| **Catalog** | The team's playbook: roles, templates, pipelines, planning rules and the default settings. The owner edits it; the Team designer can propose changes. |

### The team's folders

| Policy | When to use it | What you get |
|---|---|---|
| **Worktree per member** | A Git repository, one worktree per member. | Each member has its own checkout and branch; merges go through the lead. |
| **Shared folder** | A project too large to copy (a game with big assets), or a non-Git folder. | Members edit the same folder. Builds should use the **build broker**. |
| **Auto** | You want the lead to decide per member. | The lead chooses a worktree or the shared folder for each member. |

With **Branch per task** on, each task gets its own branch (for example
`task/{id}`) from the base branch, and a returned task keeps its branch.

## The life of a team

| Step | What happens | Your part |
|---|---|---|
| **Draft** | You start planning. The lead (or the Team designer) writes the goal, members, phases and tasks. Nothing runs. | Start it, read it, edit the goal or ask for changes. |
| **Planning** | You review the plan: phases, tasks, acceptance criteria, dependencies, pipelines and estimates. | Approve the phases (Approve & Run), or send the plan back. |
| **Running** | Zerus hands ready tasks to free members. Members report; the lead accepts, returns or reassigns. Pipelines run reviews and tests before the lead sees a report. | Answer what waits for you; the lead asks when a change, a permission or a question needs you. |
| **Paused** | You pause the team. No new hand-outs; running turns finish. | Resume when you want it to continue. |
| **Finished** | Every task is decided. The lead reports the result; a build may be the last task. | Read the summary, or extend the plan with a new phase. |

A team plans in one of two modes. **Standard** plans a release in phases: a
phase is approved, runs, and the next one starts. **Free** planning keeps one
open phase that takes tasks as they come; you close it when the stream ends.

### The owner's gates

- **Approve a plan or a phase.** Nothing runs before it. The lead asks with your
  own sentence, typed in its session, when a plan changes.
- **Answer what waits for you.** Proposals, permissions, questions, drafts and
  reports appear in **Waiting for you**; you answer in place.
- **Set the rules.** The catalog (roles, templates, pipelines, planning rules)
  and the team settings (limits, branches, intake, restarts, builds) are yours.
- **Stop.** Pausing or stopping the team is always yours.

## Getting started, step by step

### 1. New team

Open **New team** and fill in:

| Field | What to enter |
|---|---|
| **Name** | A short name for the team. |
| **Template** | Start from a saved template (Feature delivery, Bug hunt, ...), or **No template** and let the lead draft the team. |
| **Goal** | What should be delivered. One or two sentences are enough. |
| **Folder** | The repository or folder the team works in. **Browse...** picks it. |
| **Member folders** | **Auto**, **Shared folder** or **Worktree per member**. |
| **Lead** | The agent, account, model and effort of the lead. Each agent lists the models its account offers. |
| **Members** | Add members with **+ Member**: a role, an agent, an account, a model, an effort, and optionally its own folder. The limit is shown beside the list. |
| **Planning** | Who plans: the lead plans the work, or you plan it. |
| **Approval** | **Review plan before running** keeps every phase waiting for you. |

Press **Start planning**. The lead's session starts in the team's folder and the
draft appears. Nothing else runs yet.

### 2. Plan and approve

Read the plan in the **Plan** panel. Each phase lists its tasks with acceptance
criteria, dependencies, assignee or role, pipeline and estimate.

- **Approve & Run** starts the first phase. Later phases start when the phase
  before them is done, or earlier if you start them ahead.
- **Send back** returns the plan to the lead with your reason.
- **Extend** adds a phase; **Edit draft** changes a phase that has not started.
- Planning rules built into the catalog check the plan (missing criteria, a
  member with no free time, a role with no member) and warn you before you run.

### 3. Watch the work

The **Team** tab is the team workspace:

| View | What it shows |
|---|---|
| **Graph** | Tasks and their dependencies, in lanes by wave. The critical path and the members' load are visible. |
| **Board** | Tasks as columns by state: Pending, Active, Submitted, In review, In test, Accepted, Problems. |
| **Layers** | The same tasks row by row, one layer per dependency wave. Good for long plans and quick scanning. |
| **Profile** | Where the phase's time and tokens went: the critical path, each member's lanes, spans, top losses, and the estimate against the worked time. |
| **Tests** | The tests the team ran: the queue, the suites, their results (passed, failed, reused, passed on retry, flaky), durations and budgets. |
| **Builds** | The builds the team ran and their queue, with their results. |

- The **member panel** lists the members with their state, their task and
  their queue (including a stage queue for a member doing reviews or tests).
- A **task card** opens on click: state, attempt, stage chips, acceptance
  criteria, the report, findings, and the actions that fit its state.
- The **coordinator chat** at the bottom talks to the lead; you can answer a
  proposal there too.
- **Heat** tints the Graph and Board by time lost or by tokens, so the expensive
  parts of the plan stand out. **Hide finished** hides decided work.

### 4. Answer what waits for you

The bell in the left rail opens **Waiting for you**. It lists, across teams:

| Kind | What it is | What you do |
|---|---|---|
| **Plan / phase draft** | A phase that needs your approval. | Approve & Run, or send it back. |
| **Report** | A task report the lead escalated to you. | Accept, reject or hold. |
| **Your word** | The lead asks for a sentence to change the plan or the roster. | Answer in place; the change waits for it. |
| **Permission** | A member asks to run a command or use a tool. | Allow or Deny, with a reason. |
| **Question** | The lead or a member asks you to decide something. | Pick an option or write the answer. |
| **Proposal** | The lead proposes a change (a member, a task added to a running phase, a setting, a plan mode). | Allow applies exactly that change once; Deny sends the reason. |
| **Draft** | A team draft is waiting for you. | Start it, edit it, discard it. |
| **Held task** | The lead is holding a task for you. | Release it or leave it. |

An item answered in place leaves the list. A stale or already answered item is
refused, so nothing is applied twice.

### 5. Finish

When every task is decided the lead reports the result to you. A team may end
with an integration task and a build. The lead finishes the team when the work
is done; you can extend it with a new phase, or start a new goal from the same
result.

## Features, one line each

| Feature | What it is for |
|---|---|
| **Pipelines and stages** | A task passes review and test stages before the lead sees it: `code` (review, test, lead), `docs` (review, then accept), `check` (the lead decides), `none`. |
| **Stage verdicts** | A stage member passes or returns the task with findings; a return becomes the implementer's next attempt. After the maximum rounds the lead decides. |
| **Command and land stages** | The catalog may add a command stage (its exit code is the verdict) and one land stage (exit codes map to accepted, returned or held; needs your `land_allowed`). |
| **Add-to-phase and proposals** | The lead proposes adding, removing or swapping members, adding tasks to a running phase, cancelling tasks, discarding a phase or a setting change; you Allow exactly that change. |
| **Plan modes** | Standard plans a release in phases; free keeps an open phase that takes tasks as they come. |
| **Auto drafts and start-ahead** | With Auto-accept, a drafted phase stays a draft until its first hand-out; the lead can start it ahead when members are idle. |
| **Rebalancing** | A free member of the right role takes a ready task; a task for a role goes to a member whose models allow it. |
| **Stuck and crashed members** | A member that stops without a report, or crashes, is noticed; with `restart_crashed` it is restarted at most three times an hour, then held for the lead. |
| **Budgets** | A member may declare a time budget; the lead hears once when it is exceeded. |
| **Sub-teams** | A task may be delegated to a sub-team with its own lead, members and plan; its summary returns as the parent task's report. |
| **Model servers** | A computer on the network can serve models through an OpenAI- or Anthropic-compatible API; New session can start an agent on this computer wired to it, never changing your global configuration. |
| **Test discipline** | Only the tests a change touches run: the test map, the queue, reused results, flaky marks, per-suite budgets, remote runners, and a full run only before a release. |
| **Build cache** | Builds share a compiler cache (sccache and ccache), so a rebuild is fast across worktrees. |
| **Build broker** | Members sharing one folder build their own snapshots of it, so a half-written file never breaks another member's build. |
| **Test isolation** | Test templates run in their own folder and environment, with the person's paths fingerprinted before and after. |
| **Missing-tool offers** | A tool a test needs but the computer lacks is offered to you in Waiting for you; never installed by a member. |
| **Estimates** | The lead estimates each task at planning; the task card shows the estimate against the worked time, and the Profile compares every estimate with the work. |

## Scenarios

### A feature in a Git repository, with worktrees

Turn on: **Worktree per member**, the `code` pipeline, **Branch per task**,
**Affected tests** and the **build cache**.

Why: each member works in its own checkout, so parallel tasks never collide;
review and test run before the lead sees the report; a per-task branch makes a
returned task easy to rework; only the touched tests run, and the shared build
cache keeps each worktree's builds fast.

### A game project in one shared folder, with the build broker

Turn on: **Shared folder**, the **build broker**, **Test isolation**, and the
`code` pipeline with a test stage.

Why: a worktree per member would copy hundreds of gigabytes of assets. In one
folder, the broker gives every member its own snapshot: its own changed files
come from the working copy, another member's incomplete file is taken at HEAD,
and the build cache still serves it. The test stage still checks the change
before it reaches the lead.

### A docs or research team

Turn on: the `docs` pipeline (review, then accept) or `check`, **Auto** or
**Shared folder** folders, and a role for each kind of writer.

Why: there is little to build. A review stage is the useful gate; a named
reviewer reads the change against the criteria, and the task is accepted
without the lead in the middle. Roles keep the writers, the reviewer and the
editor apart.

### A long stream in free plan mode

Turn on: **Free** plan mode, the **intake key** for urgent work, and
**start-ahead**.

Why: a support queue never drains. A free team keeps one open phase and takes
each task as it arrives; a build is added when you ask for one, and you close
the phase when the stream ends. An intake key lets a health watch add an urgent
task without granting the lead any other right.

### A release with a full run

Turn on: **Standard** planning, a `code` pipeline with a land stage, **Branch
per task**, and the full-run policy set by you.

Why: a release needs the whole test set once, a review and test before the
landing, and a clean merge. The lead plans the phases; the land stage accepts
or returns by its exit codes; the full run is started by you (or the lead with
your word), because it is the one run that may be slow.

## Rules of thumb

- Approve small phases. Shorter phases mean faster feedback and less rework.
- Let the lead propose changes. A change you allow once is exactly the change
  the card shows.
- Keep an eye on **Waiting for you**: proposals, permissions and questions are
  answered there, never by a sentence in another chat.
- Use **estimates** and the **Profile** to see which phase was too big, which
  review cost the most, and which member was idle while work was open.
- When several members share one folder, turn the **build broker** on before
  the first build.
