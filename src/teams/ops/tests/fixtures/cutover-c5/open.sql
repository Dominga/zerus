BEGIN TRANSACTION;
CREATE TABLE answer_marks (
 team TEXT NOT NULL, member TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 revision INTEGER NOT NULL, replies INTEGER NOT NULL, PRIMARY KEY(team,member)
);
CREATE TABLE cancel_outcomes (
 decision INTEGER PRIMARY KEY, team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('cancelled','refused','released')), reason TEXT NOT NULL, at_ms INTEGER NOT NULL
);
CREATE TABLE cards (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL REFERENCES teams(id),
 kind TEXT NOT NULL, node INTEGER, actor TEXT NOT NULL, text TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '[]',
 state TEXT NOT NULL DEFAULT 'open', decision TEXT, decided_actor TEXT, created_ms INTEGER NOT NULL, decision_seq INTEGER,
 parameters TEXT, decisions TEXT);
CREATE TABLE decisions (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL, node INTEGER NOT NULL,
 generation INTEGER NOT NULL, actor TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL,
 state TEXT NOT NULL DEFAULT 'pending', stopped_ready INTEGER NOT NULL DEFAULT 0, stop INTEGER NOT NULL DEFAULT 0,
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
CREATE TABLE dependencies (team TEXT NOT NULL, node INTEGER NOT NULL, dependency INTEGER NOT NULL,
 kind TEXT NOT NULL, PRIMARY KEY(team,node,dependency),
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id),
 FOREIGN KEY(team,dependency) REFERENCES node_identity(team,id));
CREATE TABLE effect_outcomes (
 team TEXT NOT NULL, request_id TEXT NOT NULL, at_ms INTEGER NOT NULL,
 PRIMARY KEY(team,request_id)
);
INSERT INTO "effect_outcomes" VALUES('00000000-0000-4000-8000-00000000beef','c5548100-9471-4f68-a9b9-9a50dc275acd',1234);
INSERT INTO "effect_outcomes" VALUES('00000000-0000-4000-8000-00000000beef','1312cf18-8b34-413f-962e-598ee333be32',1234);
CREATE TABLE effects (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL, node INTEGER NOT NULL,
 member TEXT NOT NULL, generation INTEGER NOT NULL, request_id TEXT NOT NULL UNIQUE,
 actor TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'handout', state TEXT NOT NULL DEFAULT 'reserved', outcome TEXT, created_ms INTEGER NOT NULL,
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id), FOREIGN KEY(team,member) REFERENCES members(team,id));
INSERT INTO "effects" VALUES(1,'00000000-0000-4000-8000-00000000beef',1,'m1',1,'c5548100-9471-4f68-a9b9-9a50dc275acd','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout','cutting_over','{"status":"cutting_over","reason":"Fence held. Nothing was sent."}',1234);
INSERT INTO "effects" VALUES(2,'00000000-0000-4000-8000-00000000beef',2,'m2',1,'1312cf18-8b34-413f-962e-598ee333be32','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout','uncertain','{"status":"uncertain","reason":"Fence held. Nothing was sent."}',1234);
CREATE TABLE inbox (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL REFERENCES teams(id),
 kind TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL, actor TEXT NOT NULL,
 text TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '[]', state TEXT NOT NULL DEFAULT 'open', reply TEXT, reply_actor TEXT,
 created_ms INTEGER NOT NULL, UNIQUE(team,kind,node,generation));
