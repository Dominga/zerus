PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE teams (
 id TEXT PRIMARY KEY, project TEXT NOT NULL, name TEXT NOT NULL, goal TEXT NOT NULL,
 home TEXT NOT NULL, owner TEXT NOT NULL, status TEXT NOT NULL, lead TEXT NOT NULL,
 settings TEXT NOT NULL, bundle TEXT NOT NULL, integration_branch TEXT NOT NULL,
 base_commit TEXT NOT NULL, revision INTEGER NOT NULL, person_paused INTEGER NOT NULL DEFAULT 0, next_node INTEGER NOT NULL DEFAULT 1
);
INSERT INTO teams VALUES('00000000-0000-4000-8000-00000000beef','example-project','Fixes','','00000000-0000-4000-8000-0000000000aa','local','"running"','lead','{"delegations":{"accept_results":true,"answer_members":true,"confirm_trust":true,"resume_exited":true,"merge_accepted":true},"caps":{"max_members":6,"max_phase_tasks":50,"max_open_tasks":500,"max_open_questions":3},"timers":{"report_timeout_seconds":120,"start_timeout_seconds":180,"inbox_interval_seconds":120,"remind_lead_minutes":15,"remind_person_minutes":60,"lead_stall_minutes":20,"answer_escalate_minutes":10,"resume_limit_per_hour":3},"queue":true,"routing":"direct","plan_approval":"review","phase_acceptance":"lead","integration":"branch","land_allowed":false}','{"template":{"id":"bugfix","name":"Bugfix","description":"A queue of independent fixes: a developer and a reviewer.","lead":{"agent":"claude"},"roster":[{"role":"developer","count":2},{"role":"reviewer","count":1}],"default_pipeline":"reviewed","settings":{"delegations":{"accept_results":true,"answer_members":true,"confirm_trust":true,"resume_exited":true,"merge_accepted":true},"caps":{"max_members":6,"max_phase_tasks":50,"max_open_tasks":500,"max_open_questions":3},"timers":{"report_timeout_seconds":120,"start_timeout_seconds":180,"inbox_interval_seconds":120,"remind_lead_minutes":15,"remind_person_minutes":60,"lead_stall_minutes":20,"answer_escalate_minutes":10,"resume_limit_per_hour":3},"queue":true,"routing":"direct","plan_approval":"review","phase_acceptance":"lead","integration":"branch","land_allowed":false}},"roles":[{"id":"developer","name":"Developer","prompt":"Implement the task in your own worktree. Start from the base commit named in the task, keep to its write set, run the checks that can fail for your change, commit, and report what you changed and how you checked it.","agent":"claude"},{"id":"reviewer","name":"Reviewer","prompt":"Review the reported commit against the task''s criteria. You never change what you review. Give a verdict: pass, or return with the exact findings.","agent":"claude"}],"pipelines":[{"id":"reviewed","name":"Reviewed: review, lead","implementer":"developer","stages":[{"kind":"review","role":"reviewer"}],"accept":"person"}]}','zerus/team/fixes','0123456789abcdef0123456789abcdef01234567',18,0,3);
CREATE TABLE members (
 team TEXT NOT NULL REFERENCES teams(id), id TEXT NOT NULL, data TEXT NOT NULL,
 machine TEXT, session_id TEXT, state TEXT NOT NULL, person_stopped INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(team,id), UNIQUE(machine,session_id)
);
INSERT INTO members VALUES('00000000-0000-4000-8000-00000000beef','lead','{"id":"lead","role":"lead","agent":"claude","account":"account-example","state":"active","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000064"},"worktree":"/workspace/example-team-lead","ownership_nonce":"nonce-lead"}','00000000-0000-4000-8000-0000000000aa','launch_id:00000000-0000-4000-8000-000000000064','"active"',0);
INSERT INTO members VALUES('00000000-0000-4000-8000-00000000beef','m1','{"id":"m1","role":"developer","agent":"claude","account":"account-example","state":"active","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000031"},"worktree":"/workspace/example-team-m1","ownership_nonce":"nonce-m1"}','00000000-0000-4000-8000-0000000000aa','launch_id:00000000-0000-4000-8000-000000000031','"active"',0);
INSERT INTO members VALUES('00000000-0000-4000-8000-00000000beef','m2','{"id":"m2","role":"developer","agent":"claude"}',NULL,NULL,'"not_created"',0);
INSERT INTO members VALUES('00000000-0000-4000-8000-00000000beef','m3','{"id":"m3","role":"reviewer","agent":"claude","account":"account-example","state":"active","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000033"},"worktree":"/workspace/example-team-m3","ownership_nonce":"nonce-m3"}','00000000-0000-4000-8000-0000000000aa','launch_id:00000000-0000-4000-8000-000000000033','"active"',0);
CREATE TABLE phases (team TEXT NOT NULL REFERENCES teams(id), n INTEGER NOT NULL,
 data TEXT NOT NULL, PRIMARY KEY(team,n));
