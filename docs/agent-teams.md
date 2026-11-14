# Agent Teams

Agent Teams are a preview. They are off on every computer until a person turns
them on, and this guide describes the first milestone: a team that takes tasks
from a queue. The commands, their replies and their codes are in the
[Agent Teams reference](reference.md#agent-teams).

## What a team is

A team is a lead session and member sessions that work one queue of Zerus
tasks in one Git project. You send a task to the team. The Zerus desktop
service hands it to a free member of the right role, runs the review or test
stages of its pipeline after the member reports, brings the result to the
lead, and merges accepted work into the team's integration branch. You are
asked only for what is yours to decide.

A team task is an ordinary Zerus task. It stays on the project's task board,
keeps its history and attempts, and is reported with `hgs task` like any
other. The team adds who works on it, which stages it passes and who decided
it.

Use a team when a stream of independent tasks in one Git project should each
be implemented, reviewed and collected on one branch without you handing out
every task. For one task, an ordinary session is simpler. A team needs a Git
folder: every member works in its own worktree.

## Switching the preview on

Open **Settings**, section **Agent Teams**, and turn on **Agent Teams
(preview)**. The setting "Applies to this computer. Nothing runs until a team
is created." From a terminal outside any agent session, `hgs team preview on`
does the same, and `hgs team preview status` shows the state. When the desktop
cannot confirm a change, it says "The change could not be confirmed. Nothing on
screen changed." and offers **Read the state again**.

The switch follows this computer's command-line state. An unavailable read
keeps the switch disabled and shows its reason. **Read again** requests a fresh
answer even when the followed views and their contents have not changed.

Turning it on adds a marker file to this computer's `hgs` state directory and
makes the desktop show **New team** and **Waiting for you** in the rail, the
**Team** tab on a lead's session, **Send to team** on the task board and a team
chooser in the New task form. An `hgs task` command that touches a team, with
`--team` or on a task whose team key guards it, then proves its caller as
`hgs team` does: a command without session markers counts as yours only when
no live agent session runs above it, and a command inside a sandbox with its
own user namespace is refused. Every other task command behaves as with the
preview off. The team store is created only when the first team is created.

Turning it off while a team may still be live asks "Turn Agent Teams off?":
"Teams on this computer stop being served. Nothing is deleted." With the
preview off, `hgs team` answers `teams_inactive`, the entry points disappear,
and a team's tasks on this computer behave as ordinary tasks. A task whose
team lives on another computer still refuses agents.

## Starting a team

Press **New team** in the rail. The dialog starts from a **Template**:

| Template | Roster besides the lead | Default pipeline | A queued task starts |
|---|---|---|---|
| Feature delivery | two developers, a reviewer, a tester | `code` | after the lead routes it |
| Bugfix | a developer, a reviewer | `reviewed` | at once, with a free developer |
| Small | a developer, a reviewer | `reviewed` | after the lead routes it |

Fill in **Team name**, **Goal** and **Project**. Under **Settings** the dialog
shows everything the template brings, for you to confirm:

- **Roster**: each role and how many members take it.
- **Pipelines**: each pipeline the team uses, its stages, and who accepts its
  work: "the lead accepts", "you accept" or "it accepts by itself".
- **Integration**, **Queue**, **Routing** and **Land stage**.
- **Permissions**: "What the lead and the service may do without you." Each is
  a checkbox that starts as the template sets it; the built-in templates turn
  all five on.

| Permission | What it lets happen without you |
|---|---|
| **Accept results** | "The lead may accept a team task's result without you." |
| **Answer members** | "The lead answers members' questions and permission requests without you." |
| **Confirm folder trust** | "Zerus confirms the trust prompt of a folder it created for the team; on the member's account the agent records the trust for the team repository's main checkout and its subfolders." |
| **Resume exited members** | "Zerus resumes a member whose agent exited by itself." |
| **Merge accepted work** | "An accepted result is merged into the integration branch without your approval." |

Under **Project folder**, press **Check folder**: Create team stays blocked
until the folder is a Git folder ("A Git folder at %1, base commit %2."). That
commit is the team's base. **Integration branch** proposes `zerus/team/`
followed by the team's name; you can change it.

"Nothing is created until you press Create team." The team is created from
exactly what the dialog showed you:

- **Create team** sends the template you saw, identified by a digest of
  everything a team takes from it, and the five permissions as you set them.
  The team gets those permissions in place of the template's.
- When the template changed after the dialog showed it, nothing is created:
  "The template changed since you saw it, and nothing was created: review it
  as it is now, then create the team again." The dialog shows it as it is now.
- A template that asks you to confirm a permission or a setting this version
  does not know can be neither created nor saved: "This version does not know
  every permission of the template. Update Zerus to create a team."
- When the result of Create team is not known, the dialog says "The create's
  result is unknown. Check it with the same request, or dismiss it."
  **Check** asks again with the same request, so it can never create a second
  team.

`hgs team create` at a terminal may leave out the digest and the permissions;
the team then takes the template as the catalog holds it at that moment.

Team and template names may use ordinary Unicode letters. Creating a team,
saving a catalog or importing a template refuses control characters, line
separators and bidirectional formatting characters in names. Template, role,
pipeline and member IDs use a restricted ASCII spelling; the
[reference](reference.md#agent-teams) gives the rule. Older teams and catalogs
remain readable; a new save or import must meet the admission rules.

Once the team is created, the desktop service:

- creates the integration branch at the base commit, without checking it out;
- launches the lead, then each member in its own generated worktree, linked
  as a child session of the lead where its agent allows that. A member with a
  durable session and run can be linked before its first conversation;
- picks each session's account at launch: by default the signed-in account of
  the role's agent with the most room left in its usage windows.

With the preview on, Sessions groups a team under its name, with the lead first
and the other members ordered by role and member ID. Each row shows its role.
Membership uses the team roster's computer and launch/run bindings, including
when a parent link is pending; session names do not establish membership. A
live, quiet member without its first conversation shows **Waiting for a task**.
Ordinary sessions and the Sessions list with the preview off keep their usual
status and grouping rules.

A member that cannot be started becomes a card in Waiting for you that says
why and whether Zerus tries again. When Zerus does not know whether a member
started, it checks before it starts anything again. **Save as template** keeps
the template under a name of yours in your own catalog, with the permissions
as you set them.

## When a change does not go through

A change you make on the Team tab, in Waiting for you or with `hgs team`
either goes through or ends in one of three ways:

- **Refused by a rule.** Nothing was written. The desktop shows the team's
  reason; the command line answers with a code and a sentence, for example
  `stale_revision` when the team changed after you read it. Read the team
  again before you decide anew.
- **The store is busy.** "The team store is busy. Nothing was written; try
  again." Nothing changed, and you may simply try again. A command waits 8
  seconds for the store before it answers this way.
- **The result is not known.** The write may have happened or not. The
  command line says "The result is not known; repeat the same request with the
  same request ID." The desktop keeps the change open with **Check**, which
  repeats it with the same request, and **Dismiss**. A repeated request ID
  never applies a change twice, so a change is never sent again as a new one.

## The Team tab

The lead's session has a **Team** tab. Its bar shows the team's status, its
active members, open inbox items and open cards, and the team's actions:
**Pause** or **Resume**, **Stop**, and **Archive** after a stop. Destructive
actions ask for confirmation first.

**Members** lists each member with its role, what it works on ("Working on
#%1", "Reviewing #%1", "Testing #%1") and whether its session is linked. A
member's actions are **Pause**, **Resume**, **Stop** and **Remove**; the lead
cannot be removed. Removing a member puts its open work back to the lead.

A member whose terminal holds input, such as an open question, an unsent
draft or a dialog waiting for the person, takes no task: it is a fact about
the member, not a failure, with one typed cause for all of them. The
service hands the task to another free member, and tries the member again
after one, two, four, eight and then fifteen minutes; a ready task that only
that member can take gets a card after the wait. A member the service cannot
read at all counts the same way, and a ready task that needs only such a
member gets one card that names it.

The **Board** groups the team's tasks, with the queue as its own group:
**Queue**, **In work**, **Submitted** for reported work, a group for each stage
such as **In review** or **In test**, **Accepted** and **Cancelled**. A task
already handed out stays held until its observation moves; it takes no free
member from a task that waits. When no member can take a ready task, the lead
gets one inbox item for the episode, and no second one while it lasts. A card
shows the task's role, pipeline, member, round and stages; a stage card says
which task it checks ("checks the work of %1"), and a card that waits for a
decision says "Decision needed".

A card's decisions are **Admit** for a task that waits for admission,
**Accept** and **Return** for reported work, **Hold** or **Release**, and
**Cancel task**. Return and Cancel task ask for a reason. Reassign and Route
are commands (`hgs team reassign`, `hgs team route`); Route cannot move a
stage, whose member is chosen by the hand-out. **Add task** opens the
project's task board.

On a stopped or archived team's board, a task also shows what the current
stop recorded: cancellation, release, key clearing, a refusal and its reason,
or work not yet reached. An older stop without a recorded identity shows no
invented outcome.

## Sending a task to a team

- On the task board, a task in Backlog or To do has **Send to team: %1** in
  its menu, one entry per team of its project.
- The New task form has a **Team** chooser ("Send the new task to a team's
  queue").
- From a terminal: `hgs task add --project PROJECT --team TEAM < prompt.txt`
  for a new task, `hgs task target REF team TEAM` for an existing one.
  `--role` and `--pipeline` route the task as you send it.

The task stays an ordinary task with a `team` key that names its team and
home computer. A task you send, or the lead sends, is admitted at once. A task
sent by a member or by another agent waits until the lead admits it. The queue
is taken in the order of the board's To do column, and each member works on
one task at a time.

When the team refuses a new task, for example because its queue is full, the
task is still created but not queued: it stays on the board as an ordinary
task, and its team key is left as it was before the send. The desktop says
"The task was created, but %1 did not queue it: %2" and offers **Send to
team** again or **Dismiss**; the command line says "Task REF exists and was not
queued (CODE): REASON". When the result is not known, the task keeps its team
key and the desktop says "The task was created; whether %1 queued it is not
known. Send to team queues it now, and never twice." Only a task in Backlog or
To do can be sent.

## Waiting for you

**Waiting for you** in the rail counts the open cards of every team on this
computer. The page lists "Cards only you can answer, oldest first." Each card
shows its kind, its team, its age, its text and its decisions; **Open task**
opens the task on the team's board.

A card offers the decisions it accepts. **Integration** and **Work
acceptance** take **Approve** or **Reject**. **Acknowledge** closes a card and
changes nothing else.

| Card | When it appears |
|---|---|
| **Integration** | The lead accepted a result, and merging it waits for you because Merge accepted work is off. **Approve**: the service merges it and finishes the task. **Reject**: no new merge is authorized; the task stays in review. A merge already recorded is not undone and opens the card below |
| **Rejected result is merged** | A result reached the integration branch before its Integration card was rejected. The card and the lead's inbox name the task, commit and branch. Rejection did not undo that merge; only a revert on the integration branch removes it |
| **Work acceptance** | Every stage of a pipeline whose acceptance is yours has passed. **Approve**: the task is accepted, and merged where the team integrates. **Reject**: the task goes to the lead |
| **Member not started** | A member could not be started; the card says why and whether Zerus tries again |
| **Start not confirmed** | Whether a member started is not known; Zerus checks before it starts anything again |
| **No room for members** | New members wait until there is room for another session or account |
| **Member needs you** | A member's session asks for something Zerus does not confirm on your behalf |
| **Team service blocked** | A step of the service failed three times, for example a merge that cannot run, and is tried again; the same card names a member a ready task needs that cannot be read. It also names the cause when an open hand-out's holder has no durable session handle or its session record cannot be read: after the same three-minute period, with the same deduplication and close on recovery or settlement. An undecidable binding never counts as a departure; decisively ended bindings keep the existing left-member settlement. It is also the card for the project limit: when the project's own settings refuse every hand-out, one such card per team appears at the first refusal, its text names the limit, and the queue waits until the project allows it |
| **Report waiting** | A member's report has waited for the lead's decision longer than the team allows, an hour by default |
| **Lead not working** | The lead has not worked for a while, 20 minutes by default, although items wait in its inbox |
| **Late session** | A member's session started after the team stopped or the member was removed; Zerus does not stop it |

The page also lists **Requests of team sessions**: a question or a permission
request of a lead or a member that only a person can answer. Choosing one
opens that session.

## The lead's decisions

The lead reads its inbox and decides each report the pipeline handed to it.
The lead decides and the service carries the decision out. A task has one
decision at a time.

A new Return, Reassign or Cancel can replace an earlier Return or Reassign
only before its reopen is reserved. Other unsettled decisions must finish
first; the queue says whether its pending decision can be replaced.

The member's report text stays separate from **Service notes**, such as
"The report text was shortened" or "The member's worktree had uncommitted
changes." The service keeps at most 15,000 bytes of report text. In prompts
for the lead, a stage or a dependent task, each line of the member's text is
quoted and the service notes have their own heading. Text written by a member
does not become a service note.

- **Accept.** With an integration branch, the task is Done once the service
  has merged it, or once you approved its Integration card. A merge conflict
  returns the task to its member with the list of conflicting files. Without
  an integration branch the service moves the task to Done at once. The
  service finishes or merges only while the task is still as the lead accepted
  it; see [You and the team on one task](#you-and-the-team-on-one-task).
- **Return**, with a reason. The service reopens the task to the same member
  with the reason, which starts a new round.
- **Reassign**, to another active member of the same role, with a reason. The
  service hands the task over to that member.
- **Hold** and **Release.** A held task is not handed out.
- **Cancel**, with a reason. Running work is cancelled, and its member gets
  the ordinary cancel notice of a task; the task leaves the queue once the
  service has seen it cancelled. A task with nothing running (Backlog, To do or
  In review) leaves the team and stays in its stage as an ordinary task. A task
  whose delivery is unknown, and a Done task, are left as they are, and the
  lead gets an inbox item about it.

A lead can accept only work its pipeline handed over, and cannot accept or
cancel a stage: a stage ends by its holder's verdict or by you.

**A member's question.** A member whose task is blocked asks with
`hgs team ask`, and the question waits in the lead's inbox. With Answer
members on, the lead answers with `hgs team reply`. The answer stays with the
question: the member reads it in `hgs team current`, which shows the member's
own questions about the task it now holds, newest first, with their answers.
A question from an earlier round is not shown there; what the next round must
know goes into the reason of a Return.

## Pipelines and stages

A pipeline says what happens between a member's report and the end of a task.
`code` has a review by a reviewer and a test by a tester, then the lead
decides; `reviewed` has a review, then the lead decides; `docs` has a review
and is accepted when the review passes; `check` goes straight to the lead.
Each stage is a task of its own, handed to a free member of the stage's role
and never to the member who implemented the work.

The stage's holder answers with a verdict: pass, or return with findings. A
return reopens the task to its implementer without the lead. After the
pipeline's rounds, three unless it sets another number, the task goes to the
lead. Every reopen counts as a round, whoever caused it.

## Doorbells and timers

The inbox is where the lead's work waits; a doorbell only tells a session to
look. When items wait, the service sends the lead one message that says how
many wait and names `hgs team inbox`. When answers to its questions wait, a
member gets one message that says how many and names `hgs team current`. A
doorbell names a count and a command and never the text of an item, a question
or an answer.

A doorbell rings only while the team is running and the session is live, idle
and has no pending request, and only for something that arrived after the
previous doorbell. It rings at most once per interval, 120 seconds unless the
team sets another, and goes to the exact run and conversation the service
read. A doorbell whose delivery is not known is never sent again; the next new
item rings a new one, and a resumed session gets a new doorbell, never the old
one. A member's doorbell is dropped once the member no longer holds that
attempt: after an accept, a return, a reassignment, a cancel or a stop.

Timers turn waiting into an inbox item for the lead or a card for you:

| After | When | Becomes |
|---|---|---|
| 120 seconds | a member's turn ended on a task in progress without a report | an inbox item |
| 180 seconds | a handed-out task has not started | an inbox item |
| 15 minutes | a report waits for the lead's decision | an inbox item |
| 60 minutes | that report still waits | a **Report waiting** card |
| 20 minutes | the lead has not worked while its oldest inbox item waits | a **Lead not working** card |

The times are the team's settings. A timer fires once for its task and round,
or for its inbox item, and only while its condition still holds: what ends the
condition also closes the card it opened. No timer fires while the team is
paused or stopped. After a restart of the desktop service a timer may fire
late, never early.

## You and the team on one task

A team's task stays yours as well: you can start, cancel, close or move it on
the task board at any time, and the team follows what you did.

- **You start it yourself.** The team treats the run as yours: it ends its own
  attempt, settles any decision that was waiting, and goes on from your run as
  a new round.
- **A team's cancel never ends your run.** A cancel acts on the run the team
  knew. If you started the task again before the cancel was carried out, the
  cancel is settled and your run goes on.
- **Two runs at once.** While two runs claim the task, the team decides
  nothing about it: it finishes and merges nothing, and no cancel ends it,
  until one run remains. A task that has ended, cancelled or Done, counts as
  ended whoever ran it.
- **You take it out.** Retargeting the task away from the team takes it out of
  the queue; the team does not take it back.

## The integration branch

The integration branch collects accepted work. The service merges with Git
plumbing and moves the branch only if it still points where the merge
started. It never checks the branch out, never runs `reset`, never writes the
primary checkout or another branch, and never pushes. No hook runs on these
merges. Integration is unavailable while the integration branch is checked
out in a worktree, with Git older than 2.38, and in a repository that has
submodules, is a partial clone, or declares a hook or a merge driver in its
own Git configuration; the service then merges nothing, and the third failed
attempt opens a card in Waiting for you. Your own merge driver and signing
settings are honoured. Landing the branch on your base branch, or pushing
it, is your own step.

The permission **Merge accepted work** (`merge_accepted`) decides who starts a
merge: the lead's Accept while it is on, your approval of the Integration card
while it is off. Switching it off takes back every accepted result that has
not merged yet and that you did not accept or approve yourself: each goes back
to waiting for you with an Integration card.

## Pause, stop and archive

- **Pause** stops hand-outs and doorbells and sends nothing to any session;
  no timer fires while the team is paused. A paused member takes no new task.
  The lead can pause and resume the team and its members, but cannot resume
  what you paused or stopped.
- **Stop** a member: it takes no new task, and neither the lead nor the
  service resumes it. **Remove** takes it out of the team.
- **Stop** the team: every unfinished task is cancelled as after a lead's
  Cancel. Running work is cancelled with the ordinary notice; a task with
  nothing running stays in its stage; a task whose delivery is unknown, and a
  Done task, are left as they are; a task you started yourself after the stop
  is left to you. The service then removes the team key from every task that
  was open at the stop; each stays on the board as an ordinary task.
  In bounded background passes it also clears this team's keys from closed
  tasks that have no live team binding, without adding a history entry.
  A key for another team or a task bound again is kept. Archive also starts
  this closed-task cleanup.
- A member whose session starts after the team stopped, or after you removed
  it, is not taken into the team: a **Late session** card names it.
- **Archive** follows a stop and closes the team's open cards.

Zerus never stops a team's agents by itself. The sessions stay, and you end
them as any other session. Quitting a member's agent by hand is not a stop of
the team or of the member: with Resume exited members on, the member is
resumed. Use Stop on the member instead.

A cancelled task can be moved from In review or Done back to To do and sent
to a team again. Moving it back clears its cancelled mark and reason; a send
also clears an older stale mark in Backlog or To do. An attempt still being
cancelled is settled before the send creates its new queue entry; a task
still in In review cannot be resent.

## Project limits

A hand-out is written with the lead as its actor and the member as the
executor, so the project's task settings for agents apply to it as to any
agent. With **Agents may start and cancel tasks in other sessions** off, or
with the hourly limit of agent starts reached, the refusal is a fact about the
team, not about a task or a member: the whole queue waits. One card in Waiting
for you, shown as **Team service blocked** and bound to no task, opens at the
first refusal and says that the team's tasks wait and are handed out by themselves when the project allows it; its
decisions are **Retry**, which checks on the next service pass, and
**Acknowledge**. While the fact stands the service hands nothing out, and it
tries one hand-out as a probe after one, two, four, eight and then fifteen
minutes; a probe the project admits ends the fact and the queue goes on. The
lead sees the fact among the service items of `hgs team status`. An hourly
limit hands the task out by itself at the first probe after the hour passes,
nobody doing anything. A refused hand-out is never forced.

## What the service confirms by itself

Two permissions of the team, **Confirm folder trust** (`confirm_trust`) and
**Resume exited members** (`resume_exited`), and nothing else, let the service
answer for you:

- **Folder trust.** The startup folder trust prompt of a Claude, Codex or Kimi
  member whose worktree Zerus created for this team is answered at once.
  On that member's account the agent records trust for the team's main Git
  checkout and its subfolders. Codex may name that repository root in its
  prompt; the service verifies it against Git rather than trusting the text.
  **Settings > Approvals** records the scope: only a confirmed grant says
  "Trusted for %1 and its subfolders on this account"; failed, cancelled or
  unconfirmed attempts say trust was requested but not confirmed.
- **Resume.** A member whose agent exited by itself is resumed in the same
  conversation, at most three times an hour, with a short brief that points
  it at `hgs team current`.

Each answer is checked against the exact run, conversation and prompt,
recorded in the approval journal before delivery, and never repeated when its
delivery is unknown. Any other prompt, a folder Zerus did not create, and a
member you stopped go to you. These confirmations do not turn on automatic
tool approvals.

## Limits of this milestone

- Linux and macOS only.
- On macOS the system hides the environment of its own programs, among them
  the shells and `ssh`, from other processes. A person's command started below
  such a program, for example in `tmux`, over `ssh` or in a terminal inside
  another application, is refused: "On macOS a person's team command cannot
  be checked below a program that hides its environment, usually a system
  shell; run it from the Zerus window." Commands that a lead or a member runs
  are not affected.
- A team lives on one computer: its lead, its members and its Team tab.
- Only a queue: no planning from a goal, no phases and no Graph yet.
- DeepSeek can be a lead or member with a host that proves each tool call
  with its per-call token, witnesses task input and reports its lifecycle.
  A host lacking those capabilities stays unavailable; Zerus never reloads
  or restarts a live host to add them. OpenCode's shared-service sessions
  cannot be a lead or member in this milestone.
- On Linux, `hgs team` from a shell inside a sandbox with its own user
  namespace is refused, whether a person or an agent runs it, so a member
  whose agent runs its commands in such a sandbox cannot take part.
  A private process namespace also prevents the caller proof from seeing
  the host outside it: the refusal names that limit and the remedy. Run team
  commands outside that sandbox or turn its private process namespace off.
- A person's command is recognized by an interim rule: no live agent session
  may be among its process ancestors. It guards against accidents and against
  an agent that merely drops its session markers, not against one that sets
  out to pass as you. Every reply to such a command carries a note that
  states what this rule cannot detect.
- An accepted or cancelled queue entry is already decided and takes no
  Cancel. A task's later run must be observed as its own current attempt;
  the team's cancellation never takes authority over an unrelated run.
- Closed-task key cleanup is a bounded sweep. A task document that cannot be
  read is skipped, and a key synchronized back behind the cursor or after
  completion is not swept again until another Stop or Archive. A person may
  retarget a task whose old key remains; live bindings and other teams' keys
  are never cleared by this sweep.

## Origin

Agent Teams were designed and first built by WhiteWh in the `WhiteWh/zerus`
fork; pull request #322 is its first version. Zerus keeps its concepts, its
working process and its desktop views, and builds the runtime on Zerus tasks
and the desktop service. Files imported from the fork keep WhiteWh as their
Git author. See "Origin and credit" in the
[design](superpowers/specs/2026-11-11-agent-teams-design.md#origin-and-credit).
