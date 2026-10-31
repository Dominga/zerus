Build broker: members sharing one folder (hgs guide team-broker)

When two or more members work in one folder (a game project, no worktrees),
their builds would race in one build folder and compile each other's
half-written files. The owner's build broker stops that. It is off by default.

Lead: your inbox names a shared folder; propose the switch on a card:
{"kind":"switch-setting","setting":"build_broker","value":true}.

Member, with the broker on: a build template runs in your own snapshot of the
folder. Name the files you change: "args":{"files":["src/a.cpp"]}; the rest
builds at HEAD, so another member's unfinished file never reaches your build.
Builds of one build folder run one at a time; an unchanged tree builds once.
A failure in another member's file is theirs: wait and build again, never
edit their files.