INSERT INTO "inbox" VALUES(1,'00000000-0000-4000-8000-00000000beef','delivery_uncertain',1,1,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','Fence held. Nothing was sent.','[]','open',NULL,NULL,1234);
INSERT INTO "inbox" VALUES(2,'00000000-0000-4000-8000-00000000beef','delivery_uncertain',2,1,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','Fence held. Nothing was sent.','[]','open',NULL,NULL,1234);
CREATE TABLE integration_facts (decision INTEGER PRIMARY KEY REFERENCES decisions(seq),
 team TEXT NOT NULL, node INTEGER NOT NULL, actor TEXT NOT NULL,
 commit_id TEXT, conflicts TEXT NOT NULL, created_ms INTEGER NOT NULL,
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
CREATE TABLE journal (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL REFERENCES teams(id),
 at_ms INTEGER NOT NULL, actor TEXT NOT NULL, kind TEXT NOT NULL, request_id TEXT NOT NULL, text TEXT NOT NULL,
 refs TEXT NOT NULL DEFAULT '[]');
INSERT INTO "journal" VALUES(1,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"person","person":"local","origin":"desktop_ui"}','create','f6c4fe44-b330-44b2-80e2-62d1d443677e','create at revision 1','["team:00000000-0000-4000-8000-00000000beef"]');
INSERT INTO "journal" VALUES(2,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','set_member_session','4d5c2db4-3e5a-4c7a-a622-d8f2123ec3cb','set_member_session at revision 2','["team:00000000-0000-4000-8000-00000000beef","member:lead"]');
INSERT INTO "journal" VALUES(3,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','set_member_session','1d04737d-5233-40b7-a2d3-af4a564ddf32','set_member_session at revision 3','["team:00000000-0000-4000-8000-00000000beef","member:m1"]');
INSERT INTO "journal" VALUES(4,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','set_member_session','82e5270f-72a7-4eb7-90cf-f27a90fd14a9','set_member_session at revision 4','["team:00000000-0000-4000-8000-00000000beef","member:m2"]');
INSERT INTO "journal" VALUES(5,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"person","person":"local","origin":"desktop_ui"}','add_task','a9d3c7c1-f054-42ea-a097-b65d5ff3c067','add_task at revision 5','["team:00000000-0000-4000-8000-00000000beef","task:00000000-0000-4000-8000-000000000001"]');
INSERT INTO "journal" VALUES(6,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','observe_task','09255e9d-e070-43ea-8abb-131f214c959c','observe_task at revision 6','["team:00000000-0000-4000-8000-00000000beef","node:1"]');
INSERT INTO "journal" VALUES(7,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout_intent','b4812d9e-d7d7-4fc6-acd8-e1c998dd4d16','handout_intent at revision 7','["team:00000000-0000-4000-8000-00000000beef","node:1","member:m1","effect:c5548100-9471-4f68-a9b9-9a50dc275acd"]');
INSERT INTO "journal" VALUES(8,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout_outcome','6e3625bf-55a4-4ec7-83ec-460034f3b6ad','handout_outcome at revision 8: cutting over: Fence held. Nothing was sent.','["team:00000000-0000-4000-8000-00000000beef","effect:c5548100-9471-4f68-a9b9-9a50dc275acd"]');
INSERT INTO "journal" VALUES(9,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"person","person":"local","origin":"desktop_ui"}','add_task','fafbd4be-905d-4135-a487-da3fc63d5734','add_task at revision 9','["team:00000000-0000-4000-8000-00000000beef","task:00000000-0000-4000-8000-000000000002"]');
INSERT INTO "journal" VALUES(10,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','observe_task','1aed9b3f-2cdb-4abe-8171-bb708f93aed8','observe_task at revision 10','["team:00000000-0000-4000-8000-00000000beef","node:2"]');
INSERT INTO "journal" VALUES(11,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout_intent','d97e5ffd-4bd7-4fc1-92a9-d3318067b321','handout_intent at revision 11','["team:00000000-0000-4000-8000-00000000beef","node:2","member:m2","effect:1312cf18-8b34-413f-962e-598ee333be32"]');
INSERT INTO "journal" VALUES(12,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout_outcome','b141ff37-de30-427a-b553-3ef7719283bc','handout_outcome at revision 12: uncertain: Fence held. Nothing was sent.','["team:00000000-0000-4000-8000-00000000beef","effect:1312cf18-8b34-413f-962e-598ee333be32"]');
CREATE TABLE journal_archive(
  seq INT,
  team TEXT,
  at_ms INT,
  actor TEXT,
  kind TEXT,
  request_id TEXT,
  text TEXT,
  refs TEXT
);
CREATE TABLE key_clear_finished(team TEXT PRIMARY KEY);
CREATE TABLE key_clear_teams (team TEXT PRIMARY KEY, pending INTEGER NOT NULL);
CREATE TABLE key_clear_work (
 team TEXT NOT NULL, node INTEGER NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('stage','stop')),
 PRIMARY KEY(team,node)
);
CREATE TABLE key_clears (
 team TEXT NOT NULL, node INTEGER NOT NULL, state TEXT NOT NULL CHECK(state IN ('cleared','refused')),
 reason TEXT NOT NULL, at_ms INTEGER NOT NULL,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
CREATE TABLE late_sessions (
 team TEXT NOT NULL, member TEXT NOT NULL, launch TEXT NOT NULL, machine TEXT NOT NULL,
 session_id TEXT NOT NULL, name TEXT NOT NULL, card INTEGER NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,member,launch)
);
CREATE TABLE live_tasks (task_project TEXT NOT NULL, task_uuid TEXT NOT NULL, team TEXT NOT NULL, node INTEGER NOT NULL,
 PRIMARY KEY(task_project,task_uuid), FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
INSERT INTO "live_tasks" VALUES('example-project','00000000-0000-4000-8000-000000000001','00000000-0000-4000-8000-00000000beef',1);
INSERT INTO "live_tasks" VALUES('example-project','00000000-0000-4000-8000-000000000002','00000000-0000-4000-8000-00000000beef',2);
CREATE TABLE member_handles (
 team TEXT NOT NULL, member TEXT NOT NULL, machine TEXT NOT NULL,
 session_id TEXT NOT NULL, launch TEXT NOT NULL, label_hint TEXT NOT NULL,
 created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,member), UNIQUE(machine,session_id)
);
CREATE TABLE members (
 team TEXT NOT NULL REFERENCES teams(id), id TEXT NOT NULL, data TEXT NOT NULL,
 machine TEXT, session_id TEXT, state TEXT NOT NULL, person_stopped INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(team,id), UNIQUE(machine,session_id)
);
INSERT INTO "members" VALUES('00000000-0000-4000-8000-00000000beef','lead','{"id":"lead","role":"lead","agent":"claude","account":"account-example","state":"active","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000064"},"worktree":"/workspace/example-team-lead","ownership_nonce":"nonce-lead"}','00000000-0000-4000-8000-0000000000aa','launch_id:00000000-0000-4000-8000-000000000064','"active"',0);
INSERT INTO "members" VALUES('00000000-0000-4000-8000-00000000beef','m1','{"id":"m1","role":"developer","agent":"claude","account":"account-example","state":"active","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000031"},"worktree":"/workspace/example-team-m1","ownership_nonce":"nonce-m1"}','00000000-0000-4000-8000-0000000000aa','launch_id:00000000-0000-4000-8000-000000000031','"active"',0);
INSERT INTO "members" VALUES('00000000-0000-4000-8000-00000000beef','m2','{"id":"m2","role":"developer","agent":"claude","account":"account-example","state":"active","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000032"},"worktree":"/workspace/example-team-m2","ownership_nonce":"nonce-m2"}','00000000-0000-4000-8000-0000000000aa','launch_id:00000000-0000-4000-8000-000000000032','"active"',0);
INSERT INTO "members" VALUES('00000000-0000-4000-8000-00000000beef','m3','{"id":"m3","role":"reviewer","agent":"claude"}',NULL,NULL,'"not_created"',0);
CREATE TABLE node_archive (
 team TEXT NOT NULL, id INTEGER NOT NULL, data TEXT NOT NULL, rank TEXT NOT NULL,
 admitted INTEGER NOT NULL, routed INTEGER NOT NULL, hold INTEGER NOT NULL DEFAULT 0,
 task_revision TEXT NOT NULL, task_stage TEXT NOT NULL, task_cancelled INTEGER NOT NULL,
 generation INTEGER NOT NULL DEFAULT 1, decided_by TEXT, integrated INTEGER NOT NULL DEFAULT 0,
 integration_commit TEXT, requested TEXT, integration_required INTEGER NOT NULL DEFAULT 1,
 node_state TEXT GENERATED ALWAYS AS (CASE
   WHEN task_cancelled THEN 'cancelled'
   WHEN task_stage='done' AND integration_required AND COALESCE(json_extract(data,'$.kind'),'task')<>'stage' AND NOT integrated THEN 'reported'
   WHEN task_stage='done' THEN 'accepted'
   WHEN task_stage='unobserved' THEN 'waiting'
   WHEN NOT admitted OR NOT routed OR task_stage='backlog' THEN 'waiting'
   WHEN task_stage='in_progress' THEN 'in_progress'
   WHEN task_stage='in_review' THEN 'reported' ELSE 'pending' END) VIRTUAL, hold_owner TEXT, hand_back INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(team,id), FOREIGN KEY(team,id) REFERENCES node_identity(team,id)
);
CREATE TABLE node_identity (team TEXT NOT NULL REFERENCES teams(id), id INTEGER NOT NULL,
 task_project TEXT NOT NULL, task_uuid TEXT NOT NULL, PRIMARY KEY(team,id));
INSERT INTO "node_identity" VALUES('00000000-0000-4000-8000-00000000beef',1,'example-project','00000000-0000-4000-8000-000000000001');
INSERT INTO "node_identity" VALUES('00000000-0000-4000-8000-00000000beef',2,'example-project','00000000-0000-4000-8000-000000000002');
CREATE TABLE nodes (
 team TEXT NOT NULL, id INTEGER NOT NULL, data TEXT NOT NULL, rank TEXT NOT NULL,
 admitted INTEGER NOT NULL, routed INTEGER NOT NULL, hold INTEGER NOT NULL DEFAULT 0,
 task_revision TEXT NOT NULL, task_stage TEXT NOT NULL, task_cancelled INTEGER NOT NULL,
 generation INTEGER NOT NULL DEFAULT 1, decided_by TEXT, integrated INTEGER NOT NULL DEFAULT 0,
 integration_commit TEXT, requested TEXT, integration_required INTEGER NOT NULL DEFAULT 1,
 node_state TEXT GENERATED ALWAYS AS (CASE
   WHEN task_cancelled THEN 'cancelled'
   WHEN task_stage='done' AND integration_required AND COALESCE(json_extract(data,'$.kind'),'task')<>'stage' AND NOT integrated THEN 'reported'
   WHEN task_stage='done' THEN 'accepted'
   WHEN task_stage='unobserved' THEN 'waiting'
   WHEN NOT admitted OR NOT routed OR task_stage='backlog' THEN 'waiting'
   WHEN task_stage='in_progress' THEN 'in_progress'
   WHEN task_stage='in_review' THEN 'reported' ELSE 'pending' END) VIRTUAL, hold_owner TEXT, hand_back INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(team,id), FOREIGN KEY(team,id) REFERENCES node_identity(team,id)
);
INSERT INTO "nodes" VALUES('00000000-0000-4000-8000-00000000beef',1,'{"id":1,"phase":0,"pipeline":"reviewed","role":"developer","round":1,"source":"sent","task":{"project":"example-project","uuid":"00000000-0000-4000-8000-000000000001"},"title":"Task 1"}','a00000001',1,1,0,'token-1','todo',0,1,NULL,0,NULL,NULL,1,NULL,0);
INSERT INTO "nodes" VALUES('00000000-0000-4000-8000-00000000beef',2,'{"id":2,"phase":0,"pipeline":"reviewed","role":"developer","round":1,"source":"sent","task":{"project":"example-project","uuid":"00000000-0000-4000-8000-000000000002"},"title":"Task 2"}','a00000002',1,1,0,'token-2','todo',0,1,NULL,0,NULL,NULL,1,NULL,0);
CREATE TABLE phases (team TEXT NOT NULL REFERENCES teams(id), n INTEGER NOT NULL,
 data TEXT NOT NULL, PRIMARY KEY(team,n));
INSERT INTO "phases" VALUES('00000000-0000-4000-8000-00000000beef',0,'{"n":0,"title":"Queue","kind":"queue","mode":"free","state":"approved"}');
CREATE TABLE pipeline_runs (
 team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 state TEXT NOT NULL, stage INTEGER,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
CREATE TABLE reports (team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 actor TEXT NOT NULL, text TEXT NOT NULL, commit_id TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '[]', PRIMARY KEY(team,node,generation),
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
CREATE TABLE requests (id TEXT PRIMARY KEY, team TEXT NOT NULL, actor TEXT NOT NULL,
 content TEXT NOT NULL, result TEXT NOT NULL);
INSERT INTO "requests" VALUES('f6c4fe44-b330-44b2-80e2-62d1d443677e','00000000-0000-4000-8000-00000000beef','{"kind":"person","person":"local","origin":"desktop_ui"}','{"request_id":"f6c4fe44-b330-44b2-80e2-62d1d443677e","team":"00000000-0000-4000-8000-00000000beef","expected_revision":0,"op":"create","project":"example-project","name":"Fixes","goal":"","home":"00000000-0000-4000-8000-0000000000aa","bundle":{"template":{"id":"bugfix","name":"Bugfix","description":"A queue of independent fixes: a developer and a reviewer.","lead":{"agent":"claude"},"roster":[{"role":"developer","count":2},{"role":"reviewer","count":1}],"default_pipeline":"reviewed","settings":{"delegations":{"accept_results":true,"answer_members":true,"confirm_trust":true,"resume_exited":true,"merge_accepted":true},"caps":{"max_members":6,"max_phase_tasks":50,"max_open_tasks":500,"max_open_questions":3},"timers":{"report_timeout_seconds":120,"start_timeout_seconds":180,"inbox_interval_seconds":120,"remind_lead_minutes":15,"remind_person_minutes":60,"lead_stall_minutes":20,"answer_escalate_minutes":10,"resume_limit_per_hour":3},"queue":true,"routing":"direct","plan_approval":"review","phase_acceptance":"lead","integration":"branch","land_allowed":false}},"roles":[{"id":"developer","name":"Developer","prompt":"Implement the task in your own worktree. Start from the base commit named in the task, keep to its write set, run the checks that can fail for your change, commit, and report what you changed and how you checked it.","agent":"claude"},{"id":"reviewer","name":"Reviewer","prompt":"Review the reported commit against the task''s criteria. You never change what you review. Give a verdict: pass, or return with the exact findings.","agent":"claude"}],"pipelines":[{"id":"reviewed","name":"Reviewed: review, lead","implementer":"developer"}]},"integration_branch":"zerus/team/fixes","base_commit":"0123456789abcdef0123456789abcdef01234567"}','{"Success":{"ok":true,"revision":1}}');
INSERT INTO "requests" VALUES('4d5c2db4-3e5a-4c7a-a622-d8f2123ec3cb','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"4d5c2db4-3e5a-4c7a-a622-d8f2123ec3cb","team":"00000000-0000-4000-8000-00000000beef","expected_revision":1,"op":"set_member_session","member":"lead","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000064"},"account":"account-example","worktree":"/workspace/example-team-lead","ownership_nonce":"nonce-lead"}','{"Success":{"ok":true,"revision":2}}');
INSERT INTO "requests" VALUES('1d04737d-5233-40b7-a2d3-af4a564ddf32','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"1d04737d-5233-40b7-a2d3-af4a564ddf32","team":"00000000-0000-4000-8000-00000000beef","expected_revision":2,"op":"set_member_session","member":"m1","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000031"},"account":"account-example","worktree":"/workspace/example-team-m1","ownership_nonce":"nonce-m1"}','{"Success":{"ok":true,"revision":3}}');
INSERT INTO "requests" VALUES('82e5270f-72a7-4eb7-90cf-f27a90fd14a9','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"82e5270f-72a7-4eb7-90cf-f27a90fd14a9","team":"00000000-0000-4000-8000-00000000beef","expected_revision":3,"op":"set_member_session","member":"m2","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000032"},"account":"account-example","worktree":"/workspace/example-team-m2","ownership_nonce":"nonce-m2"}','{"Success":{"ok":true,"revision":4}}');
INSERT INTO "requests" VALUES('a9d3c7c1-f054-42ea-a097-b65d5ff3c067','00000000-0000-4000-8000-00000000beef','{"kind":"person","person":"local","origin":"desktop_ui"}','{"request_id":"a9d3c7c1-f054-42ea-a097-b65d5ff3c067","team":"00000000-0000-4000-8000-00000000beef","expected_revision":4,"op":"add_task","task":{"project":"example-project","uuid":"00000000-0000-4000-8000-000000000001"},"role":null,"pipeline":null}','{"Success":{"ok":true,"revision":5,"node":1}}');
INSERT INTO "requests" VALUES('09255e9d-e070-43ea-8abb-131f214c959c','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"09255e9d-e070-43ea-8abb-131f214c959c","team":"00000000-0000-4000-8000-00000000beef","expected_revision":5,"op":"observe_task","node":1,"observation":{"revision":"token-1","previous_revision":"","stage":"todo","cancelled":false,"rank":"a00000001","title":"Task 1","attempt":0,"attempt_id":"","attempt_conflict":false}}','{"Success":{"ok":true,"revision":6}}');
INSERT INTO "requests" VALUES('b4812d9e-d7d7-4fc6-acd8-e1c998dd4d16','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"b4812d9e-d7d7-4fc6-acd8-e1c998dd4d16","team":"00000000-0000-4000-8000-00000000beef","expected_revision":6,"op":"handout_intent","node":1,"member":"m1","effect_request_id":"c5548100-9471-4f68-a9b9-9a50dc275acd"}','{"Success":{"ok":true,"revision":7,"node":1,"item":1}}');
INSERT INTO "requests" VALUES('6e3625bf-55a4-4ec7-83ec-460034f3b6ad','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"6e3625bf-55a4-4ec7-83ec-460034f3b6ad","team":"00000000-0000-4000-8000-00000000beef","expected_revision":7,"op":"handout_outcome","effect_request_id":"c5548100-9471-4f68-a9b9-9a50dc275acd","outcome":{"status":"cutting_over","reason":"Fence held. Nothing was sent."}}','{"Success":{"ok":true,"revision":8,"item":1}}');
INSERT INTO "requests" VALUES('fafbd4be-905d-4135-a487-da3fc63d5734','00000000-0000-4000-8000-00000000beef','{"kind":"person","person":"local","origin":"desktop_ui"}','{"request_id":"fafbd4be-905d-4135-a487-da3fc63d5734","team":"00000000-0000-4000-8000-00000000beef","expected_revision":8,"op":"add_task","task":{"project":"example-project","uuid":"00000000-0000-4000-8000-000000000002"},"role":null,"pipeline":null}','{"Success":{"ok":true,"revision":9,"node":2}}');
INSERT INTO "requests" VALUES('1aed9b3f-2cdb-4abe-8171-bb708f93aed8','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"1aed9b3f-2cdb-4abe-8171-bb708f93aed8","team":"00000000-0000-4000-8000-00000000beef","expected_revision":9,"op":"observe_task","node":2,"observation":{"revision":"token-2","previous_revision":"","stage":"todo","cancelled":false,"rank":"a00000002","title":"Task 2","attempt":0,"attempt_id":"","attempt_conflict":false}}','{"Success":{"ok":true,"revision":10}}');
INSERT INTO "requests" VALUES('d97e5ffd-4bd7-4fc1-92a9-d3318067b321','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"d97e5ffd-4bd7-4fc1-92a9-d3318067b321","team":"00000000-0000-4000-8000-00000000beef","expected_revision":10,"op":"handout_intent","node":2,"member":"m2","effect_request_id":"1312cf18-8b34-413f-962e-598ee333be32"}','{"Success":{"ok":true,"revision":11,"node":2,"item":2}}');
INSERT INTO "requests" VALUES('b141ff37-de30-427a-b553-3ef7719283bc','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"b141ff37-de30-427a-b553-3ef7719283bc","team":"00000000-0000-4000-8000-00000000beef","expected_revision":11,"op":"handout_outcome","effect_request_id":"1312cf18-8b34-413f-962e-598ee333be32","outcome":{"status":"uncertain","reason":"Fence held. Nothing was sent."}}','{"Success":{"ok":true,"revision":12,"item":2}}');
CREATE TABLE stage_tasks (
 team TEXT NOT NULL, node INTEGER NOT NULL, target INTEGER NOT NULL,
 generation INTEGER NOT NULL, state TEXT NOT NULL DEFAULT 'wanted',
 request_id TEXT UNIQUE, outcome TEXT, created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
CREATE TABLE suggestions (team TEXT NOT NULL, node INTEGER NOT NULL, role TEXT, pipeline TEXT, PRIMARY KEY(team,node),
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
CREATE TABLE target_implementers (
 team TEXT NOT NULL, target INTEGER NOT NULL, member TEXT NOT NULL, round INTEGER NOT NULL,
 PRIMARY KEY(team,target,member), FOREIGN KEY(team,target) REFERENCES node_identity(team,id)
);
CREATE TABLE task_observations (
 team TEXT NOT NULL, node INTEGER NOT NULL, data TEXT NOT NULL,
 released INTEGER NOT NULL DEFAULT 0,
 attempt INTEGER NOT NULL DEFAULT 0, attempt_id TEXT NOT NULL DEFAULT '',
 last_attempt_id TEXT NOT NULL DEFAULT '', attempt_conflict INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
INSERT INTO "task_observations" VALUES('00000000-0000-4000-8000-00000000beef',1,'{"revision":"token-1","previous_revision":"","stage":"todo","cancelled":false,"rank":"a00000001","title":"Task 1","attempt":0,"attempt_id":"","attempt_conflict":false}',0,0,'','',0);
INSERT INTO "task_observations" VALUES('00000000-0000-4000-8000-00000000beef',2,'{"revision":"token-2","previous_revision":"","stage":"todo","cancelled":false,"rank":"a00000002","title":"Task 2","attempt":0,"attempt_id":"","attempt_conflict":false}',0,0,'','',0);
CREATE TABLE teams (
 id TEXT PRIMARY KEY, project TEXT NOT NULL, name TEXT NOT NULL, goal TEXT NOT NULL,
 home TEXT NOT NULL, owner TEXT NOT NULL, status TEXT NOT NULL, lead TEXT NOT NULL,
 settings TEXT NOT NULL, bundle TEXT NOT NULL, integration_branch TEXT NOT NULL,
 base_commit TEXT NOT NULL, revision INTEGER NOT NULL, person_paused INTEGER NOT NULL DEFAULT 0, next_node INTEGER NOT NULL DEFAULT 1
, stop_seq INTEGER NOT NULL DEFAULT 0);
INSERT INTO "teams" VALUES('00000000-0000-4000-8000-00000000beef','example-project','Fixes','','00000000-0000-4000-8000-0000000000aa','local','"running"','lead','{"delegations":{"accept_results":true,"answer_members":true,"confirm_trust":true,"resume_exited":true,"merge_accepted":true},"caps":{"max_members":6,"max_phase_tasks":50,"max_open_tasks":500,"max_open_questions":3},"timers":{"report_timeout_seconds":120,"start_timeout_seconds":180,"inbox_interval_seconds":120,"remind_lead_minutes":15,"remind_person_minutes":60,"lead_stall_minutes":20,"answer_escalate_minutes":10,"resume_limit_per_hour":3},"queue":true,"routing":"direct","plan_approval":"review","phase_acceptance":"lead","integration":"branch","land_allowed":false}','{"template":{"id":"bugfix","name":"Bugfix","description":"A queue of independent fixes: a developer and a reviewer.","lead":{"agent":"claude"},"roster":[{"role":"developer","count":2},{"role":"reviewer","count":1}],"default_pipeline":"reviewed","settings":{"delegations":{"accept_results":true,"answer_members":true,"confirm_trust":true,"resume_exited":true,"merge_accepted":true},"caps":{"max_members":6,"max_phase_tasks":50,"max_open_tasks":500,"max_open_questions":3},"timers":{"report_timeout_seconds":120,"start_timeout_seconds":180,"inbox_interval_seconds":120,"remind_lead_minutes":15,"remind_person_minutes":60,"lead_stall_minutes":20,"answer_escalate_minutes":10,"resume_limit_per_hour":3},"queue":true,"routing":"direct","plan_approval":"review","phase_acceptance":"lead","integration":"branch","land_allowed":false}},"roles":[{"id":"developer","name":"Developer","prompt":"Implement the task in your own worktree. Start from the base commit named in the task, keep to its write set, run the checks that can fail for your change, commit, and report what you changed and how you checked it.","agent":"claude"},{"id":"reviewer","name":"Reviewer","prompt":"Review the reported commit against the task''s criteria. You never change what you review. Give a verdict: pass, or return with the exact findings.","agent":"claude"}],"pipelines":[{"id":"reviewed","name":"Reviewed: review, lead","implementer":"developer"}]}','zerus/team/fixes','0123456789abcdef0123456789abcdef01234567',12,0,3,0);
CREATE TABLE timer_firings (
 team TEXT NOT NULL, kind TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 item INTEGER NOT NULL, target TEXT NOT NULL CHECK(target IN ('inbox','card')),
 seq INTEGER NOT NULL, subject INTEGER NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,kind,node,generation,item)
);
CREATE TABLE timers_extension (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL);
INSERT INTO "timers_extension" VALUES(1,12);
CREATE TABLE verdicts (
 team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 actor TEXT NOT NULL, pass INTEGER NOT NULL, text TEXT NOT NULL,
 PRIMARY KEY(team,node,generation), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
CREATE INDEX member_session ON members(team,machine,session_id);
CREATE INDEX identity_task ON node_identity(task_project,task_uuid,team,id);
CREATE INDEX live_tasks_team ON live_tasks(team,node);
CREATE INDEX queue_rank ON nodes(team,rank,id);
CREATE INDEX nodes_state ON nodes(team,node_state,id);
CREATE UNIQUE INDEX archive_node ON node_archive(team,id);
CREATE INDEX archive_state ON node_archive(team,node_state,id);
CREATE INDEX dependency_dependants ON dependencies(team,dependency,node);
CREATE INDEX inbox_page ON inbox(team,seq);
CREATE INDEX inbox_open ON inbox(team,state,seq);
CREATE INDEX cards_page ON cards(team,seq);
CREATE INDEX cards_open ON cards(team,state,seq);
CREATE INDEX journal_page ON journal(team,seq);
CREATE UNIQUE INDEX journal_archive_page ON journal_archive(team,seq);
CREATE INDEX effects_page ON effects(team,seq);
CREATE INDEX effects_pending ON effects(team,state,seq);
CREATE UNIQUE INDEX effect_node ON effects(team,node) WHERE state IN ('reserved','submitted','uncertain','cutting_over');
CREATE UNIQUE INDEX effect_member ON effects(team,member) WHERE state IN ('reserved','submitted','uncertain','cutting_over');
CREATE INDEX decisions_node ON decisions(team,node,seq);
CREATE INDEX decisions_page ON decisions(team,seq);
CREATE UNIQUE INDEX decision_pending ON decisions(team,node) WHERE state<>'settled';
CREATE INDEX integration_node ON integration_facts(team,node,decision);
CREATE INDEX stage_tasks_pending ON stage_tasks(team,node)
 WHERE state IN ('wanted','reserved');
CREATE INDEX teams_live ON teams(id)
 WHERE status IN ('"running"','"paused"');
CREATE INDEX cards_waiting ON cards(seq) WHERE state='open';
CREATE INDEX effects_attempt ON effects(team,node,generation,seq);
CREATE INDEX integration_approved ON cards(team,decision_seq)
 WHERE kind='integration' AND state='closed' AND decision='approve';
CREATE INDEX integration_card ON cards(decision_seq)
 WHERE kind='integration';
CREATE INDEX inbox_unstaffed_told ON inbox(team,node,seq)
 WHERE kind LIKE 'unstaffed:%' AND reply IS NULL;
CREATE INDEX inbox_lead_queue ON inbox(team,seq)
 WHERE node=0 AND reply IS NULL AND (kind LIKE 'lead_admission:%' OR kind LIKE 'lead_routing:%');
CREATE INDEX nodes_lead_admission ON nodes(team,id)
 WHERE admitted=0 AND node_state IN ('waiting','pending');
CREATE INDEX nodes_lead_routing ON nodes(team,id)
 WHERE admitted=1 AND routed=0 AND node_state IN ('waiting','pending');
CREATE INDEX list_nodes_progress ON nodes(team,node_state,id)
 WHERE COALESCE(json_extract(data,'$.kind'),'task')='task';
CREATE INDEX list_archive_progress ON node_archive(team,node_state,id)
 WHERE COALESCE(json_extract(data,'$.kind'),'task')='task';
CREATE INDEX list_inbox_questions ON inbox(team,kind)
 WHERE state='open' AND kind GLOB 'question:*';
CREATE INDEX list_cards_blocked ON cards(team,kind) WHERE state='open';
CREATE INDEX list_phases_progress ON phases(team,json_extract(data,'$.state'));
CREATE INDEX list_members_progress ON members(team,state,id);
CREATE TRIGGER member_handles_terminal_v1 AFTER UPDATE OF status ON teams
 WHEN NEW.status IN ('"finished"','"stopped"','"archived"') BEGIN
 DELETE FROM member_handles WHERE team=NEW.id;
END;
CREATE TRIGGER member_handles_removed_v1 AFTER UPDATE OF state ON members
 WHEN NEW.state='"removed"' BEGIN
 DELETE FROM member_handles WHERE team=NEW.team AND member=NEW.id;
END;
CREATE INDEX key_clear_pending ON key_clear_teams(team) WHERE pending=1;
CREATE INDEX key_clear_work_kind ON key_clear_work(team,kind,node);
CREATE INDEX timer_card_subject ON timer_firings(team,subject) WHERE target='card';
CREATE INDEX timer_card_node ON timer_firings(team,node) WHERE target='card';
CREATE INDEX key_refusals ON key_clears(team,node) WHERE state='refused';
CREATE INDEX key_refusals_newest ON key_clears(team,at_ms,node) WHERE state='refused';
CREATE TRIGGER key_clear_terminal_v1 AFTER UPDATE OF status ON teams
 WHEN NEW.status IN ('"stopped"','"archived"') BEGIN
 INSERT OR IGNORE INTO key_clear_teams(team,pending) VALUES(NEW.id,1);
END;
CREATE TRIGGER timer_cards_decided_v1 AFTER INSERT ON decisions BEGIN
 UPDATE cards SET state='closed',decision='stale',decided_actor=actor
  WHERE state='open' AND kind='report_waiting' AND seq IN
  (SELECT seq FROM timer_firings WHERE team=NEW.team AND node=NEW.node AND target='card');
END;
CREATE TRIGGER timer_cards_item_v1 AFTER UPDATE OF state ON inbox
 WHEN OLD.state='open' AND NEW.state<>'open' BEGIN
 UPDATE cards SET state='closed',decision='stale',decided_actor=actor
  WHERE state='open' AND kind IN ('report_waiting','silent_lead') AND seq IN
  (SELECT seq FROM timer_firings WHERE team=NEW.team AND subject=NEW.seq AND target='card');
END;
CREATE TRIGGER key_clear_stage_archived_v2 AFTER INSERT ON node_archive
 WHEN json_extract(NEW.data,'$.kind')='stage' BEGIN
 INSERT OR IGNORE INTO key_clear_work(team,node,kind) SELECT a.team,a.id,'stage' FROM node_archive a
  WHERE a.team=NEW.team AND a.id=NEW.id AND json_extract(a.data,'$.kind')='stage'
  AND NOT EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.state IN ('wanted','reserved','created'))
  AND (EXISTS(SELECT 1 FROM node_identity i WHERE i.team=a.team AND i.id=a.id AND i.task_uuid<>'')
   OR EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.request_id IS NOT NULL))
  AND NOT EXISTS(SELECT 1 FROM key_clears k WHERE k.team=a.team AND k.node=a.id);
 UPDATE key_clear_teams SET pending=1 WHERE team=NEW.team AND pending=0
  AND EXISTS(SELECT 1 FROM key_clear_work w WHERE w.team=NEW.team AND w.node=NEW.id);
END;
CREATE TRIGGER key_clear_stage_settled_v2 AFTER UPDATE OF state ON stage_tasks
 WHEN NEW.state NOT IN ('wanted','reserved','created') BEGIN
 INSERT OR IGNORE INTO key_clear_work(team,node,kind) SELECT a.team,a.id,'stage' FROM node_archive a
  WHERE a.team=NEW.team AND a.id=NEW.node AND json_extract(a.data,'$.kind')='stage'
  AND NOT EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.state IN ('wanted','reserved','created'))
  AND (EXISTS(SELECT 1 FROM node_identity i WHERE i.team=a.team AND i.id=a.id AND i.task_uuid<>'')
   OR EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.request_id IS NOT NULL))
  AND NOT EXISTS(SELECT 1 FROM key_clears k WHERE k.team=a.team AND k.node=a.id);
 UPDATE key_clear_teams SET pending=1 WHERE team=NEW.team AND pending=0
  AND EXISTS(SELECT 1 FROM key_clear_work w WHERE w.team=NEW.team AND w.node=NEW.node);
END;
CREATE TRIGGER key_clear_cancel_settled_v1 AFTER UPDATE OF state ON decisions
 WHEN NEW.kind='cancel' AND OLD.state<>'settled' AND NEW.state='settled' BEGIN
 INSERT OR IGNORE INTO key_clear_work(team,node,kind) SELECT i.team,i.id,'stop' FROM node_identity i
  LEFT JOIN stage_tasks s ON s.team=i.team AND s.node=i.id
  WHERE i.team=NEW.team AND i.id=NEW.node
  AND EXISTS(SELECT 1 FROM teams t WHERE t.id=NEW.team AND t.status IN ('"stopped"','"archived"'))
  AND (i.task_uuid<>'' OR s.request_id IS NOT NULL)
  AND NOT EXISTS(SELECT 1 FROM task_observations o WHERE o.team=i.team AND o.node=i.id AND json_extract(o.data,'$.key') IS NOT NULL)
  AND NOT EXISTS(SELECT 1 FROM key_clears k WHERE k.team=i.team AND k.node=i.id);
END;
CREATE INDEX cancel_refusals ON cancel_outcomes(team,node) WHERE state='refused';
CREATE INDEX cancel_node_newest ON cancel_outcomes(team,node,decision DESC);
CREATE INDEX decisions_attempt ON decisions(team,generation,node);
CREATE INDEX answer_marks_node ON answer_marks(team,node);
CREATE INDEX inbox_questions ON inbox(team,node,generation,seq) WHERE kind GLOB 'question:*';
CREATE TRIGGER answer_mark_drop_v1 AFTER INSERT ON decisions BEGIN
 DELETE FROM answer_marks WHERE team=NEW.team AND node=NEW.node;
END;
DELETE FROM "sqlite_sequence";
INSERT INTO "sqlite_sequence" VALUES('journal',12);
INSERT INTO "sqlite_sequence" VALUES('effects',2);
INSERT INTO "sqlite_sequence" VALUES('inbox',2);
COMMIT;
PRAGMA user_version=2;
