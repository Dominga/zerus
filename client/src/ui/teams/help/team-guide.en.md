# Working with a team in Zerus

A team brings several agents together around one plan. The lead breaks the work into tasks, assigns them and accepts the results. You set the goal, answer questions and decide where the team may act independently and where it must wait for you.

## Prepare the plan

Prepare a good plan in a regular session before creating the team.

1. Discuss what you want to achieve with an agent.
2. Ask it to write a Markdown plan: the goal, constraints, acceptance criteria and anything it must leave alone.
3. Run an **independent cross-review with another agent**, preferably a stronger one. Ask it to find gaps, questionable choices and risks, then have the author revise the plan.

A strong mid-frontier model can lead the team: Opus, Codex or a capable alternative such as Kimi or GLM. Reserve a stronger model, such as Astra or Fable, for reviewing the plan. The point of cross-review is an independent assessment: the reviewer should challenge the plan rather than simply agree with its author.

A prompt for the reviewer:

> Independently cross-review this plan. Check completeness, dependencies, acceptance criteria, risks and unnecessary work. Identify concrete problems and propose fixes. Have the author update the plan after the review.

Give the finished file to the lead. A clear description of the result means fewer questions along the way.

## Create the team

Fill in the goal, project folder and lead settings. **Start planning** creates a team draft and starts only the lead, in the project folder. It does not create the other members or their working folders yet.

Give the lead the plan and ask it to design a team for the work. It proposes members, roles, accounts, models and tasks, and asks you about anything missing.

Check the lineup, folder policy and memory mode. **Create team** creates the members and their working folders. Tasks start after plan approval, either manual or through enabled automatic approval.

## Example team lineups

Start with a lineup where every role has useful work. Add members when the plan offers genuinely independent tasks.

### A feature in a regular Git project

- **Lead:** Opus, Codex or a suitable Kimi/GLM model. Plans the work, answers questions and accepts reports.
- **Two developers:** work on independent modules in separate worktrees.
- **Reviewer:** Astra or Fable for difficult decisions and independent review.
- **QA:** checks acceptance criteria and user scenarios. For a small change, another checking role may cover QA too.

Settings: **Worktree per member**, review and test pipeline stages, affected tests, the merge queue and the build cache. This lineup fits work with two development streams and enough review capacity to keep up.

### A large game project in one shared folder

- **Lead:** divides the work by concrete files and modules.
- **Two or three developers:** each edits an agreed area.
- **Reviewer and QA:** check changes, integration and behavior in the editor.

Settings: **Shared folder**, the build broker and test isolation. Unreal normally uses the broker's shared build mode. If all tasks touch the same module, reduce the number of members writing at once.

### A small task or a limited budget

- **A lead.**
- **One implementer.**
- **An independent reviewer** for the plan and final result.

This lineup is easier to coordinate. Bring the strongest reviewer in at specific points: plan cross-review, an architectural decision or the final check. For documentation and research, use a review pipeline; add builds and test stages where they can actually check the result.

## Working folders: separate or shared

Choose the policy in **Member folders**. It affects how members edit files, build the project and combine their results.

| Option | When it fits | How it works |
|---|---|---|
| **Worktree per member** | A regular Git project that benefits from independent parallel work. | Each member gets its own checkout and branch. Changes land on a common integration branch. |
| **Shared folder** | A very large project with heavy assets, one shared editor or a non-Git folder. | Members see and edit the same files. Divide responsibilities and coordinate builds. |
| **Auto** | You want the lead to choose. | The lead chooses per member: for example, developers get their own folders and reviewers use the shared folder. |

### Working with worktrees

A worktree is a separate checkout of the same Git repository. Git history is shared, but every member has its own current files and branch. Project assets and build outputs may still consume substantial disk space in each folder.

A member completes its task, checks the result and reports with a commit. With the **merge queue** enabled, accepting the result queues it for integration: the queue applies the commit, runs affected tests and reports success, a conflict or a failure. New work starts from the current integration line.

