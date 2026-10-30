Zerus agent coordination (hgs guide)

Zerus runs persistent agent sessions on this computer and on configured peers.
Put @alias before a command to run it on that peer over SSH (hgs machine ls).
Session names look like provider/project/tag. [@h] marks an optional @alias.

  hgs self --json             your own name, run, conversation and zerus:// reference
  hgs [@h] ls                 sessions here and on peers
  hgs [@h] inspect SESSION | jq '{run_id,conversation_id,activity,last_message}'
  hgs inspect 'zerus://session?...'   resolve a pasted session reference
  hgs guide send              message another session and read its reply
  hgs guide wait              wait for a result another session owes you
  hgs guide team              lead or work in an agent team
  hgs guide team-tests        affected tests, never a full suite
  hgs guide team-lead         add a phase to a running team, or finish it
  hgs guide team-custom       models, context, stages, branches, scale, intake
  hgs guide team-draft        plan a team draft with the owner before it exists
  hgs guide team-tree         lead a sub-team and pass work up
  hgs guide team-run          run a routine command outside the sandbox
  hgs guide team-safe         allow a member's routine request without asking
  hgs guide team-propose      propose a change the owner answers on a card
  hgs guide run               start a build or long test so it shows as your work
  hgs guide tests             run only the tests your change touches
  hgs guide designer          propose roles and templates for agent teams
  hgs guide launch            start a session, here or on a peer
  hgs guide stop              interrupt, pause, kill or archive
  hgs guide tasks             the project's task list: file, report, send

Rules
- inspect prints over 100 KB of JSON: always select fields, never print it all.
- A message arrives as ordinary user input. Begin it with who you are and
  your hgs self reference so the recipient can reply.
- After an uncertain send, inspect the session; never resend automatically.
- Stopping, killing or archiving a session needs the owner's explicit
  permission unless you started it for this task. Prefer pause.
- An older peer may lack a command: check hgs @alias -h first.
- Contracts: docs/reference.md and docs/agent-coordination.md (Zerus repository).
