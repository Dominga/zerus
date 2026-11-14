//! Real linked worktree controls from the independent review.
use super::*;
use std::{
    path::{Path, PathBuf},
    process::Command,
};
pub(super) struct Fixture {
    pub _temp: tempfile::TempDir,
    pub config: Config,
    pub b: Binding,
}
fn git(at: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(at)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn repository(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let repo = root.join("repository");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
    let folder = root.join("member");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            folder.to_str().unwrap(),
        ],
    );
    let common = repo.join(".git");
    let metadata = PathBuf::from(git(&folder, &["rev-parse", "--absolute-git-dir"]));
    (folder, common, metadata)
}
impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (folder, common, metadata) = repository(&root);
        let state = root.join("state");
        crate::platform::private_dir(&state.join("teams")).unwrap();
        std::fs::write(state.join("teams/preview"), "on").unwrap();
        crate::platform::private_dir(&state.join("worktree-cache")).unwrap();
        let nonce = uuid::Uuid::new_v4().to_string();
        let owner = json!({"version":1,"id":nonce,"common_dir":common,"path":folder,"metadata":metadata,"branch":"fixture","head":"fixture","created_at":1});
        std::fs::write(metadata.join("zerus-owner.json"), owner.to_string()).unwrap();
        std::fs::write(
            crate::state::team_worktree_receipt_path(&state, &common, &folder),
            owner.to_string(),
        )
        .unwrap();
        let (mut b, _, _) = super::super::policy::tests::sample();
        b.folder = folder.to_str().unwrap().into();
        b.common = common.to_str().unwrap().into();
        b.nonce = nonce;
        b.launch = uuid::Uuid::new_v4().to_string();
        b.session = uuid::Uuid::new_v4().to_string();
        let config = Config {
            home: String::new(),
            dir: root.join("config"),
            state,
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        Self {
            _temp: temp,
            config,
            b,
        }
    }
    pub fn launch_plan(&self) -> super::super::super::ports::Plan {
        let b = &self.b;
        super::super::super::ports::Plan {
            id: uuid::Uuid::new_v4().to_string(),
            team: b.team.clone(),
            person: b.person.clone(),
            machine: b.machine.clone(),
            project: "example-project".into(),
            member: b.member.clone(),
            role: b.role.clone(),
            agent: b.agent.clone(),
            account: b.account.clone(),
            model: String::new(),
            effort: String::new(),
            base_commit: String::new(),
            placement: super::super::super::ports::Placement {
                source: Path::new(&b.common)
                    .parent()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .into(),
                git_dir: b.common.clone(),
                branch: "fixture".into(),
                destination: b.folder.clone(),
            },
            parent: None,
            launch_id: b.launch.clone(),
            session_name: String::new(),
        }
    }
    pub fn record(&self) {
        let mut db = super::super::super::ledger::Ledger::open(&self.config.state).unwrap();
        let tree = super::super::super::ports::Worktree {
            path: self.b.folder.clone(),
            ownership_nonce: self.b.nonce.clone(),
        };
        db.record_checkout(&self.launch_plan(), &tree).unwrap();
    }
    fn admitted(&self) -> bool {
        let mut store = Store::open(&self.config.state).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let mut native = Native::new(&self.config, &self.b.machine, &stop, &mut store).unwrap();
        let o = native.owned(&self.b).unwrap();
        super::super::policy::ownership(&self.b, &o).is_ok()
    }
    fn replace(&self) {
        let folder = Path::new(&self.b.folder);
        let moved = folder.with_file_name("original-member");
        std::fs::rename(folder, &moved).unwrap();
        std::fs::create_dir(folder).unwrap();
        std::fs::rename(moved.join(".git"), folder.join(".git")).unwrap();
    }
}
#[test]
fn a_launch_without_directory_object_evidence_is_manual() {
    let f = Fixture::new();
    assert!(!f.admitted());
}
#[test]
fn a_replaced_checkout_with_its_original_git_link_is_manual_before_first_read() {
    let f = Fixture::new();
    f.record();
    assert!(f.admitted());
    f.replace();
    assert!(!f.admitted());
    // A later confirmation pass/restart cannot use the replacement as its baseline.
    assert!(!f.admitted());
}

#[test]
fn launch_object_proof_is_write_once_and_retains_birth_when_available() {
    let f = Fixture::new();
    f.record();
    let plan = f.launch_plan();
    let mut db = super::super::super::ledger::Ledger::open(&f.config.state).unwrap();
    let before = db
        .checkout(&f.b.team, &f.b.member, &f.b.launch)
        .unwrap()
        .unwrap();
    let saved = serde_json::to_value(&before).unwrap();
    let metadata = std::fs::metadata(&f.b.folder).unwrap();
    if metadata.created().is_ok() {
        assert!(!saved["birth"].is_null());
    }
    let tree = super::super::super::ports::Worktree {
        path: f.b.folder.clone(),
        ownership_nonce: f.b.nonce.clone(),
    };
    assert!(!db.record_checkout(&plan, &tree).unwrap());
    f.replace();
    assert!(db.record_checkout(&plan, &tree).is_err());
    assert_eq!(
        db.checkout(&f.b.team, &f.b.member, &f.b.launch)
            .unwrap()
            .unwrap(),
        before
    );
}