Enable **A branch per task** to give each task its own branch. Otherwise a member continues on its existing branch. A shared compiler cache helps reuse compiled work between checkouts.

A prompt for the lead:

> Developers use separate worktrees. Divide tasks by module, arrange review and affected tests before integration, and land results through the merge queue.

### Working in a shared folder

Everyone sees each other's edits immediately. Avoid having two agents rewrite the same file at once. The lead should name the concrete files or modules each task writes and connect overlapping tasks with dependencies. A broad area such as “all of src” does not replace that coordination.

Enable **Build broker for members sharing a folder** in team settings. **Test isolation** is useful too: each test run gets separate temporary folders and an environment of its own.

A prompt for the lead:

> We work in one shared folder. Divide the files between members and connect overlapping tasks with dependencies. Send builds through the build broker and tests through the configured templates and queue. Each member lists its changed files in its build request.

## What the build broker does

When members build in one folder, concurrent builds may write to the same output directory. One agent's build may also pick up another agent's unfinished edits. The build broker organizes these runs and reduces interference.

It is **off by default**. Turn it on in team settings when several members share a folder and build the project. The lead may also propose it on a card for you to approve.

The broker has two modes.

### Snapshot builds

The broker prepares a snapshot for a member: tracked sources at **HEAD**, with that member's changed files, listed in the build request, applied on top. Other members' unfinished edits stay out of that snapshot.

Sources are copied while preserving incremental builds. Large asset folders can be linked to avoid copying every asset. Builds sharing one build directory are queued. Where the inputs match and result reuse is allowed, a repeated check may use an earlier result.

Use this mode when members share a source folder but need to check their own changes independently. If a task depends on another change, put that change on the agreed base first: the snapshot does not automatically include a neighbor's uncommitted edits.

### Builds in the shared folder

For Unreal and projects where snapshots cost too much or an open editor needs the build, the broker builds directly in the shared folder. There is no second Intermediate directory or separate project copy.

Builds run one at a time. Before starting, the broker waits for the other members to declare their edits ready by requesting a build. The wait is bounded: **300 seconds by default**. After that, it may build and explicitly name unfinished changes included in the run.

Members should therefore request a build when their files are in a coherent state and list those files in the request. This mode orders builds, while the build itself uses the shared files as they stand.

Follow the queue and results in **Builds**. Members with independent worktrees usually do not need a shared-folder broker.

## Work modes: Standard and Free

Choose the mode **per phase**, in its header. A team can have Standard phases alongside one open Free phase.

### Standard: work in phases

The lead divides the plan into phases with clear goals, usually up to 20–25 tasks each. A phase should finish with an installable build and **NOTES.md**. Review and QA happen at planned checkpoints within the phase.

A new phase starts as a draft. Read it and press **Approve** when it looks right. To approve prepared drafts automatically, turn on **Auto-accept drafts**. Automatic approval applies when the plan passes the required checks and has no blockers.

Approved phases may run in parallel: a free member takes a task when its dependencies are ready and its files are available. If a phase must wait for all earlier work, make that an explicit barrier in the plan.

### Where the team waits for you

A phase also has result acceptance:

- **Your acceptance — a hard gate.** You check the result, accept the phase or send it back with a comment. While a phase awaits your acceptance, later phases do not start.
- **The lead's acceptance — a soft gate.** The lead accepts the result and work continues without your manual check of that phase. For automatic progression, **turn on Auto-accept drafts** and leave result acceptance with the lead.

These are two settings: **Auto-accept drafts** approves the draft before work, while lead acceptance completes the finished phase. One does not replace the other.

The lead adds phases at your request. Changes to an agreed plan should also follow your request.

Problems found during work become:

