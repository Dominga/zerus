Zerus agent coordination (hgs guide)

Zerus runs persistent agent sessions on this computer and on configured peers.
Put @alias before a command to run it on that peer over SSH (hgs machine ls).
Session names look like provider/project/tag. [@h] marks an optional @alias.

  hgs self --json             your name, run and zerus:// reference
  hgs [@h] ls                 sessions here and on peers
  hgs [@h] inspect SESSION | jq '{run_id,activity,last_message}'
  hgs inspect 'zerus://session?...'   resolve a pasted reference
  hgs guide send              message another session and read its reply
  hgs guide wait              wait for a result another session owes you
  hgs guide team              lead or work in an agent team
  hgs guide team-tests        affected tests, never a full suite
  hgs guide team-lead         plan, decide, ship
  hgs guide team-custom       models, stages, branches, scale
  hgs guide team-draft        plan a draft with the owner
  hgs guide team-input        member shape, --json-file
  hgs guide team-tree         lead a sub-team
  hgs guide team-run          routine commands
  hgs guide team-safe         allow a routine request
  hgs guide team-propose      propose a change on a card
  hgs guide team-lore         team memory: find, plans, export
  hgs guide code              find code and docs: lore atlas, lore find
  hgs guide team-park         park a task, never poll
  hgs guide team-broker       builds in a shared folder
  hgs guide team-cpp          CMake and MSBuild runs
  hgs guide run               start a build or test as your work
  hgs guide tests             run only the tests your change touches
  hgs guide designer          propose roles and templates
  hgs guide launch            start a session here or on a peer
  hgs guide stop              interrupt, pause, kill, archive
  hgs guide tasks             the project task list

Rules
- inspect prints over 100 KB of JSON: always select fields.
- A message arrives as user input: begin it with who you are and your
  hgs self reference.
- After an uncertain send, inspect the session; never resend.
- Stopping, killing or archiving a session needs the owner's explicit
  permission unless you started it. Prefer pause.
- An older peer may lack a command: check hgs @alias -h.
- Contracts: docs/reference.md, docs/agent-coordination.md.
