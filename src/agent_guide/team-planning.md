Team planning (hgs guide team-planning)

Read the goal and the pinned routes before drafting:

  hgs team current
  hgs team phases --team TEAM

Use the returned next_phase and next_node, not the example's 1. Choose role
and pipeline from routes. Each write takes JSON on stdin; use a new request
UUID and the current team revision. On stale_revision, read again.

  hgs team submit-draft-phase
    {"request_id":"UUID","team":"TEAM","expected_revision":42,"phase":1,
     "draft":{"title":"Foundation","barrier":true,"nodes":[
       {"id":1,"title":"Build the foundation","text":"Implement the shared part.",
        "criteria":["The targeted checks pass"],"role":"developer",
        "pipeline":"reviewed","kind":"task","deps":[],
        "writes":["src/example.rs"],"estimate_minutes":0}]}}

  hgs team phase --team TEAM --phase 1

Edit with that draft's generation and the complete updated draft:

  hgs team edit-draft-phase
    {"request_id":"UUID","team":"TEAM","expected_revision":42,"phase":1,
     "generation":1,"draft":{"title":"Foundation revised","barrier":true,"nodes":[
       {"id":1,"title":"Build the foundation","text":"Implement the shared part.",
        "criteria":["The targeted checks pass","The regression is covered"],
        "role":"developer","pipeline":"reviewed","kind":"task","deps":[],
        "writes":["src/example.rs"],"estimate_minutes":0}]}}

Keep retained IDs. Allocate added node IDs consecutively from next_node;
removed IDs are never reused. Zero estimate means unknown. A rejected draft
includes the person's comment; editing makes a new review generation.

Drafts create no task documents or member worktrees. Do not create planned
tasks with hgs task add. Approval freezes the draft; wait for confirmed
publication and phase acceptance. Never approve yourself or recreate an
uncertain task. The service applies Auto policy, not the lead. Phase 0 remains
the intake queue. Your queue and report duties are in hgs guide team-lead.