- **Crit — critical.** Added to the current phase immediately, with a purple border and a **Crit** label. You receive a notification. An open Crit prevents phase acceptance and a release build.
- **Minor — noncritical.** Does not block work. Goes to the next phase or a separate small-fixes phase, which needs no manual approval and runs when members have capacity.

### Free: a stream in one open phase

If you want to run the current work as a stream, **tell the lead explicitly**:

> Work in Free mode: keep the current task stream in one open phase and add incoming tasks to it. Do not split this stream into a sequence of new phases. Build when I ask, and close the phase after my confirmation.

Switching one phase to Free does not by itself explain how you want the entire plan organized. **Free belongs to a particular phase**; other phases in the team may remain Standard. Only one Free phase may be open at a time.

Within that phase, you give tasks to the lead, which adds and distributes them to free members. It handles stuck or crashed members within the actions the team is allowed to take.

When there are temporarily no tasks, the phase stays open for new ones. You can ask for a build separately. Closing the phase requires an up-to-date build if new changes have been accepted since the last one.

## Tests and their queue

Run checks that can actually catch an error in the current change. Zerus selects them through the project's test map. Settings live in **.zerus/tests.toml**; without it Zerus can propose detected checks but does not enforce a complete configured policy.

The usual flow:

1. Finish the change and use **hgs test affected** to find its affected tests.
2. If those tests need a build, run the appropriate build template first.
3. Request the checks. **hgs team run** checks a member's working folder; the shared suite queue checks the integration line. These are separate mechanisms with separate results.
4. Report what actually ran, where it ran and how it ended.

### What the shared queue does

The queue considers available CPU, RAM and GPU against each suite's requirements. Forced requests come first, then shorter runs; two heavy suites never run at once. Runs estimated to exceed the suite's budget normally wait for the night window unless forced. Tests for another platform go to a configured runner; when no suitable runner exists, the queue shows why it is waiting.

If a suite allows reuse and its inputs match the last green measured run, its result may be **reused**. That means a saved result was used. Cargo disables reuse by default. **Force run** requests a fresh run, gives it priority and removes the night-window wait, but it still needs sufficient resources.

An ordinary failure may be retried according to the project's policy. A pass on retry is marked **flaky**; a crash is not automatically retried. The queue also tracks budget overruns and notable slowdowns.

**Tests** shows pending and running requests, waiting reasons, results, durations and available per-test details. A full suite is planned separately with an estimate and your permission under the team's policy. A member agrees any run longer than 10 minutes with the lead first.

### A prompt for the lead

> After each change, use hgs test affected to run only relevant checks. Build the required targets first, then send the printed run requests. Check integration through the queue on the common branch. Agree a full run with me separately. In reports, distinguish fresh runs, reused results, flaky passes and tests that could not run.

### Command examples

Selecting checks does not run them. From the project folder:

```powershell
# Read the project's settings.
hgs test config

# Select checks for named files. This example is from Zerus.
hgs test affected --files src/state/teams/test_queue.rs

# Select checks for a commit range. Replace BASE with your base branch.
hgs test affected BASE..HEAD

# Select checks for changes already staged in Git.
hgs test affected --staged
```

**affected** prints commands, working folders, time estimates and, for supported checks, ready-made **team_run** and **team_build** fields. The agent uses these with the identities of its team and current task attempt.

A member's focused Cargo request in PowerShell:

```powershell
$request = @{
    request_id = [guid]::NewGuid().ToString()
    team = 'TEAM_ID'
    attempt = 'ATTEMPT_ID'
    template = 'cargo-test'
    args = @{
        bin = 'hgs'
        filter = 'state::teams::test_queue::tests::'
    }
}
$request | ConvertTo-Json -Depth 6 -Compress | hgs team run --json
```

This example is for Zerus. The agent takes **TEAM_ID** and **ATTEMPT_ID** from its current task hand-out and chooses the binary and filter for the changed code. Every new run needs a new **request_id**. Repeating the same request with the same ID reads its result without running it twice.