INSERT INTO phases VALUES('00000000-0000-4000-8000-00000000beef',0,'{"n":0,"title":"Queue","kind":"queue","mode":"free","state":"approved"}');
CREATE TABLE node_identity (team TEXT NOT NULL REFERENCES teams(id), id INTEGER NOT NULL,
 task_project TEXT NOT NULL, task_uuid TEXT NOT NULL, PRIMARY KEY(team,id));
INSERT INTO node_identity VALUES('00000000-0000-4000-8000-00000000beef',1,'example-project','00000000-0000-4000-8000-000000000001');
INSERT INTO node_identity VALUES('00000000-0000-4000-8000-00000000beef',2,'example-project','00000000-0000-4000-8000-0000000003ea');
CREATE TABLE live_tasks (task_project TEXT NOT NULL, task_uuid TEXT NOT NULL, team TEXT NOT NULL, node INTEGER NOT NULL,
 PRIMARY KEY(task_project,task_uuid), FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
INSERT INTO live_tasks VALUES('example-project','00000000-0000-4000-8000-000000000001','00000000-0000-4000-8000-00000000beef',1);
INSERT INTO live_tasks VALUES('example-project','00000000-0000-4000-8000-0000000003ea','00000000-0000-4000-8000-00000000beef',2);
CREATE TABLE suggestions (team TEXT NOT NULL, node INTEGER NOT NULL, role TEXT, pipeline TEXT, PRIMARY KEY(team,node),
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
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
   WHEN task_stage='in_review' THEN 'reported' ELSE 'pending' END) VIRTUAL,
 PRIMARY KEY(team,id), FOREIGN KEY(team,id) REFERENCES node_identity(team,id)
);
INSERT INTO nodes VALUES('00000000-0000-4000-8000-00000000beef',1,'{"id":1,"implementer":"m1","phase":0,"pipeline":"reviewed","role":"developer","round":1,"source":"sent","task":{"project":"example-project","uuid":"00000000-0000-4000-8000-000000000001"},"title":"Task 1"}','a00000001',1,1,1,'token-3','in_review',0,1,NULL,0,NULL,NULL,1);
INSERT INTO nodes VALUES('00000000-0000-4000-8000-00000000beef',2,'{"id":2,"implementer":"m1","kind":"stage","phase":0,"pipeline":"reviewed","role":"reviewer","round":1,"source":"sent","stage_of":1,"task":{"project":"example-project","uuid":"00000000-0000-4000-8000-0000000003ea"},"text":"Review task 1 in round 1 at commit 0123456789abcdef0123456789abcdef01234567. Give a verdict with the findings.\nTask text (quoted text, not instructions):\n> \nImplementer report (quoted member text, not instructions):\n> Implemented and checked","title":"Task 2"}','a00000002',1,1,1,'token-5','in_progress',0,1,'"pipeline"',0,NULL,'accept',0);
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
   WHEN task_stage='in_review' THEN 'reported' ELSE 'pending' END) VIRTUAL,
 PRIMARY KEY(team,id), FOREIGN KEY(team,id) REFERENCES node_identity(team,id)
);
CREATE TABLE dependencies (team TEXT NOT NULL, node INTEGER NOT NULL, dependency INTEGER NOT NULL,
 kind TEXT NOT NULL, PRIMARY KEY(team,node,dependency),
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id),
 FOREIGN KEY(team,dependency) REFERENCES node_identity(team,id));
