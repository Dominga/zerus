//! Real owned pipes with fake protocol replies. No native caller, genuine
//! checkout manifest, FD loan, program stop or removal is qualified here.
use super::*;
use std::{
    cell::RefCell,
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
};

const SCRIPT: &str = r#"#!/usr/bin/python3
import json,os,pathlib,sys,time
root=pathlib.Path(__file__).parent
args=sys.argv[1:]
assert args.pop(0)=='--desktop-helper-rendezvous-v1'
assert sys.stdin.buffer.readline()==b'HGS-ORIGINAL-CHILD-V1\n'
with (root/'calls').open('a') as out: out.write(json.dumps(args)+'\n')
if args==['--help']:
    print('Worktree program custody ABI: worktree-program-custody-v1' if not (root/'unsupported').exists() else 'legacy help',flush=True)
    sys.exit(0)
assert args[:1]==['--expect-machine'] and args[2:]==['worktrees','program-holder','--json']
first=json.loads(sys.stdin.buffer.readline())
assert first['op']=='open'
manifest=json.loads((root/'manifest').read_text())
manifest['capture']=first['discovery']['capture']
manifest['issued']=int(time.time());manifest['expires']=manifest['issued']+300
print(json.dumps(dict(op='ready',abi='worktree-program-custody-v1',pipeline=first['pipeline'],manifest=manifest,
    holder=dict(pid=os.getpid(),parent=os.getppid(),birth='fixture-data',audit=None),socket='/tmp/example/pins')),flush=True)
frame=json.loads(sys.stdin.buffer.readline())
assert frame==dict(op='close',pipeline=first['pipeline'])
if (root/'hold-output').exists() and os.fork()==0:
    until=time.monotonic()+20
    while not (root/'release').exists() and time.monotonic()<until:time.sleep(.005)
    os._exit(0)
print(json.dumps(dict(op='closed',pipeline=first['pipeline'])),flush=True)
"#;
struct Fixture(PathBuf, RefCell<Option<std::sync::Weak<()>>>);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("zerus-worktree-owner-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::write(root.join("hgs"), SCRIPT).unwrap();
        fs::set_permissions(root.join("hgs"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            root.join("manifest"),
            serde_json::to_vec(&crate::worktree_programs::tests::manifest()).unwrap(),
        )
        .unwrap();
        Self(root, RefCell::new(None))
    }
    fn owner(&self) -> (Owner, std::sync::Weak<()>) {
        let source = Arc::new(());
        let released = Arc::downgrade(&source);
        *self.1.borrow_mut() = Some(released.clone());
        let owner = Owner::begin(
            &self.0.join("hgs"),
            Control::Open {
                pipeline: "99999999-9999-4999-8999-999999999999".into(),
                discovery: crate::worktree_programs::Discovery {
                    capture: crate::worktree_programs::tests::manifest().capture,
                },
            },
            "a".repeat(64),
            FrozenEnvironment::capture().unwrap(),
            source,
            Arc::new(Command::spawn),
        )
        .unwrap();
        (owner, released)
    }
    fn calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.0.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.0.join("release"), b"release");
        let until = Instant::now() + Duration::from_secs(10);
        while self
            .1
            .get_mut()
            .as_ref()
            .is_some_and(|w| w.upgrade().is_some())
            && Instant::now() < until
        {
            thread::sleep(POLL);
        }
        // Preserve the release marker rather than strand a still-charged
        // failing fixture. Its original child has its own bounded watchdog.
        if !self
            .1
            .get_mut()
            .as_ref()
            .is_some_and(|w| w.upgrade().is_some())
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
#[test]
fn the_original_duplex_owner_probes_first_keeps_input_and_joins_every_pipe_on_close() {
    let fixture = Fixture::new();
    let (mut owner, released) = fixture.owner();
    assert!(matches!(owner.ready().unwrap(), Reply::Ready { .. }));
    assert!(released.upgrade().is_some());
    assert_eq!(fixture.calls()[0], ["--help"]);
    assert_eq!(
        &fixture.calls()[1][2..],
        ["worktrees", "program-holder", "--json"]
    );
    owner.close().unwrap();
    assert!(
        released.upgrade().is_some(),
        "the owner separately retains its original native source"
    );
    drop(owner);
    assert!(released.upgrade().is_none());
}
#[test]
fn a_legacy_image_never_receives_a_new_subcommand_or_a_replacement_selection() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("unsupported"), b"legacy").unwrap();
    let (mut owner, released) = fixture.owner();
    assert_eq!(owner.ready(), Err(Error::Unsupported));
    assert_eq!(fixture.calls(), [vec!["--help".to_string()]]);
    drop(owner);
    assert!(released.upgrade().is_none());
}
#[test]
fn closed_reply_and_process_exit_cannot_release_a_real_held_reader_charge() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("hold-output"), b"hold").unwrap();
    let (mut owner, released) = fixture.owner();
    owner.ready().unwrap();
    assert_eq!(owner.close(), Err(Error::Unsettled));
    drop(owner);
    assert!(
        released.upgrade().is_some(),
        "closed ACK is not physical EOF"
    );
    fs::write(fixture.0.join("release"), b"release").unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while released.upgrade().is_some() && Instant::now() < until {
        thread::sleep(POLL);
    }
    assert!(released.upgrade().is_none());
}
