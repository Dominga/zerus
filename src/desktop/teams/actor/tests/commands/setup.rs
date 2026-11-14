use super::*;
use crate::teams::ops::{HandoutOutcome, Operation};
impl Fixture {
    pub(super) fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let h = Harness::empty();
        std::fs::set_permissions(h.temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        crate::platform::private_dir(&h.temp.path().join("config")).unwrap();
        let config = Config {
            home: h.temp.path().display().to_string(),
            dir: h.temp.path().join("config"),
            state: h.temp.path().into(),
            selves: vec!["fixture".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        let mut f = Self {
            h,
            config,
            cli: std::env::var_os("HGS_TEST_BIN")
                .expect("the explicit real-command matrix needs HGS_TEST_BIN")
                .into(),
            task: TaskRef {
                project: "example-project".into(),
                uuid: String::new(),
            },
            node: 0,
            id: String::new(),
        };
        let initialized=f.process(&["swarm".into(),"initialize".into()],r##"{"projects":[{"id":"example-project","name":"example","color":"#123456","sessions":[],"folders":[]}]}"##,None);
        assert!(
            initialized.status.success(),
            "{}",
            String::from_utf8_lossy(&initialized.stderr)
        );
        let machine = crate::swarm::local_node_id(&f.config).unwrap();
        let mut op = create();
        if let Operation::Create { home, .. } = &mut op {
            *home = machine.clone();
        }
        f.h.run(&person(), op).unwrap();
        for member in ["lead", "m1", "m2", "m3"] {
            f.bind(member, &machine);
        }
        f.add_task(&machine);
        f
    }
    fn add_task(&mut self, machine: &str) {
        let tasks =
            ProjectStore::open(&self.config.state.join("replica"), &self.task.project).unwrap();
        let mut doc = tasks
            .new_task(
                "Work",
                Stage::Todo,
                Target::Any,
                TaskActor::Person,
                &Position::End,
                "example",
                1,
            )
            .unwrap();
        self.task.uuid = doc.uuid().into();
        tasks.save(&mut doc).unwrap();
        drop(tasks);
        self.node = crate::teams::tasks::send(
            &crate::teams::tasks::Caller {
                state: &self.config.state,
                machine,
                actor: TaskActor::Person,
            },
            &self.task,
            TEAM,
            None,
            None,
        )
        .unwrap()
        .node
        .unwrap();
        self.id = self.h.intent(self.node, "m1");
    }
    fn bind(&mut self, member: &str, machine: &str) {
        use sha2::{Digest, Sha256};
        let launch = uuid::Uuid::new_v4().to_string();
        let name = format!("codex/command-{member}");
        let key = SessionKey {
            machine: machine.into(),
            session: name.clone(),
            session_id: format!("launch_id:{launch}"),
        };
        let sample = fixture()
            .members
            .into_iter()
            .find(|m| m.id == member)
            .unwrap();
        self.h
            .run(
                &service(),
                Operation::SetMemberSession {
                    member: member.into(),
                    session: zerus_team_plan::model::SessionKey {
                        machine: machine.into(),
                        session_id: key.session_id.clone(),
                    },
                    account: "fixture".into(),
                    worktree: sample.worktree,
                    ownership_nonce: uuid::Uuid::new_v4().to_string(),
                },
            )
            .unwrap();
        private_write(&self.config.state.join(format!("{:x}.json",Sha256::digest(name.as_bytes()))),json!({"version":1,"agent":"codex","name":name,"launch_id":launch,"run_id":format!("run-{member}"),"conversation_id":format!("conversation-{member}"),"activity":"idle"}).to_string().as_bytes());
    }
    pub(super) fn submitted(&mut self) {
        let id = self.id.clone();
        let reply = self.delivery(&id, "m1", false);
        assert_eq!(reply["state"], "submitted");
        self.h.outcome(
            &id,
            HandoutOutcome::Submitted {
                receipt: "fixture receipt".into(),
            },
        );
        crate::teams::tasks::observe(&self.config.state, &self.task).unwrap();
    }
    pub(super) fn reported(&mut self) {
        let tasks =
            ProjectStore::open(&self.config.state.join("replica"), &self.task.project).unwrap();
        let mut doc = tasks.load(&self.task.uuid).unwrap();
        let holder = doc
            .view()
            .unwrap()
            .execution
            .unwrap()
            .current
            .unwrap()
            .executor
            .unwrap();
        let actor = TaskActor::Agent(holder);
        doc.begin_attempt(&self.id, &actor, chrono::Utc::now().timestamp_millis())
            .unwrap();
        let reported = doc
            .report_attempt(
                &self.id,
                "Work is ready",
                zerus_replica::execution::ReportKind::Ready,
                None,
                &actor,
                chrono::Utc::now().timestamp_millis(),
            )
            .unwrap();
        assert!(
            reported.moves,
            "holder report was history only: {reported:?}"
        );
        tasks.save(&mut doc).unwrap();
        drop(tasks);
        crate::teams::tasks::observe(&self.config.state, &self.task).unwrap();
        let generation = self.h.store.generation(TEAM, self.node).unwrap().unwrap();
        self.h
            .run(
                &service(),
                Operation::RecordReport {
                    node: self.node,
                    generation,
                    member: "m1".into(),
                    text: "Work is ready".into(),
                    commit: COMMIT.into(),
                    notes: Vec::new(),
                },
            )
            .unwrap();
    }
}