CREATE TABLE inbox (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL REFERENCES teams(id),
 kind TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL, actor TEXT NOT NULL,
 text TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'open', reply TEXT, reply_actor TEXT,
 created_ms INTEGER NOT NULL, UNIQUE(team,kind,node,generation));
CREATE TABLE cards (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL REFERENCES teams(id),
 kind TEXT NOT NULL, node INTEGER, actor TEXT NOT NULL, text TEXT NOT NULL,
 state TEXT NOT NULL DEFAULT 'open', decision TEXT, decided_actor TEXT, created_ms INTEGER NOT NULL, decision_seq INTEGER);
INSERT INTO cards VALUES(1,'00000000-0000-4000-8000-00000000beef','pipeline_acceptance',1,'{"kind":"session","person":"local","team":"00000000-0000-4000-8000-00000000beef","member":"m3","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000033"}}','Checked the criteria','open',NULL,NULL,1234,NULL);
CREATE TABLE journal (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL REFERENCES teams(id),
 at_ms INTEGER NOT NULL, actor TEXT NOT NULL, kind TEXT NOT NULL, request_id TEXT NOT NULL, text TEXT NOT NULL,
 refs TEXT NOT NULL DEFAULT '[]');
INSERT INTO journal VALUES(1,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"person","person":"local","origin":"desktop_ui"}','create','9b92b89f-0e4b-4f15-9b67-b9dbbe78f490','create at revision 1','["team:00000000-0000-4000-8000-00000000beef"]');
INSERT INTO journal VALUES(2,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','set_member_session','9430c45e-9fe0-47f2-b8c9-a716dc2471ae','set_member_session at revision 2','["team:00000000-0000-4000-8000-00000000beef","member:lead"]');
INSERT INTO journal VALUES(3,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','set_member_session','1a545510-7eee-48b0-8e71-fac9efbdea24','set_member_session at revision 3','["team:00000000-0000-4000-8000-00000000beef","member:m1"]');
INSERT INTO journal VALUES(4,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','set_member_session','26402fc6-5f3b-47f3-a081-47f190c4ea34','set_member_session at revision 4','["team:00000000-0000-4000-8000-00000000beef","member:m3"]');
INSERT INTO journal VALUES(5,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"person","person":"local","origin":"desktop_ui"}','add_task','59796e37-b969-4b97-a227-b33a9a1ff6a6','add_task at revision 5','["team:00000000-0000-4000-8000-00000000beef","task:00000000-0000-4000-8000-000000000001"]');
INSERT INTO journal VALUES(6,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','observe_task','30f90606-439d-4535-99f5-bf0ceea537e0','observe_task at revision 6','["team:00000000-0000-4000-8000-00000000beef","node:1"]');
INSERT INTO journal VALUES(7,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout_intent','f3583298-d5b3-4474-bea5-29acd0e99a1e','handout_intent at revision 7','["team:00000000-0000-4000-8000-00000000beef","node:1","member:m1","effect:84914d26-2422-49a4-9d05-4dc3be3b2eec"]');
INSERT INTO journal VALUES(8,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout_outcome','84e8efc5-e917-4e3b-a14c-afaa14b59fa2','handout_outcome at revision 8','["team:00000000-0000-4000-8000-00000000beef","effect:84914d26-2422-49a4-9d05-4dc3be3b2eec"]');
INSERT INTO journal VALUES(9,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','observe_task','de0563fb-115e-4a92-b152-6c82130530d4','observe_task at revision 9','["team:00000000-0000-4000-8000-00000000beef","node:1"]');
INSERT INTO journal VALUES(10,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','observe_task','2dc17a73-d13d-45cb-9df4-8c817b1388ab','observe_task at revision 10','["team:00000000-0000-4000-8000-00000000beef","node:1"]');
INSERT INTO journal VALUES(11,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','record_report','5a6730a5-36cc-4d55-965d-52251b443b11','record_report at revision 11','["team:00000000-0000-4000-8000-00000000beef","node:1","member:m1"]');
INSERT INTO journal VALUES(12,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','stage_task_intent','7c2de677-a21f-4d4f-95c8-a51f1fb48b65','stage_task_intent at revision 12','["team:00000000-0000-4000-8000-00000000beef","node:2","effect:e69287f0-a482-444d-9c2d-9415d06a899e"]');
INSERT INTO journal VALUES(13,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','stage_task_outcome','f7707773-5e31-4f2d-bb52-855a42595227','stage_task_outcome at revision 13','["team:00000000-0000-4000-8000-00000000beef","effect:e69287f0-a482-444d-9c2d-9415d06a899e"]');
INSERT INTO journal VALUES(14,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','observe_task','4ea03994-b4cc-40b6-86b3-663594a5e391','observe_task at revision 14','["team:00000000-0000-4000-8000-00000000beef","node:2"]');
INSERT INTO journal VALUES(15,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout_intent','71b76514-73a3-4df2-aecc-65870d43a793','handout_intent at revision 15','["team:00000000-0000-4000-8000-00000000beef","node:2","member:m3","effect:d50ecdac-87ae-4c4e-83e2-986bda94bd00"]');
INSERT INTO journal VALUES(16,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout_outcome','a150d896-b868-4eff-a5ac-2130c32c9583','handout_outcome at revision 16','["team:00000000-0000-4000-8000-00000000beef","effect:d50ecdac-87ae-4c4e-83e2-986bda94bd00"]');
INSERT INTO journal VALUES(17,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','observe_task','6dff4cb8-9b39-4a30-a0dc-6d5114e9c9bf','observe_task at revision 17','["team:00000000-0000-4000-8000-00000000beef","node:2"]');
INSERT INTO journal VALUES(18,'00000000-0000-4000-8000-00000000beef',1234,'{"kind":"session","person":"local","team":"00000000-0000-4000-8000-00000000beef","member":"m3","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000033"}}','verdict','4928bf1a-1a0c-4e8d-ab93-111e32da3f51','verdict at revision 18','["team:00000000-0000-4000-8000-00000000beef","node:2"]');
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
CREATE TABLE effects (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL, node INTEGER NOT NULL,
 member TEXT NOT NULL, generation INTEGER NOT NULL, request_id TEXT NOT NULL UNIQUE,
 actor TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'handout', state TEXT NOT NULL DEFAULT 'reserved', outcome TEXT, created_ms INTEGER NOT NULL,
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id), FOREIGN KEY(team,member) REFERENCES members(team,id));
INSERT INTO effects VALUES(1,'00000000-0000-4000-8000-00000000beef',1,'m1',1,'84914d26-2422-49a4-9d05-4dc3be3b2eec','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout','reported','{"status":"submitted","receipt":"receipt-example"}',1234);
INSERT INTO effects VALUES(2,'00000000-0000-4000-8000-00000000beef',2,'m3',1,'d50ecdac-87ae-4c4e-83e2-986bda94bd00','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','handout','reported','{"status":"submitted","receipt":"receipt-example"}',1234);
CREATE TABLE reports (team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 actor TEXT NOT NULL, text TEXT NOT NULL, commit_id TEXT NOT NULL, PRIMARY KEY(team,node,generation),
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
INSERT INTO reports VALUES('00000000-0000-4000-8000-00000000beef',1,1,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','Implemented and checked','0123456789abcdef0123456789abcdef01234567');
CREATE TABLE decisions (seq INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL, node INTEGER NOT NULL,
 generation INTEGER NOT NULL, actor TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL,
 state TEXT NOT NULL DEFAULT 'pending', stopped_ready INTEGER NOT NULL DEFAULT 0,
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
INSERT INTO decisions VALUES(1,'00000000-0000-4000-8000-00000000beef',2,1,'{"kind":"session","person":"local","team":"00000000-0000-4000-8000-00000000beef","member":"m3","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000033"}}','accept','','ready',0);
CREATE TABLE integration_facts (decision INTEGER PRIMARY KEY REFERENCES decisions(seq),
 team TEXT NOT NULL, node INTEGER NOT NULL, actor TEXT NOT NULL,
 commit_id TEXT, conflicts TEXT NOT NULL, created_ms INTEGER NOT NULL,
 FOREIGN KEY(team,node) REFERENCES node_identity(team,id));
CREATE TABLE requests (id TEXT PRIMARY KEY, team TEXT NOT NULL, actor TEXT NOT NULL,
 content TEXT NOT NULL, result TEXT NOT NULL);
INSERT INTO requests VALUES('9b92b89f-0e4b-4f15-9b67-b9dbbe78f490','00000000-0000-4000-8000-00000000beef','{"kind":"person","person":"local","origin":"desktop_ui"}','{"request_id":"9b92b89f-0e4b-4f15-9b67-b9dbbe78f490","team":"00000000-0000-4000-8000-00000000beef","expected_revision":0,"op":"create","project":"example-project","name":"Fixes","goal":"","home":"00000000-0000-4000-8000-0000000000aa","bundle":{"template":{"id":"bugfix","name":"Bugfix","description":"A queue of independent fixes: a developer and a reviewer.","lead":{"agent":"claude"},"roster":[{"role":"developer","count":2},{"role":"reviewer","count":1}],"default_pipeline":"reviewed","settings":{"delegations":{"accept_results":true,"answer_members":true,"confirm_trust":true,"resume_exited":true,"merge_accepted":true},"caps":{"max_members":6,"max_phase_tasks":50,"max_open_tasks":500,"max_open_questions":3},"timers":{"report_timeout_seconds":120,"start_timeout_seconds":180,"inbox_interval_seconds":120,"remind_lead_minutes":15,"remind_person_minutes":60,"lead_stall_minutes":20,"answer_escalate_minutes":10,"resume_limit_per_hour":3},"queue":true,"routing":"direct","plan_approval":"review","phase_acceptance":"lead","integration":"branch","land_allowed":false}},"roles":[{"id":"developer","name":"Developer","prompt":"Implement the task in your own worktree. Start from the base commit named in the task, keep to its write set, run the checks that can fail for your change, commit, and report what you changed and how you checked it.","agent":"claude"},{"id":"reviewer","name":"Reviewer","prompt":"Review the reported commit against the task''s criteria. You never change what you review. Give a verdict: pass, or return with the exact findings.","agent":"claude"}],"pipelines":[{"id":"reviewed","name":"Reviewed: review, lead","implementer":"developer","stages":[{"kind":"review","role":"reviewer"}],"accept":"person"}]},"integration_branch":"zerus/team/fixes","base_commit":"0123456789abcdef0123456789abcdef01234567"}','{"Success":{"ok":true,"revision":1}}');
INSERT INTO requests VALUES('9430c45e-9fe0-47f2-b8c9-a716dc2471ae','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"9430c45e-9fe0-47f2-b8c9-a716dc2471ae","team":"00000000-0000-4000-8000-00000000beef","expected_revision":1,"op":"set_member_session","member":"lead","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000064"},"account":"account-example","worktree":"/workspace/example-team-lead","ownership_nonce":"nonce-lead"}','{"Success":{"ok":true,"revision":2}}');
INSERT INTO requests VALUES('1a545510-7eee-48b0-8e71-fac9efbdea24','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"1a545510-7eee-48b0-8e71-fac9efbdea24","team":"00000000-0000-4000-8000-00000000beef","expected_revision":2,"op":"set_member_session","member":"m1","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000031"},"account":"account-example","worktree":"/workspace/example-team-m1","ownership_nonce":"nonce-m1"}','{"Success":{"ok":true,"revision":3}}');
INSERT INTO requests VALUES('26402fc6-5f3b-47f3-a081-47f190c4ea34','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"26402fc6-5f3b-47f3-a081-47f190c4ea34","team":"00000000-0000-4000-8000-00000000beef","expected_revision":3,"op":"set_member_session","member":"m3","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000033"},"account":"account-example","worktree":"/workspace/example-team-m3","ownership_nonce":"nonce-m3"}','{"Success":{"ok":true,"revision":4}}');
INSERT INTO requests VALUES('59796e37-b969-4b97-a227-b33a9a1ff6a6','00000000-0000-4000-8000-00000000beef','{"kind":"person","person":"local","origin":"desktop_ui"}','{"request_id":"59796e37-b969-4b97-a227-b33a9a1ff6a6","team":"00000000-0000-4000-8000-00000000beef","expected_revision":4,"op":"add_task","task":{"project":"example-project","uuid":"00000000-0000-4000-8000-000000000001"},"role":null,"pipeline":null}','{"Success":{"ok":true,"revision":5,"node":1}}');
INSERT INTO requests VALUES('30f90606-439d-4535-99f5-bf0ceea537e0','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"30f90606-439d-4535-99f5-bf0ceea537e0","team":"00000000-0000-4000-8000-00000000beef","expected_revision":5,"op":"observe_task","node":1,"observation":{"revision":"token-1","previous_revision":"","stage":"todo","cancelled":false,"rank":"a00000001","title":"Task 1","attempt":0,"attempt_id":"","attempt_conflict":false}}','{"Success":{"ok":true,"revision":6}}');
INSERT INTO requests VALUES('f3583298-d5b3-4474-bea5-29acd0e99a1e','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"f3583298-d5b3-4474-bea5-29acd0e99a1e","team":"00000000-0000-4000-8000-00000000beef","expected_revision":6,"op":"handout_intent","node":1,"member":"m1","effect_request_id":"84914d26-2422-49a4-9d05-4dc3be3b2eec"}','{"Success":{"ok":true,"revision":7,"node":1,"item":1}}');
INSERT INTO requests VALUES('84e8efc5-e917-4e3b-a14c-afaa14b59fa2','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"84e8efc5-e917-4e3b-a14c-afaa14b59fa2","team":"00000000-0000-4000-8000-00000000beef","expected_revision":7,"op":"handout_outcome","effect_request_id":"84914d26-2422-49a4-9d05-4dc3be3b2eec","outcome":{"status":"submitted","receipt":"receipt-example"}}','{"Success":{"ok":true,"revision":8}}');
INSERT INTO requests VALUES('de0563fb-115e-4a92-b152-6c82130530d4','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"de0563fb-115e-4a92-b152-6c82130530d4","team":"00000000-0000-4000-8000-00000000beef","expected_revision":8,"op":"observe_task","node":1,"observation":{"revision":"token-2","previous_revision":"token-1","stage":"in_progress","cancelled":false,"rank":"a00000001","title":"Task 1","attempt":0,"attempt_id":"","attempt_conflict":false}}','{"Success":{"ok":true,"revision":9}}');
INSERT INTO requests VALUES('2dc17a73-d13d-45cb-9df4-8c817b1388ab','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"2dc17a73-d13d-45cb-9df4-8c817b1388ab","team":"00000000-0000-4000-8000-00000000beef","expected_revision":9,"op":"observe_task","node":1,"observation":{"revision":"token-3","previous_revision":"token-2","stage":"in_review","cancelled":false,"rank":"a00000001","title":"Task 1","attempt":0,"attempt_id":"","attempt_conflict":false}}','{"Success":{"ok":true,"revision":10}}');
INSERT INTO requests VALUES('5a6730a5-36cc-4d55-965d-52251b443b11','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"5a6730a5-36cc-4d55-965d-52251b443b11","team":"00000000-0000-4000-8000-00000000beef","expected_revision":10,"op":"record_report","node":1,"generation":1,"member":"m1","text":"Implemented and checked","commit":"0123456789abcdef0123456789abcdef01234567"}','{"Success":{"ok":true,"revision":11}}');
INSERT INTO requests VALUES('7c2de677-a21f-4d4f-95c8-a51f1fb48b65','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"7c2de677-a21f-4d4f-95c8-a51f1fb48b65","team":"00000000-0000-4000-8000-00000000beef","expected_revision":11,"op":"stage_task_intent","node":2,"effect_request_id":"e69287f0-a482-444d-9c2d-9415d06a899e"}','{"Success":{"ok":true,"revision":12,"node":2}}');
INSERT INTO requests VALUES('f7707773-5e31-4f2d-bb52-855a42595227','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"f7707773-5e31-4f2d-bb52-855a42595227","team":"00000000-0000-4000-8000-00000000beef","expected_revision":12,"op":"stage_task_outcome","effect_request_id":"e69287f0-a482-444d-9c2d-9415d06a899e","outcome":{"status":"created","task":{"project":"example-project","uuid":"00000000-0000-4000-8000-0000000003ea"}}}','{"Success":{"ok":true,"revision":13}}');
INSERT INTO requests VALUES('4ea03994-b4cc-40b6-86b3-663594a5e391','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"4ea03994-b4cc-40b6-86b3-663594a5e391","team":"00000000-0000-4000-8000-00000000beef","expected_revision":13,"op":"observe_task","node":2,"observation":{"revision":"token-4","previous_revision":"","stage":"todo","cancelled":false,"rank":"a00000002","title":"Task 2","attempt":0,"attempt_id":"","attempt_conflict":false}}','{"Success":{"ok":true,"revision":14}}');
INSERT INTO requests VALUES('71b76514-73a3-4df2-aecc-65870d43a793','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"71b76514-73a3-4df2-aecc-65870d43a793","team":"00000000-0000-4000-8000-00000000beef","expected_revision":14,"op":"handout_intent","node":2,"member":"m3","effect_request_id":"d50ecdac-87ae-4c4e-83e2-986bda94bd00"}','{"Success":{"ok":true,"revision":15,"node":2,"item":2}}');
INSERT INTO requests VALUES('a150d896-b868-4eff-a5ac-2130c32c9583','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"a150d896-b868-4eff-a5ac-2130c32c9583","team":"00000000-0000-4000-8000-00000000beef","expected_revision":15,"op":"handout_outcome","effect_request_id":"d50ecdac-87ae-4c4e-83e2-986bda94bd00","outcome":{"status":"submitted","receipt":"receipt-example"}}','{"Success":{"ok":true,"revision":16}}');
INSERT INTO requests VALUES('6dff4cb8-9b39-4a30-a0dc-6d5114e9c9bf','00000000-0000-4000-8000-00000000beef','{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','{"request_id":"6dff4cb8-9b39-4a30-a0dc-6d5114e9c9bf","team":"00000000-0000-4000-8000-00000000beef","expected_revision":16,"op":"observe_task","node":2,"observation":{"revision":"token-5","previous_revision":"token-4","stage":"in_progress","cancelled":false,"rank":"a00000002","title":"Task 2","attempt":0,"attempt_id":"","attempt_conflict":false}}','{"Success":{"ok":true,"revision":17}}');
INSERT INTO requests VALUES('4928bf1a-1a0c-4e8d-ab93-111e32da3f51','00000000-0000-4000-8000-00000000beef','{"kind":"session","person":"local","team":"00000000-0000-4000-8000-00000000beef","member":"m3","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000033"}}','{"request_id":"4928bf1a-1a0c-4e8d-ab93-111e32da3f51","team":"00000000-0000-4000-8000-00000000beef","expected_revision":17,"op":"verdict","node":2,"generation":1,"pass":true,"text":"Checked the criteria"}','{"Success":{"ok":true,"revision":18,"item":1}}');
CREATE TABLE pipeline_runs (
 team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 state TEXT NOT NULL, stage INTEGER,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
INSERT INTO pipeline_runs VALUES('00000000-0000-4000-8000-00000000beef',1,1,'person',NULL);
CREATE TABLE stage_tasks (
 team TEXT NOT NULL, node INTEGER NOT NULL, target INTEGER NOT NULL,
 generation INTEGER NOT NULL, state TEXT NOT NULL DEFAULT 'wanted',
 request_id TEXT UNIQUE, outcome TEXT, created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
INSERT INTO stage_tasks VALUES('00000000-0000-4000-8000-00000000beef',2,1,1,'finished','e69287f0-a482-444d-9c2d-9415d06a899e','{"status":"created","task":{"project":"example-project","uuid":"00000000-0000-4000-8000-0000000003ea"}}',1234);
CREATE TABLE verdicts (
 team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 actor TEXT NOT NULL, pass INTEGER NOT NULL, text TEXT NOT NULL,
 PRIMARY KEY(team,node,generation), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
INSERT INTO verdicts VALUES('00000000-0000-4000-8000-00000000beef',2,1,'{"kind":"session","person":"local","team":"00000000-0000-4000-8000-00000000beef","member":"m3","session":{"machine":"00000000-0000-4000-8000-0000000000aa","session_id":"launch_id:00000000-0000-4000-8000-000000000033"}}',1,'Checked the criteria');
CREATE TABLE task_observations (
 team TEXT NOT NULL, node INTEGER NOT NULL, data TEXT NOT NULL,
 released INTEGER NOT NULL DEFAULT 0,
 attempt INTEGER NOT NULL DEFAULT 0, attempt_id TEXT NOT NULL DEFAULT '',
 last_attempt_id TEXT NOT NULL DEFAULT '', attempt_conflict INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
INSERT INTO task_observations VALUES('00000000-0000-4000-8000-00000000beef',1,'{"revision":"token-3","previous_revision":"token-2","stage":"in_review","cancelled":false,"rank":"a00000001","title":"Task 1","attempt":0,"attempt_id":"","attempt_conflict":false}',0,0,'','',0);
INSERT INTO task_observations VALUES('00000000-0000-4000-8000-00000000beef',2,'{"revision":"token-5","previous_revision":"token-4","stage":"in_progress","cancelled":false,"rank":"a00000002","title":"Task 2","attempt":0,"attempt_id":"","attempt_conflict":false}',0,0,'','',0);
CREATE TABLE timers_extension (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL);
INSERT INTO timers_extension VALUES(1,8);
CREATE TABLE key_clear_teams (team TEXT PRIMARY KEY, pending INTEGER NOT NULL);
CREATE TABLE key_clear_work (
 team TEXT NOT NULL, node INTEGER NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('stage','stop')),
 PRIMARY KEY(team,node)
);
CREATE TABLE timer_firings (
 team TEXT NOT NULL, kind TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 item INTEGER NOT NULL, target TEXT NOT NULL CHECK(target IN ('inbox','card')),
 seq INTEGER NOT NULL, subject INTEGER NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,kind,node,generation,item)
);
CREATE TABLE effect_outcomes (
 team TEXT NOT NULL, request_id TEXT NOT NULL, at_ms INTEGER NOT NULL,
 PRIMARY KEY(team,request_id)
);
INSERT INTO effect_outcomes VALUES('00000000-0000-4000-8000-00000000beef','84914d26-2422-49a4-9d05-4dc3be3b2eec',1234);
INSERT INTO effect_outcomes VALUES('00000000-0000-4000-8000-00000000beef','d50ecdac-87ae-4c4e-83e2-986bda94bd00',1234);
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
CREATE TABLE cancel_outcomes (
 decision INTEGER PRIMARY KEY, team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('cancelled','refused','released')), reason TEXT NOT NULL, at_ms INTEGER NOT NULL
);
PRAGMA writable_schema=ON;
CREATE TABLE IF NOT EXISTS sqlite_sequence(name,seq);
DELETE FROM sqlite_sequence;
INSERT INTO sqlite_sequence VALUES('journal',18);
INSERT INTO sqlite_sequence VALUES('effects',2);
INSERT INTO sqlite_sequence VALUES('decisions',1);
INSERT INTO sqlite_sequence VALUES('cards',1);
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
CREATE INDEX key_clear_pending ON key_clear_teams(team) WHERE pending=1;
CREATE INDEX key_clear_work_kind ON key_clear_work(team,kind,node);
CREATE INDEX timer_card_subject ON timer_firings(team,subject) WHERE target='card';
CREATE INDEX timer_card_node ON timer_firings(team,node) WHERE target='card';
CREATE INDEX key_refusals ON key_clears(team,node) WHERE state='refused';
CREATE INDEX cancel_refusals ON cancel_outcomes(team,node) WHERE state='refused';
CREATE INDEX decisions_attempt ON decisions(team,generation,node);
PRAGMA writable_schema=OFF;
COMMIT;
PRAGMA user_version=2;
