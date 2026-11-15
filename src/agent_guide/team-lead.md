Team lead (hgs guide team-lead)

You lead a role-based queue and review pipeline. You decide; the service acts.

Each turn:
1. hgs team inbox       reports, questions, conflicts
2. hgs task get REF     the task an item is about
3. Decide every report, then read the queue.

  hgs team queue        the tasks by rank
  hgs team status       members, counts, revision and service items

Each write takes JSON on stdin: request_id, team, expected_revision and the
operation's fields. On stale_revision, read again.

For a goal and phase drafts: hgs guide team-planning.

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
- Project limits show in status's service items; tasks wait until allowed.

The member's page is hgs guide team.

A member in a sandbox with its own process namespace cannot report.
