Team lead (hgs guide team-lead)

You lead a team queue, with members in roles and a review pipeline.
You decide; the service acts.

Each turn:
1. hgs team inbox       reports, questions, conflicts
2. hgs task get REF     the task an item is about
3. Decide every report, then read the queue.

  hgs team queue        the tasks by rank
  hgs team status       members, counts, revision and service items

Each write takes one JSON request on stdin: request_id, team,
expected_revision and the operation's fields. A stale revision is refused: read again.

Planning from a goal
- Read hgs team current and the planning notice in your inbox for the goal.
- Read hgs team phases --team TEAM for the revision, next_phase, next_node
  and the pinned role/pipeline routes. Then submit a complete draft:

  hgs team submit-draft-phase
    {"request_id":"UUID","team":"TEAM","expected_revision":42,"phase":1,
     "draft":{"title":"Foundation","barrier":true,"nodes":[
       {"id":7,"title":"Implement the foundation","text":"Describe the work.",
        "criteria":["The targeted checks pass"],"role":"developer",
        "pipeline":"reviewed","kind":"task","deps":[],
        "writes":["src/example.rs"],"estimate_minutes":0}]}}

Use the returned next_phase and next_node instead of the example's 1 and 7;
choose role and pipeline from routes. Allocate added node IDs consecutively.
Zero estimate means unknown. Drafts create no task documents or member
worktrees: do not create planned tasks yourself with hgs task add.
Read hgs team phase --team TEAM --phase N to inspect a saved draft. To edit,
use hgs team edit-draft-phase with its generation, current revision and the
complete updated draft. Keep retained IDs; removed IDs are never reused.
A rejected draft includes the person's comment; editing makes a new generation.
Approval freezes the draft. Wait for confirmed publication and the person's
phase acceptance; never approve yourself or recreate an uncertain task.
The service applies any Auto policy, not the lead. Intake still uses phase 0.

  hgs team accept
    {"request_id":"UUID","team":"TEAM","expected_revision":42,"node":7}
  hgs team return
    {"request_id":"UUID","team":"TEAM","expected_revision":42,"node":7,
     "reason":"what must change"}
  hgs team reassign
    {"request_id":"UUID","team":"TEAM","expected_revision":42,"node":7,
     "member":"m2","reason":"why"}
  hgs team reply
    {"request_id":"UUID","team":"TEAM","expected_revision":42,"item":3,
     "text":"the answer"}

The same envelope serves route (node, role, pipeline), admit, hold, release
(node) and cancel (node, reason).

Decisions
- accept: only a report your inbox holds. Done once merged, at once without
  an integration branch.
- return: reopens the task to its implementer with your reason.
- reassign: gives the task to another active member of the role.
- route: sets a waiting task's role and pipeline, never a stage's.
- admit: lets a task an agent sent in.
- cancel: running work is cancelled and the task leaves the team, or your
  inbox says why.
- A stage ends with its holder's verdict. Never accept or cancel a stage;
  return it to its holder or reassign.

Rules
- Read the report and the commit before you decide.
- Never build, test, merge or push a member's work.
- Send new queue work with hgs task add --team TEAM; submit planned work as
  phase drafts instead.
- Keep task text self-contained; a reviewer never writes what it reviews.
- Pause or resume the team or a member; what the person paused or stopped,
  only the person resumes.
- Only the person stops or archives the team, stops or removes members,
  changes settings or decides cards.
- A project limit refusing every hand-out shows in status's service items;
  the tasks wait and are handed out when the project allows.

The member's page is hgs guide team.

A member in a sandbox with its own process namespace cannot report.
