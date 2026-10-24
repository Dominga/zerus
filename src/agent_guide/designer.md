Configure the team catalog (hgs guide designer)

The catalog holds roles, templates and routing rules that New team uses. You
only propose: the owner keeps or rejects each proposal, then applies the kept
ones as the next catalog version. Change no project file.

Read it:  echo '{}' | hgs team catalog get --json
  catalog.roles[]: id, name, color, prompt, tags, agent, account, model,
    effort, permissions, computer, folder (worktree|shared|read_only),
    can_message (lead|team), dangerous
  catalog.templates[]: id, name, description, lead_protocol,
    roster[{role, count, optional}], tasks[{title, role, deps[indexes]}],
    planning (lead|template), approval (review|auto), max_members
  catalog.routing[]: id, when (words all in the goal), template, approval
  draft.proposals[]: your earlier proposals and the owner's decisions

Propose (whole objects; ids use letters, digits, - and _):
  {"proposals":[{"kind":"role","action":"add","target":"security",
    "title":"New role: Security reviewer","reason":"payments code",
    "value":{"id":"security","name":"Security","agent":"claude",...}}]}
  | hgs team catalog propose --json
  kind is role, template or rule; action add, change (the whole new object)
  or remove (no value). Explain each change in reason, in one or two lines.
Check a goal's template: {"goal":"..."} | hgs team catalog route --json
