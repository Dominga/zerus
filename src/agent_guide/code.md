Find code and documentation (hgs guide code)

A checkout whose root holds lore.toml with [atlas] enabled has CodeLore's code map. It
covers the code and the Markdown documentation, refreshes itself from git on every
command (no daemon, no watcher) and names each hit as path:start-end: read those lines,
not the whole file.

  lore atlas search "what the code does"     code and docs by words, terms and symbols
  lore atlas search "exact error text"       an error message finds where it is raised
  lore atlas symbol NAME                     where NAME is defined and which files name it
  lore atlas deps FILE                       what FILE imports and what imports it
  lore atlas impact FILE                     every file a change of FILE reaches
  lore find "WORDS"                          records; code places only under a match
  lore for FILE                              what is known about FILE before you edit it

Run them in your own folder (or with -C FOLDER). Search before you open files by
guesswork; use grep only for an exact string the map did not find.

Where the project's memory lives depends on the git root's lore.toml (this page
ends with what it says for your folder):
- one area, use-the-project-store: the records (the documentation sliced into
  topics and decisions, faults, readings) live in the project's store under Zerus
  data, never in the repository; a lore add in the checkout is refused on purpose.
  Search it with hgs team lore find WORDS [--team ID]. Each record cites its source
  as refs.links zerus:PATH#HEADING@COMMIT: open that file at that commit to check it.
- real areas: the records live in the repository, committed by lore itself. Use
  lore -C GIT_ROOT find, for, show and add there (hgs team lore find reads them
  too, also outside a team).
Tasks live in Zerus (hgs task, the team's plan): never write lore plans by hand;
lore keeps knowledge only: fault, decision, reading, topic, recipe.