Read the shared queue and recent results:

```powershell
'{"team":"TEAM_ID","action":"queue"}' | hgs team test --json
'{"team":"TEAM_ID","action":"results","limit":20}' | hgs team test --json
```

Submit a specific suite to the shared queue:

```powershell
'{"team":"TEAM_ID","action":"submit","suite":"rust","targets":["--bin hgs state::teams::test_queue::tests"]}' | hgs team test --json
```

Here **rust** is a suite name in Zerus's configuration. In another project, use its suite and mapped targets. The shared queue checks the integration folder, so the change must reach it first. **queued** means the request was enqueued; a separate result confirms successful execution.

## Memory: what the team keeps

An agent's conversation context is limited. Zerus saves the state of the work and can also keep knowledge in **CodeLore**, so context compaction does not mean reconstructing everything from chat.

### The team's system memory

Zerus stores the plan, tasks, attempts, reports, decisions, findings, run results and journal events. They belong to the team and survive the context of an individual agent.

In **plain** mode it generates a short digest: what was done and why, what remains open, decisions, findings and the latest checks. The lead receives this digest to recover its bearings. You can read it too:

```powershell
hgs team status --digest --team TEAM_ID
```

The digest is bounded and does not replace detailed reports. Members receive applicable rules, task descriptions and relevant decisions in their task hand-outs. Ask the lead to register important decisions explicitly: “Record this as a decision and bind it to the affected tasks.”

### CodeLore: accumulated project knowledge

CodeLore stores separate records: plans and outcomes, decisions, investigated faults, checks and measurements. For a fixed fault, preserve the whole chain: **symptom → cause → fix → verification**.

Zerus writes team knowledge automatically when all required information is available. An incomplete record becomes a question for the lead to fill in. Agents search for relevant records instead of rereading the entire history. After context compaction they can recall the plan, open items and decisions.

**CodeLore Atlas** also searches source code and Markdown documentation. It returns locations in files for the agent to read. Semantic search depends on the configured search model; word and symbol search are available separately. Documentation can also be prepared as memory records linking to the original sections.

Examples for the agent:

```powershell
# Find saved knowledge about a topic.
hgs team lore find "build broker" --team TEAM_ID

# Recall plans and open items.
hgs team lore plans --team TEAM_ID

# Read a matching record.
hgs team lore show RECORD_ID --team TEAM_ID

# Find implementation or documentation in the project checkout.
lore atlas search "build broker"
```

### Choose a Memory mode

| Mode | What it provides |
|---|---|
| **plain** | System state and a short recovery digest, without a separate CodeLore store. |
| **team** | A knowledge store for the team. Works when project memory is not enabled yet. |
| **project** | Team knowledge together with accumulated project knowledge. Available with project memory enabled. |
| **read-only** | Reads available memory without adding CodeLore records. |
| **off** | Basic state and the journal remain, but no memory digest or CodeLore is used. |

With project memory enabled, **project** is selected initially; otherwise **team** is selected. You can confirm a mode change while the team runs.

A finished team's store remains searchable. Moving useful knowledge into shared project memory is a separate **Export**, requested by you or a project rule. Future teams can then use its decisions and investigated faults.

## Follow the work

The **Team** tab offers several views of the same plan:

| View | Use it for |
|---|---|
| **Graph** | Phases, tasks and links. Solid lines show dependencies; dashed lines show origins. |
| **Board** | Tasks in status columns. |
| **Layers** | Tasks arranged by dependency layers. |
| **Lanes** | Work distributed across members. |
| **Profile** | Time, waits, rework, utilization and token use. |
| **Tests** | The test queue and check results. |
| **Builds** | The build queue and results. |

Use filters to focus:

- **All / Active / Problems** — all tasks, active tasks or problems.
- **Show cancelled** — include cancelled tasks.
- **Heat** — tint items by the selected metric.
- **Hide finished** — hide accepted tasks and completed phases.
- **Collapse all** — collapse every phase.

