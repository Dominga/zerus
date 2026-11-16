Phase corrections (hgs guide team-rework)

The person, or the exact lead with Lead acceptance and accept_results,
can send a waiting phase back: phase, publication and a required comment.
The person confirms in the Zerus window; Person CLI writes, including @host,
return person_gesture_required. The exact delegated lead may use
send-back-phase through its existing session authority. Keep the phase's acceptance gate, barrier and policy.
Accepted tasks and commits are not reopened. Phase 0 remains intake.

Read the same phase again. Its rework.id and base_publication identify the
episode. submit-phase-corrections takes phase, publication, rework and a
complete draft (the shape is in hgs guide team-planning). Allocate added IDs
from next_node; never reuse accepted task IDs. Use the existing barrier.

Edit a submitted proposal with its current generation and retained IDs:

  hgs team edit-phase-corrections
    {"request_id":"UUID","team":"TEAM","expected_revision":42,"phase":1,
     "publication":"PUBLICATION","rework":"REWORK","generation":3,
     "draft":{"title":"Foundation corrections","barrier":false,"nodes":[
       {"id":1,"title":"Regression coverage","text":"Cover the missing case.",
        "criteria":["Accepted work stays unchanged","The failure case passes"],
        "role":"developer","pipeline":"reviewed","kind":"task","deps":[],
        "writes":["src/correction.rs"],"estimate_minutes":30},
       {"id":2,"title":"Check the correction","text":"Check compatibility.",
        "criteria":["Compatibility holds"],"role":"developer","pipeline":"reviewed",
        "kind":"task","deps":[{"node":1}],"writes":[],"estimate_minutes":30}]}}

publication means the BASE approved publication, not the new correction
publication. Preserve the exact episode, generation and revision shown by
the read. On stale_revision read again; do not silently rebase an old form.

Only the person approves/rejects corrections or undoes a rejection. Auto
uses the service under the existing policy. Do not approve yourself. The
phase returns to Approved only after every named correction task is saved
and publication is confirmed. Unknown delivery uses Check with its original
request; never recreate or replay it. Earlier unreturned versions are
unavailable history, not the current draft or current live estimates.
