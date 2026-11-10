Read and keep a team's memory (hgs guide team-lore)

A team whose memory mode is team, project or read-only has a RustCodeLore
workspace. Zerus writes it for the team: each phase's plan and its tasks'
outcomes, the owner's decisions, the faults of fixed findings and the
newest test readings, as records CodeLore's rules accept.

Search before you ask or diagnose (the code and docs themselves: hgs guide code):
  hgs team lore find WORDS [--team ID]   the workspace, the project's store
                                         and its team sub-stores; each hit
                                         names its store (project, team NAME)
  hgs team lore show ID [--team ID]      one record
After a compaction, recall the plan before anything else:
  hgs team lore plans [--team ID]        plans, counts and the open items
From a member's session --team may be left out. A sandbox cannot run lore:
ask the lead.

A record Zerus cannot complete (a commit, a cause, a fix or a check is
missing) waits in the lead's inbox with the command that writes it; it is
never written half-filled. The lead's inbox digest names the open items and
the newest decisions.

When the team ends its workspace is kept, read only and searchable. The
owner exports its faults, decisions and readings into the project's store:
  hgs team lore export --team ID

Plain memory has no workspace: the lead's inbox digest is the memory.