### What needs your decision

**Waiting for you** collects questions, permission requests, phase acceptance, new **Crit** findings and the lead's proposals. Open an item to read and answer it.

### Tasks, plan changes and members

A task card shows its description, acceptance criteria, attempts, report, tests and commit. You can accept the result, reject it with a reason, reassign the task or edit it.

The lead's independent plan edits appear in a separate feed. An edit can be undone while the affected task has not started.

Each member shows its current activity, or “pulse”. You can open its session, pause, restart or replace it.

## Read the profiler

**Profile** helps explain where time went and why work slowed down. Use it after a phase, before planning the next one or when everyone seems busy but the result is taking a long time.

### The summary

Start with four figures:

- **Elapsed** — wall-clock time.
- **Worked** — summed work by members, including rework and processing stages.
- **Accepted** — accepted tasks out of the selected scope's total.
- **Tokens** — token use that could be accounted for.

Elapsed and Worked measure different things. If three members each work for an hour in parallel, one hour passes on the clock and the sum may be three hours of work. A high Worked figure does not itself mean a slow team.

### Time: work and waits

The timeline shows members, tasks and work spans. It separates implementation, rework, review, tests and builds from waiting for dependencies, a free member, the lead, a permission or your answer. Waits also have distinct hatch patterns.

Select a phase or the whole plan, find a long span and read its cause. This distinguishes slow implementation from a task that spent most of its time waiting for review.

Compare the **planned and actual critical paths**: dependent task chains that determine completion. A delay on that chain may prevent free members on other branches from speeding up the outcome. If the actual chain differs, investigate the new delays.

**Top losses** lists substantial rework, decision waits, missing capacity, long checks and merge problems. Order the list by time or tokens.

A dependency is not automatically a loss: a task may be scheduled to wait for its predecessor. Check whether it actually delayed the planned start.

### Estimate, overrun and lateness

The profiler distinguishes two questions:

- **Overrun:** active work, including all attempts and rework, compared with its estimate. Waits are excluded.
- **Lateness:** how much later a task started or finished than planned. It separates delays inherited from predecessors from the task's own delay.

For example, a task estimated at 30 minutes waits an hour for a dependency, then takes 25 minutes. It may be late on the schedule while staying within its work estimate. Conversely, a task that starts on time but needs two hours instead of half an hour overruns its estimate.

These measures require estimates and a saved plan. Missing data is shown as unknown.

### Tokens: where tokens went

This mode shows known token use, role figures and losses whose tokens could be counted. **Tokens per accepted** helps assess the cost of an accepted result, including attempts.

Compare tasks and roles of similar difficulty. High use may reflect a hard problem, repeated review or unclear requirements. A single number cannot tell you a model is worse.

**partial** means some sessions are missing from the total. A dash means the data is unknown: the provider may not supply counters, a session may be on another computer or its record may be unavailable. It is not zero use.

### Members: utilization

Members show attempts, returns, work time, waits, the share of rework and available token figures. **Utilization** measures the share of the selected window spent working; waits are not work.

Low utilization is a reason to inspect the plan. A member may be waiting for dependencies, lack suitable tasks or be stuck behind one reviewer. Adding developers may not speed that up.

### Compare: two phases

Select two phases to compare elapsed time, task count, work, rework, waits and tests. This helps assess changes to the lineup or process. Account for differences in scope and difficulty.

Ask the lead:

> After the phase, name the profiler's three biggest losses. Explain their causes and propose changes for the next phase: smaller tasks, fewer waits, stronger review or a different lineup.

Save a profile summary with **Export JSON**. **Heat** in the plan helps locate tasks with large losses, lateness or overruns under the selected metric.

## Team settings

**Team designer** configures roles, models and accounts, build and test templates, and planning rules for the lead, such as the maximum number of tasks in a phase.
