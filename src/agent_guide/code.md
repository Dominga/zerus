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

The project's memory is not in the checkout. Its records (the documentation sliced
into topics and decisions, faults, readings) live in the project's store under Zerus
data. Search it with:
  hgs team lore find WORDS [--team ID]       the team's workspace and the project store
Each record cites its source as refs.links zerus:PATH#HEADING@COMMIT: open that file
at that commit to check it.

The checkout's lore.toml has one area, use-the-project-store: a lore add there is
refused on purpose. Records go to the project store through Zerus, never into the
repository.
