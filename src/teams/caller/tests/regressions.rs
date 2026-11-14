//! Real-process review regressions. Each proof runs in a fresh process under
//! a detached helper, outside the test runner's own Zerus agent ancestry.
//! Whoever adopts a detached process depends on the machine: a subreaper of
//! this user (`systemd --user`) on a Linux desktop, PID 1 in hosted CI, an ssh
//! session or a container, launchd on macOS. Where a foreign process adopts
//! the helper, the helper is the top of the walk, and the top may have an
//! unreadable environment. The modes in [`KEPT`] give the helper an
//! environment the walk cannot read, so their helper runs below a keeper of
//! this user that outlives the probe: it is never the top (zerus-e8e6m).
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NAME: &str = "claude/example/review";
const RUN: &str = "run-review";
const ID: &str = "00000000-0000-4000-8000-0000000000c1";
const HELPER: &str = r#"
import os,sys,time,subprocess,ctypes
from pathlib import Path
base=Path(sys.argv[1]); mode=sys.argv[2]; exe=sys.argv[3]
(base/'pid').write_text(str(os.getpid()))
while not (base/'go').exists(): time.sleep(.01)
if mode=='unreadable':
    assert ctypes.CDLL(None).prctl(4,0,0,0,0)==0
keep=mode in ('marked','orphan-marked','withheld-middle','readable-middle','member-shell') or mode.startswith('id-marked') or mode.startswith('hosted-')
child=dict(os.environ)
for key in list(child):
    if (not keep and key in ['HGS_SESSION','HGS_RUN_ID','HGS_SESSION_ID']) or key.startswith('PAD_'): child.pop(key,None)
if mode=='partial': child['HGS_SESSION']='claimed'
if mode=='empty': child['HGS_RUN_ID']=''
cmd=[exe,'--exact','teams::caller::tests::probe','--nocapture']
if mode in ('withheld-middle','withheld-person','member-shell'):
    strip='env -u HGS_SESSION -u HGS_RUN_ID -u HGS_SESSION_ID ' if mode=='withheld-middle' else ''
    hold='while [ ! -f "$1/recorded" ]; do sleep 0.01; done; ' if mode=='withheld-middle' else ''
    cmd=['/bin/sh','-c','echo $$ > "$1/shell"; '+hold+strip+'"$0" --exact teams::caller::tests::probe --nocapture; exit $?',exe,str(base)]
if mode in ('readable-middle','readable-person'):
    code="""
import os,sys,time,subprocess
from pathlib import Path
base,exe,mode=Path(sys.argv[1]),sys.argv[2],sys.argv[3]
(base/'shell').write_text(str(os.getpid()))
if mode=='readable-middle':
    while not (base/'recorded').exists(): time.sleep(.01)
leaf=dict(os.environ)
for key in ('HGS_SESSION','HGS_RUN_ID','HGS_SESSION_ID'): leaf.pop(key,None)
sys.exit(subprocess.call([exe,'--exact','teams::caller::tests::probe','--nocapture'],env=leaf))
"""
    cmd=[sys.executable,'-c',code,str(base),exe,mode]
if mode=='intermediate':
    code="import os,sys,subprocess; e=dict(os.environ); [e.pop(k,None) for k in ['HGS_SESSION','HGS_RUN_ID','HGS_SESSION_ID']]; p=subprocess.run([sys.argv[1],'--exact','teams::caller::tests::probe','--nocapture'],env=e,capture_output=True,text=True,timeout=10); print(p.stdout+p.stderr)"
    cmd=[sys.executable,'-c',code,exe]
    child=dict(os.environ)
def adopted():
    out=base/'adopted'
    middle=os.fork()
    if middle==0:
        me=os.getpid()
        if os.fork()==0:
            fd=os.open(str(out),os.O_WRONLY|os.O_CREAT|os.O_TRUNC,0o600)
            os.dup2(fd,1); os.dup2(fd,2)
            until=time.time()+10
            while os.getppid()==me and time.time()<until: time.sleep(.01)
            os.write(1,('ADOPTED from=%d to=%d\n'%(me,os.getppid())).encode())
            os.execve(exe,cmd,child)
        os._exit(0)
    os.waitpid(middle,0)
    until=time.time()+15
    text=''
    while time.time()<until:
        text=out.read_text() if out.exists() else ''
        if 'test result' in text: break
        time.sleep(.01)
    return text
if mode.startswith('orphan'):
    text=adopted()
elif mode=='setsid':
    result=subprocess.run(cmd,env=child,capture_output=True,text=True,start_new_session=True,timeout=15)
    text=result.stdout+result.stderr
else:
    if mode=='nohup': cmd.insert(0,'nohup')
    result=subprocess.run(cmd,env=child,capture_output=True,text=True,timeout=15)
    text=result.stdout+result.stderr
(base/'parent').write_text(str(os.getppid()))
(base/'answer.tmp').write_text(text)
os.replace(base/'answer.tmp',base/'answer')
"#;

/// The modes whose helper has an environment the walk cannot read: a
/// nondumpable process and one over the 1 MiB cap. The walk tolerates that
/// only at the top, so these helpers run below a keeper.
const KEPT: [&str; 2] = ["unreadable", "oversize"];

/// A detached process of this user with only `PATH` in its environment: it
/// starts the helper that `helper.json` describes and waits for it, so the
/// helper's parent is this keeper until the helper has written its answer.
const KEEPER: &str = r#"
import json,os,subprocess,sys
from pathlib import Path
base=Path(sys.argv[1])
spec=json.loads((base/'helper.json').read_text())
(base/'keeper').write_text(str(os.getpid()))
subprocess.run(spec['argv'],env=spec['env'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
"#;

struct Tree {
    temp: tempfile::TempDir,
    state: PathBuf,
    pid: u32,
    birth: String,
    /// The keeper of a [`KEPT`] mode's helper.
    keeper: Option<u32>,
}

impl Tree {
    fn start(mode: &str, preview: Option<&str>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let config = temp.path().join("config");
        std::fs::create_dir(&state).unwrap();
        std::fs::create_dir(&config).unwrap();
        let state = std::fs::canonicalize(state).unwrap();
        let config = std::fs::canonicalize(config).unwrap();
        catalog(&config);
        // The helper's command line and environment; `detach` starts it.
        let mut command = Command::new("python3");
        command
            .arg("-c")
            .arg(HELPER)
            .arg(temp.path())
            .arg(mode)
            .arg(std::env::current_exe().unwrap())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", temp.path())
            .env("HGS_STATE_DIR", &state)
            .env("HGS_CONFIG_DIR", &config)
            .env("HGS_TEAMS_CALLER_PROBE", "1");
        if ![
            "clean-agent",
            "person",
            "partial",
            "empty",
            "withheld-person",
            "readable-person",
        ]
        .contains(&mode)
        {
            command.env("HGS_SESSION", NAME).env("HGS_RUN_ID", RUN);
        }
        if mode == "id-marked-home-symlink" {
            let linked = temp.path().join("linked-home");
            std::os::unix::fs::symlink(temp.path(), &linked).unwrap();
            command
                .env("HGS_CONFIG_DIR", linked.join("config"))
                .env("HGS_STATE_DIR", linked.join("state"));
        }
        if mode == "id-marked-symlink" {
            let linked = temp.path().join("linked-config");
            std::os::unix::fs::symlink(&config, &linked).unwrap();
            command.env("HGS_CONFIG_DIR", linked);
        }
        if mode.starts_with("id-") && !mode.ends_with("nohint") {
            command.env(
                "HGS_SESSION_ID",
                if mode.ends_with("badhint") {
                    "malformed"
                } else {
                    ID
                },
            );
        }
        if mode == "oversize" {
            for n in 0..18 {
                command.env(format!("PAD_{n}"), "x".repeat(64_000));
            }
        }
        if mode == "off" {
            command.env("HGS_TEAMS_DISPATCH_PROBE", "status");
        }
        if mode == "preview-status" {
            command.env("HGS_TEAMS_DISPATCH_PROBE", "preview status");
        }
        if mode == "panic" {
            command.env("HGS_TEAMS_PANIC_PROBE", "1");
        }
        if mode == "birth" {
            command.env("HGS_TEAMS_BIRTH_PROBE", "1");
        }
        if let Some(name) = hosted_name(mode) {
            command.env("HGS_SESSION", name);
        }
        if let Some(action) = preview {
            command.env("HGS_TEAMS_PREVIEW_PROBE", action);
        }
        let kept = KEPT.contains(&mode);
        detach(temp.path(), &command, kept);
        let pid_file = temp.path().join("pid");
        let pid = wait(|| {
            std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|p| p.trim().parse::<u32>().ok())
        });
        let helper = crate::platform::process_identity(pid).expect("the fixture helper's identity");
        let keeper = kept.then(|| {
            let keeper = wait(|| {
                std::fs::read_to_string(temp.path().join("keeper"))
                    .ok()
                    .and_then(|p| p.trim().parse::<u32>().ok())
            });
            assert_eq!(helper.parent, keeper, "the keeper is the helper's parent");
            // The walk's own rule: a process is the top when its parent is
            // foreign. The keeper is this user's, so the helper is not.
            assert!(
                matches!(
                    super::super::ancestry::inspect(keeper),
                    Ok(super::super::ancestry::Inspection::Own(_))
                ),
                "the helper's parent is a process of this user, so the helper is not the top"
            );
            keeper
        });
        Self {
            temp,
            state,
            pid,
            birth: helper.birth,
            keeper,
        }
    }

    fn born(&self) -> String {
        crate::platform::process_start_time(self.pid).unwrap()
    }

    /// Records `pid` and `birth` as this computer's running DeepSeek host,
    /// in the host folder `folder`, as `dsh/host_identity.rs` writes it, with
    /// this boot's identity.
    fn host(&self, folder: &str, pid: u32, birth: &str) {
        let record = json!({"state":"running","version":2,"pid":pid,"birth":birth,
            "pid_file":[1,2,3,4],"boot":crate::state::session_graph::written_boot()});
        self.identity(folder, &record);
        let dir = self.state.join("dsh/hosts").join(folder);
        std::fs::write(dir.join("host.pid"), pid.to_string()).unwrap();
    }

    /// Writes a host folder's identity record as given.
    fn identity(&self, folder: &str, record: &Value) {
        let dir = self.state.join("dsh/hosts").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("host.identity.json"), record.to_string()).unwrap();
    }

    /// OpenCode's own service file, registering `pid`, as OpenCode writes it.
    fn opencode_service(&self, pid: u32) -> PathBuf {
        let dir = self.temp.path().join(".local/state/opencode");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("service.json");
        let value =
            json!({"id":"fixture","version":"2.0.32","url":"http://127.0.0.1:4096","pid":pid});
        std::fs::write(&file, value.to_string()).unwrap();
        file
    }

    fn record(&self, pid: u32, birth: &str) -> PathBuf {
        let path = self
            .state
            .join(format!("{:x}.json", Sha256::digest(NAME.as_bytes())));
        let record = json!({"version":1,"name":NAME,"agent":"claude","run_id":RUN,
            "launch_id":"00000000-0000-4000-8000-0000000000c1",
            "session_id":"00000000-0000-4000-8000-0000000000c1",
            "conversation_id":"00000000-0000-4000-8000-0000000000c2",
            "pid":pid,"process_start":birth,"state":"running","pane":"%0"});
        std::fs::write(&path, record.to_string()).unwrap();
        path
    }

    fn answer(&self) -> String {
        std::fs::write(self.temp.path().join("go"), b"go").unwrap();
        let text = wait(|| {
            let text = std::fs::read_to_string(self.temp.path().join("answer")).ok()?;
            text.lines()
                .any(|line| line.starts_with("PROBE ") || line.starts_with("DISPATCH "))
                .then_some(text)
        });
        if let Some(keeper) = self.keeper {
            // Adoption is final: a helper that still has its keeper as its
            // parent after the probe had it throughout the probe.
            let parent = std::fs::read_to_string(self.temp.path().join("parent")).unwrap();
            assert_eq!(
                parent.trim(),
                keeper.to_string(),
                "the helper's parent: {text}"
            );
        }
        text
    }
}

/// Starts `helper` detached from the test runner: a shell puts it in the
/// background and exits. A [`KEPT`] helper is started by a keeper instead,
/// which the shell puts in the background; `helper` is written to
/// `helper.json` in `base` for it.
fn detach(base: &Path, helper: &Command, kept: bool) {
    let mut shell = Command::new("sh");
    shell
        .args(["-c", "\"$@\" </dev/null >/dev/null 2>&1 &", "fixture"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let envs = helper
        .get_envs()
        .filter_map(|(key, value)| Some((key, value?)));
    if kept {
        let text = |value: &OsStr| value.to_str().expect("a UTF-8 fixture value").to_owned();
        let argv: Vec<String> = std::iter::once(helper.get_program())
            .chain(helper.get_args())
            .map(text)
            .collect();
        let env: BTreeMap<String, String> =
            envs.map(|(key, value)| (text(key), text(value))).collect();
        let spec = json!({"argv": argv, "env": env});
        std::fs::write(base.join("helper.json"), spec.to_string()).unwrap();
        shell
            .args(["python3", "-c", KEEPER])
            .arg(base)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    } else {
        shell
            .arg(helper.get_program())
            .args(helper.get_args())
            .envs(envs);
    }
    assert!(shell.status().unwrap().success());
}

impl Drop for Tree {
    fn drop(&mut self) {
        // Only this fixture's helper, still waiting for its go file.
        if !self.temp.path().join("go").exists()
            && crate::platform::process_identity(self.pid).is_some_and(|p| p.birth == self.birth)
        {
            unsafe {
                libc::kill(self.pid as i32, libc::SIGTERM);
            }
        }
    }
}

fn wait<T>(mut read: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        if let Some(value) = read() {
            return value;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("fixture did not settle within its bound")
}

fn refused(text: &str) {
    assert!(text.contains("PROBE refused:"), "{text}");
    println!("{text}");
}
fn person(text: &str) {
    assert!(text.contains("PROBE person"), "{text}");
    println!("{text}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_nondumpable_same_user_ancestor_refuses() {
    let tree = Tree::start("unreadable", None);
    tree.record(tree.pid, &tree.born());
    refused(&tree.answer());
}

#[cfg(target_os = "linux")]
#[test]
fn an_environment_over_the_cap_refuses() {
    let tree = Tree::start("oversize", None);
    tree.record(tree.pid, &tree.born());
    let text = tree.answer();
    assert!(text.contains("PROBE refused:invalid_state"), "{text}");
}

#[test]
fn a_corrupt_claimed_record_refuses_instead_of_becoming_the_person() {
    let tree = Tree::start("normal", None);
    let path = tree.record(tree.pid, &tree.born());
    std::fs::write(path, b"not json").unwrap();
    refused(&tree.answer());
}

#[test]
fn an_agent_started_without_markers_is_the_documented_interim_limit() {
    let tree = Tree::start("clean-agent", None);
    tree.record(tree.pid, &tree.born());
    person(&tree.answer());
}

#[test]
fn a_plain_persons_terminal_is_person() {
    person(&Tree::start("person", None).answer());
}

#[test]
fn partial_or_empty_markers_refuse() {
    for mode in ["partial", "empty"] {
        refused(&Tree::start(mode, None).answer());
    }
}

#[test]
fn setsid_nohup_and_cleaned_helper_environment_do_not_hide_a_marked_agent() {
    for mode in ["setsid", "nohup", "normal", "intermediate"] {
        let tree = Tree::start(mode, None);
        tree.record(tree.pid, &tree.born());
        refused(&tree.answer());
    }
}

#[test]
fn a_stale_birth_is_not_a_live_agent() {
    let tree = Tree::start("normal", None);
    tree.record(tree.pid, "stale-birth");
    person(&tree.answer());
}

/// The walk and the record check read the agent's birth through one
/// interface: the birth the walk read in the probe equals the one this test
/// reads, the record check accepts it, and one unit later is not the agent.
#[test]
fn the_record_check_matches_only_the_birth_the_walk_read() {
    let tree = Tree::start("birth", None);
    tree.record(tree.pid, &tree.born());
    let text = tree.answer();
    let line = format!(
        "BIRTH pid={} birth={} walk=agent later=none",
        tree.pid, tree.birth
    );
    assert!(text.contains(&line), "{text}");
    assert!(text.contains("PROBE refused:not_allowed"), "{text}");
    println!("{text}");
}

/// A caller whose parent exited is adopted (by launchd on macOS) and no
/// longer descends from its live agent. A marked caller is refused; an
/// unmarked one is the documented reparenting limit of this interim rule.
#[test]
fn a_caller_adopted_after_its_parent_exited_no_longer_descends_from_its_agent() {
    for mode in ["orphan-marked", "orphan"] {
        let tree = Tree::start(mode, None);
        tree.record(tree.pid, &tree.born());
        let text = tree.answer();
        let (from, to) = text
            .lines()
            .find_map(|line| line.strip_prefix("ADOPTED from="))
            .and_then(|rest| rest.split_once(" to="))
            .map(|(from, to)| (from.parse::<u32>().unwrap(), to.parse::<u32>().unwrap()))
            .unwrap_or_else(|| panic!("{text}"));
        assert!(from != to && to != tree.pid, "{text}");
        #[cfg(target_os = "macos")]
        assert_eq!(to, 1, "launchd adopts an orphan on macOS: {text}");
        if mode == "orphan-marked" {
            refused(&text);
        } else {
            person(&text);
        }
    }
}

/// The Zerus window's shape: the window runs `hgs` as its own child, a
/// visible process with no shell between. A person switches the preview
/// through the real dispatcher and the real proof.
#[test]
fn the_zerus_window_shape_switches_the_preview_on_and_off() {
    for action in ["on", "off"] {
        let tree = Tree::start("person", Some(action));
        if action == "off" {
            std::fs::create_dir(tree.state.join("teams")).unwrap();
            std::fs::write(tree.state.join("teams/preview"), b"on").unwrap();
        }
        let text = tree.answer();
        assert!(text.contains(&format!("PARENT {}", tree.pid)), "{text}");
        person(&text);
        assert!(text.contains("PREVIEW 0"), "{text}");
        assert_eq!(crate::teams::enabled(&tree.state), action == "on");
    }
}

/// The process ID of the `/bin/sh` that a shell mode puts below the helper.
fn shell(tree: &Tree) -> u32 {
    wait(|| {
        std::fs::read_to_string(tree.temp.path().join("shell"))
            .ok()
            .and_then(|pid| pid.trim().parse::<u32>().ok())
    })
}

/// A system shell between a person's command and the top. When the
/// system withholds the shell's environment, the walk cannot tell whether
/// it is an agent and refuses with the sentence that names the cause and the
/// Zerus window: when that shell is a recorded live agent, and when nothing in
/// the tree is marked (a person typing into a system shell below the top).
/// When the shell is readable, the agent is found, and the unmarked tree
/// is the person.
#[test]
fn a_withheld_shell_below_the_top_refuses_a_persons_command() {
    for mode in [
        "withheld-middle",
        "withheld-person",
        "readable-middle",
        "readable-person",
    ] {
        let tree = Tree::start(mode, None);
        std::fs::write(tree.temp.path().join("go"), b"go").unwrap();
        let shell = shell(&tree);
        if mode.ends_with("-middle") {
            let start = crate::platform::process_start_time(shell).unwrap();
            tree.record(shell, &start);
            std::fs::write(tree.temp.path().join("recorded"), b"").unwrap();
        }
        let text = tree.answer();
        assert!(text.contains(&format!("PARENT {shell}")), "{text}");
        if !environment_readable(&text, shell) {
            assert!(text.contains("PROBE refused:not_allowed"), "{text}");
            assert!(
                text.contains("below a program that hides its environment, usually a system shell; run it from the Zerus window"),
                "{text}"
            );
        } else if mode.ends_with("-middle") {
            assert!(text.contains("PROBE refused:not_allowed"), "{text}");
            assert!(
                text.contains("A live agent session runs this command"),
                "{text}"
            );
        } else {
            person(&text);
        }
        println!("WITHHELD mode={mode} {text}");
    }
}

/// The ordinary member's shape: agent, a shell whose environment macOS
/// withholds, then `hgs`. The marked path reads only its own environment and
/// proves its descent natively, so it is that session on every platform. This
/// pins that the marked path never comes to depend on ancestors' environments.
#[test]
fn a_member_below_a_withheld_shell_is_that_session() {
    let tree = Tree::start("member-shell", None);
    tree.record(tree.pid, &tree.born());
    let text = tree.answer();
    let shell = shell(&tree);
    assert!(shell != tree.pid, "{text}");
    assert!(text.contains(&format!("PARENT {shell}")), "{text}");
    assert!(text.contains("PROBE session:launch_id:"), "{text}");
}

/// The session name a `hosted-*` mode claims: one that a resident host serves.
fn hosted_name(mode: &str) -> Option<&'static str> {
    match mode {
        "hosted-dsh" | "hosted-dsh-run" => Some("dsh/example/hosted"),
        "hosted-opencode" => Some("opencode/example/hosted"),
        _ => None,
    }
}

const HOST: &str = "0123456789abcdef";
const INSIDE_HOST: &str = "inside an agent host's process tree";
const CLAIM_BELOW_HOST: &str = "session markers that name another session cannot be proven here";

fn host_reads(text: &str) -> usize {
    text.lines()
        .find_map(|line| line.strip_prefix("HOST_READS "))
        .and_then(|reads| reads.trim().parse().ok())
        .unwrap_or_else(|| panic!("{text}"))
}

/// A recorded DeepSeek host directly above a person's command: refused as
/// inside the host's process tree, on every platform.
#[test]
fn an_unmarked_command_below_a_recorded_host_is_refused() {
    let tree = Tree::start("person", None);
    tree.host(HOST, tree.pid, &tree.birth);
    let text = tree.answer();
    assert!(text.contains(&format!("PARENT {}", tree.pid)), "{text}");
    assert!(text.contains("PROBE refused:not_allowed"), "{text}");
    assert!(text.contains(INSIDE_HOST), "{text}");
}

/// A recorded host with an unmarked shell below it. A withheld shell
/// environment below the top refuses before the recorded-host check.
#[test]
fn an_unmarked_shell_below_a_recorded_host_is_refused() {
    for mode in ["withheld-person", "readable-person"] {
        let tree = Tree::start(mode, None);
        tree.host(HOST, tree.pid, &tree.birth);
        let text = tree.answer();
        assert!(text.contains(&format!("PARENT {}", shell(&tree))), "{text}");
        assert!(text.contains("PROBE refused:not_allowed"), "{text}");
        if !environment_readable(&text, shell(&tree)) {
            assert!(text.contains("hides its environment"), "{text}");
        } else {
            assert!(text.contains(INSIDE_HOST), "{text}");
        }
    }
}

/// The shape observed with a real DeepSeek host: the host's own environment
/// holds the markers of another, live session that started it. A command
/// that keeps them is a marked claim below a recorded host, refused by that
/// rule before any binding is read (zerus-63rru.28; before it, the descent
/// from that session's process failed); one that drops them is refused as
/// inside the host's tree.
#[test]
fn a_host_holding_another_sessions_markers_refuses_both_shells() {
    let other = Job(Command::new("sleep").arg("30").spawn().unwrap());
    let start = crate::platform::process_start_time(other.0.id()).unwrap();
    let kept = Tree::start("member-shell", None);
    kept.record(other.0.id(), &start);
    kept.host(HOST, kept.pid, &kept.birth);
    let text = kept.answer();
    assert!(text.contains(&format!("PARENT {}", shell(&kept))), "{text}");
    refused(&text);
    assert!(text.contains(CLAIM_BELOW_HOST), "{text}");
    assert!(!text.contains("verify the claimed agent"), "{text}");
    for mode in ["normal", "withheld-middle", "readable-middle"] {
        let dropped = Tree::start(mode, None);
        dropped.record(other.0.id(), &start);
        dropped.host(HOST, dropped.pid, &dropped.birth);
        if mode.ends_with("-middle") {
            std::fs::write(dropped.temp.path().join("go"), b"go").unwrap();
            shell(&dropped);
            std::fs::write(dropped.temp.path().join("recorded"), b"").unwrap();
        }
        let text = dropped.answer();
        assert!(text.contains("PROBE refused:not_allowed"), "{mode}: {text}");
        if mode.ends_with("-middle") && !environment_readable(&text, shell(&dropped)) {
            assert!(text.contains("hides its environment"), "{mode}: {text}");
        } else {
            assert!(text.contains(INSIDE_HOST), "{mode}: {text}");
        }
    }
}

/// A recorded host that is not above the command leaves the person.
#[test]
fn a_person_outside_a_recorded_host_is_the_person() {
    let host = Job(Command::new("sleep").arg("30").spawn().unwrap());
    let birth = crate::platform::process_identity(host.0.id())
        .unwrap()
        .birth;
    let tree = Tree::start("person", None);
    tree.host(HOST, host.0.id(), &birth);
    let text = tree.answer();
    person(&text);
    assert_eq!(
        host_reads(&text),
        2,
        "the identity record and host.pid: {text}"
    );
}

/// The record's birth is the walk's own `process_identity` value, so the
/// recorded host itself always matches. A different birth means that host has
/// exited and its process ID was reused: this process is not the host, and
/// the walk goes on as for any other ancestor.
#[test]
fn a_host_record_whose_birth_differs_is_not_that_host() {
    let tree = Tree::start("person", None);
    tree.host(HOST, tree.pid, &later_birth(&tree.birth));
    person(&tree.answer());
}

/// A recorded process ID without a birth (an older `host.pid` alone) cannot
/// be told apart from the host; neither can a corrupt record.
#[test]
fn a_host_id_without_a_birth_or_a_corrupt_record_refuses() {
    for corrupt in [false, true] {
        let tree = Tree::start("person", None);
        let dir = tree.state.join("dsh/hosts").join(HOST);
        std::fs::create_dir_all(&dir).unwrap();
        if corrupt {
            std::fs::write(dir.join("host.identity.json"), b"not json").unwrap();
        } else {
            std::fs::write(dir.join("host.pid"), tree.pid.to_string()).unwrap();
        }
        let text = tree.answer();
        assert!(text.contains("PROBE refused:invalid_state"), "{text}");
        assert!(
            text.contains("DeepSeek agent host record"),
            "{text}"
        );
    }
}

/// At most 64 host folders are read, two small files each (the identity
/// record and `host.pid`); stray files beside them are not counted. A 65th
/// folder refuses before any file is read, with a sentence that names it.
#[test]
fn recorded_host_folders_are_read_within_their_bound() {
    for (folders, refused) in [(64, false), (65, true)] {
        let tree = Tree::start("person", None);
        for n in 0..folders {
            tree.host(&format!("{n:016x}"), 1 << 30, "1");
        }
        for stray in [".DS_Store", "notes.txt", "host.pid"] {
            std::fs::write(tree.state.join("dsh/hosts").join(stray), b"x").unwrap();
        }
        let text = tree.answer();
        if refused {
            assert!(text.contains("PROBE refused:invalid_state"), "{text}");
            assert!(
                text.contains("more DeepSeek agent host folders than the check reads"),
                "{text}"
            );
            assert_eq!(host_reads(&text), 0, "{text}");
        } else {
            person(&text);
            assert_eq!(host_reads(&text), 128, "{text}");
        }
        println!("HOST_FOLDERS {folders} HOST_READS {}", host_reads(&text));
    }
}

/// A state folder, or its `dsh` or `hosts` folder, that does not exist records
/// no host. One that exists but cannot be read as a folder refuses.
#[test]
fn a_missing_state_or_host_folder_records_no_host() {
    let tree = Tree::start("person", None);
    std::fs::remove_dir_all(&tree.state).unwrap();
    person(&tree.answer());
    let tree = Tree::start("person", None);
    std::fs::create_dir(tree.state.join("dsh")).unwrap();
    person(&tree.answer());
    let tree = Tree::start("person", None);
    std::fs::create_dir(tree.state.join("dsh")).unwrap();
    std::fs::write(tree.state.join("dsh/hosts"), b"not a folder").unwrap();
    let text = tree.answer();
    assert!(text.contains("PROBE refused:invalid_state"), "{text}");
    assert!(
        text.contains("DeepSeek agent host record"),
        "{text}"
    );
}

/// Only directories are host folders: a stray `.DS_Store` beside them is not
/// one, alone or next to a recorded host above the command.
#[test]
fn a_stray_file_in_the_host_records_is_not_a_host_folder() {
    let tree = Tree::start("person", None);
    std::fs::create_dir_all(tree.state.join("dsh/hosts")).unwrap();
    std::fs::write(tree.state.join("dsh/hosts/.DS_Store"), b"\0").unwrap();
    person(&tree.answer());
    let tree = Tree::start("person", None);
    tree.host(HOST, tree.pid, &tree.birth);
    std::fs::write(tree.state.join("dsh/hosts/.DS_Store"), b"\0").unwrap();
    let text = tree.answer();
    assert!(text.contains(INSIDE_HOST), "{text}");
}

/// A link in place of a host folder may lead to a live host: it refuses like
/// an unreadable record, as a link in place of `dsh` or `hosts` does.
#[test]
fn a_link_in_place_of_a_host_folder_refuses() {
    let tree = Tree::start("person", None);
    tree.host(HOST, tree.pid, &tree.birth);
    let hosts = tree.state.join("dsh/hosts");
    let moved = tree.temp.path().join("moved-host");
    std::fs::rename(hosts.join(HOST), &moved).unwrap();
    std::os::unix::fs::symlink(&moved, hosts.join(HOST)).unwrap();
    let text = tree.answer();
    assert!(text.contains("PROBE refused:invalid_state"), "{text}");
    assert!(
        text.contains("DeepSeek agent host record"),
        "{text}"
    );
}

/// The listing stops at 1024 entries of any kind, before it looks at the
/// next one, and says that stray entries are the cause: with no host folder,
/// and with a live host's folder beside 1024 files.
#[test]
fn stray_entries_past_the_listing_bound_refuse_and_say_so() {
    for with_host in [false, true] {
        let tree = Tree::start("person", None);
        let hosts = tree.state.join("dsh/hosts");
        std::fs::create_dir_all(&hosts).unwrap();
        let files = if with_host { 1024 } else { 1025 };
        for n in 0..files {
            std::fs::write(hosts.join(format!("stray-{n}")), b"").unwrap();
        }
        if with_host {
            tree.host(HOST, tree.pid, &tree.birth);
        }
        let text = tree.answer();
        assert!(text.contains("PROBE refused:invalid_state"), "{text}");
        assert!(text.contains("more than 1024 entries"), "{text}");
        assert!(
            !text.contains("host folders than the check reads"),
            "{text}"
        );
    }
}

/// A state folder that exists but cannot be searched is not a missing one:
/// it refuses. (Skipped as root, which searches any folder.)
#[test]
fn a_state_folder_that_cannot_be_searched_refuses() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root searches every folder");
        return;
    }
    struct Searchable(PathBuf);
    impl Drop for Searchable {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
        }
    }
    let tree = Tree::start("person", None);
    let locked = tree.temp.path().join("locked");
    std::fs::create_dir_all(locked.join("state")).unwrap();
    std::fs::remove_dir_all(&tree.state).unwrap();
    std::os::unix::fs::symlink(locked.join("state"), &tree.state).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let _restore = Searchable(locked);
    let text = tree.answer();
    assert!(text.contains("PROBE refused:invalid_state"), "{text}");
    assert!(
        text.contains("DeepSeek agent host record"),
        "{text}"
    );
}

/// A starting record names the previous host with its birth (refused inside
/// it) and a PID file without one (refused as unverifiable).
#[test]
fn a_starting_record_names_its_previous_host_and_its_pid_file() {
    let boot = crate::state::session_graph::written_boot();
    let tree = Tree::start("person", None);
    tree.identity(HOST, &json!({"state":"starting","version":2,
        "attempt":"11111111-1111-4111-8111-111111111111","boot":boot,
        "launcher":{"pid":(1u32 << 30),"birth":"1"},"phase":"spawning","pid_file":null,
        "previous":{"version":2,"pid":tree.pid,"birth":tree.birth,"pid_file":[1,2,3,4],"boot":boot}}));
    let text = tree.answer();
    assert!(text.contains(INSIDE_HOST), "{text}");
    let tree = Tree::start("person", None);
    tree.identity(
        HOST,
        &json!({"state":"starting","version":2,
        "attempt":"11111111-1111-4111-8111-111111111111","boot":boot,
        "launcher":{"pid":(1u32 << 30),"birth":"1"},"phase":"spawning",
        "pid_file":[tree.pid,[1,2,3,4]],"previous":null}),
    );
    let text = tree.answer();
    assert!(text.contains("PROBE refused:invalid_state"), "{text}");
    assert!(
        text.contains("DeepSeek agent host record"),
        "{text}"
    );
}

/// A starting record's launcher is the `hgs` that started the attempt, not a
/// host: a command below the launcher is not refused for it.
#[test]
fn a_starting_records_launcher_is_not_a_host() {
    let tree = Tree::start("person", None);
    tree.identity(
        HOST,
        &json!({"state":"starting","version":2,
        "attempt":"11111111-1111-4111-8111-111111111111",
        "boot":crate::state::session_graph::written_boot(),
        "launcher":{"pid":tree.pid,"birth":tree.birth},"phase":"spawning",
        "pid_file":null,"previous":null}),
    );
    person(&tree.answer());
}

/// The stated limit: a host spawned by an attempt that ended unconfirmed,
/// before its running record, is named by no record (its starting record
/// names only the launcher), so a command below it passes as a person.
#[test]
fn a_host_from_an_unconfirmed_start_is_named_nowhere_the_stated_limit() {
    let tree = Tree::start("person", None);
    tree.identity(
        HOST,
        &json!({"state":"starting","version":2,
        "attempt":"11111111-1111-4111-8111-111111111111",
        "boot":crate::state::session_graph::written_boot(),
        "launcher":{"pid":(1u32 << 30),"birth":"1"},"phase":"spawning",
        "pid_file":null,"previous":null}),
    );
    person(&tree.answer());
}

/// A record that carries another boot's identity names no process of this
/// boot, running or starting; one without a boot identity still matches.
#[test]
fn a_host_record_from_another_boot_is_not_this_boots_host() {
    let other = "22222222-2222-4222-8222-222222222222";
    let tree = Tree::start("person", None);
    tree.identity(
        HOST,
        &json!({"state":"running","version":2,"pid":tree.pid,
        "birth":tree.birth,"pid_file":[1,2,3,4],"boot":other}),
    );
    std::fs::write(
        tree.state.join("dsh/hosts").join(HOST).join("host.pid"),
        tree.pid.to_string(),
    )
    .unwrap();
    person(&tree.answer());
    let tree = Tree::start("person", None);
    tree.identity(HOST, &json!({"state":"starting","version":2,
        "attempt":"11111111-1111-4111-8111-111111111111","boot":other,
        "launcher":{"pid":(1u32 << 30),"birth":"1"},"phase":"spawning","pid_file":null,
        "previous":{"version":2,"pid":tree.pid,"birth":tree.birth,"pid_file":[1,2,3,4],"boot":other}}));
    person(&tree.answer());
    let tree = Tree::start("person", None);
    tree.identity(
        HOST,
        &json!({"state":"running","version":1,"pid":tree.pid,
        "birth":tree.birth,"pid_file":[1,2,3,4]}),
    );
    assert!(tree.answer().contains(INSIDE_HOST));
}

/// A recorded host at the top of the user's tree, as on macOS where launchd
/// adopts it. Here the top is whatever the fixture's chain ends at: the
/// user's service manager on Linux, the adopted helper on macOS. Its record
/// lives only in this test's private state.
#[test]
fn a_recorded_host_at_the_top_of_the_users_tree_is_refused() {
    let tree = Tree::start("person", None);
    let mut top = tree.pid;
    while let Some(parent) = crate::platform::process_identity(top)
        .map(|identity| identity.parent)
        .filter(|parent| crate::platform::process_identity(*parent).is_some())
    {
        top = parent;
    }
    let birth = crate::platform::process_identity(top).unwrap().birth;
    tree.host(HOST, top, &birth);
    let text = tree.answer();
    assert!(text.contains(&format!("ANCESTOR pid={top} ")), "{text}");
    assert!(text.contains("top=true"), "{text}");
    assert!(text.contains(INSIDE_HOST), "{text}");
    println!("HOST_AT_TOP pid={top} {text}");
}

/// OpenCode's own service file registers its process with no birth. An
/// ancestor it registers that is not younger than the file is refused as
/// inside the host's tree; a younger one, a malformed file or none is not.
#[test]
fn an_ancestor_registered_as_the_opencode_service_is_refused() {
    let tree = Tree::start("person", None);
    tree.opencode_service(tree.pid);
    let text = tree.answer();
    assert!(text.contains("PROBE refused:not_allowed"), "{text}");
    assert!(text.contains(INSIDE_HOST), "{text}");
    let younger = Tree::start("person", None);
    let file = younger.opencode_service(younger.pid);
    let hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(hour_ago)
        .unwrap();
    person(&younger.answer());
    let malformed = Tree::start("person", None);
    let file = malformed.opencode_service(malformed.pid);
    std::fs::write(file, b"{\"pid\":").unwrap();
    person(&malformed.answer());
}

/// A marked caller whose binding says OpenCode's shared service serves its
/// session cannot be a lead or a member: refused with the sentence that says
/// so. A marked DeepSeek claim takes the hosted proof only (zerus-63rru.28):
/// this tree carries no per-call token, so it is refused in that proof's
/// words, whatever its binding says, and the ancestry proof reads nothing.
#[test]
fn a_marked_session_served_by_a_resident_host_is_refused_plainly() {
    for (mode, backend, conversation) in [
        ("hosted-dsh", "dsh", "session-0001"),
        ("hosted-opencode", "opencode", "ses_0001"),
        ("hosted-dsh-run", "dsh", "session-0001"),
    ] {
        let tree = Tree::start(mode, None);
        let name = hosted_name(mode).unwrap();
        let run = if mode == "hosted-dsh-run" {
            "another-run"
        } else {
            RUN
        };
        let dir = tree.state.join(backend).join("bindings");
        std::fs::create_dir_all(&dir).unwrap();
        let binding = json!({"backend":backend,"name":name,"agent":backend,"run_id":run,
            "conversation_id":conversation,"created":1.0,"paused":false});
        std::fs::write(
            dir.join(format!("{:x}.json", Sha256::digest(name.as_bytes()))),
            binding.to_string(),
        )
        .unwrap();
        let text = tree.answer();
        refused(&text);
        let plain = text.contains("served by OpenCode's shared service");
        assert_eq!(plain, backend == "opencode", "{mode}: {text}");
        let hosted = text.contains("This command carries no DeepSeek per-call token");
        assert_eq!(hosted, backend == "dsh", "{mode}: {text}");
        if backend == "dsh" {
            assert!(text.lines().any(|line| line == "READS 0"), "{mode}: {text}");
        }
    }
}

struct Job(Child);
impl Drop for Job {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Mutation A: delete native ancestry, retaining live PID and birth checks.
#[test]
fn claiming_a_live_matching_sibling_is_refused() {
    let tree = Tree::start("marked", None);
    let job = Job(Command::new("sleep").arg("30").spawn().unwrap());
    tree.record(
        job.0.id(),
        &crate::platform::process_start_time(job.0.id()).unwrap(),
    );
    refused(&tree.answer());
}

/// Mutation C: use the dispatcher's own markers instead of the caller proof.
#[test]
fn real_dispatch_refuses_preview_on_and_off_below_a_live_agent() {
    for action in ["on", "off"] {
        let tree = Tree::start("normal", Some(action));
        tree.record(tree.pid, &tree.born());
        if action == "off" {
            std::fs::create_dir(tree.state.join("teams")).unwrap();
            std::fs::write(tree.state.join("teams/preview"), b"on").unwrap();
        }
        let text = tree.answer();
        assert!(text.contains("PREVIEW 1"), "{text}");
        assert_eq!(crate::teams::enabled(&tree.state), action == "off");
    }
}

#[test]
fn an_unrelated_fifo_never_blocks_or_leaks_a_path() {
    let tree = Tree::start("normal", None);
    let fifo = tree.state.join(format!("{}.json", "a".repeat(64)));
    let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let started = Instant::now();
    let text = tree.answer();
    assert!(started.elapsed() < Duration::from_secs(3), "{text}");
    refused(&text);
    assert!(!text.contains(tree.temp.path().to_str().unwrap()), "{text}");
}

#[test]
fn a_claimed_fifo_or_link_refuses_without_waiting_or_reading_the_target() {
    use std::os::unix::fs::symlink;
    for link in [false, true] {
        let tree = Tree::start("marked", None);
        let path = tree.record(tree.pid, &tree.born());
        std::fs::remove_file(&path).unwrap();
        if link {
            let target = tree.state.join("target");
            tree.record(tree.pid, &tree.born());
            std::fs::rename(&path, &target).unwrap();
            symlink(target, &path).unwrap();
        } else {
            let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        }
        let started = Instant::now();
        let text = tree.answer();
        refused(&text);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(!text.contains(tree.temp.path().to_str().unwrap()), "{text}");
    }
}

#[test]
fn a_panicking_explicit_read_leaves_every_environment_variable_unchanged() {
    let tree = Tree::start("panic", None);
    assert!(tree.answer().contains("PANIC_ENV_UNCHANGED"));
}

/// Mutation B: remove the real off dispatcher guard (the old helper's copy
/// of this check is deliberately not called by this test).
#[test]
fn real_dispatch_off_precedes_proof_and_store_io() {
    let tree = Tree::start("off", None);
    let path = tree.record(tree.pid, &tree.born());
    std::fs::write(path, b"not json").unwrap();
    let text = tree.answer();
    assert!(text.contains("teams_inactive"), "{text}");
    assert!(text.contains("READS 0"), "{text}");
    assert!(!tree.state.join("teams").exists());
}

#[test]
fn real_dispatch_preview_status_does_not_prove_the_caller() {
    let tree = Tree::start("preview-status", None);
    let path = tree.record(tree.pid, &tree.born());
    std::fs::write(path, b"not json").unwrap();
    let text = tree.answer();
    assert!(text.contains("DISPATCH 0"), "{text}");
    assert!(text.contains("READS 0"), "{text}");
    assert!(!tree.state.join("teams").exists());
}

#[test]
fn five_thousand_saved_sessions_do_not_change_the_number_of_binding_files_opened() {
    let mut counts = Vec::new();
    for saved in [0, 5000] {
        let tree = Tree::start("normal", None);
        tree.record(tree.pid, "stale-birth");
        for i in 0..saved {
            let path = tree.state.join(format!(
                "{:x}.json",
                Sha256::digest(format!("session-{i}").as_bytes())
            ));
            std::fs::write(path, b"{}").unwrap();
        }
        let text = tree.answer();
        person(&text);
        let reads = text
            .lines()
            .find_map(|s| s.strip_prefix("READS "))
            .unwrap()
            .parse::<usize>()
            .unwrap();
        counts.push(reads);
    }
    assert_eq!(counts, vec![1, 1]);
    println!(
        "STATE_BINDING_READS empty={} with_5000={}",
        counts[0], counts[1]
    );
}

#[test]
fn a_marked_call_never_waits_on_a_machine_catalog_fifo() {
    let tree = Tree::start("marked", None);
    tree.record(tree.pid, &tree.born());
    let path = tree.temp.path().join("config/swarm/catalog.json");
    std::fs::remove_file(&path).unwrap();
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let before = Instant::now();
    refused(&tree.answer());
    assert!(before.elapsed() < Duration::from_secs(3));
}

#[test]
fn one_rename_hop_is_verified_and_a_second_or_changed_identity_refuses() {
    for mode in ["one", "two", "changed", "missing"] {
        let tree = Tree::start("marked", None);
        let path = tree.record(tree.pid, &tree.born());
        let mut current: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut shadow = current.clone();
        let target = "claude/example/renamed";
        shadow["rename_shadow"] = json!(true);
        shadow["rename_target"] = json!(target);
        std::fs::write(path, shadow.to_string()).unwrap();
        current["name"] = json!(target);
        if mode == "two" {
            current["rename_shadow"] = json!(true);
            current["rename_target"] = json!(NAME);
        }
        if mode == "changed" {
            current["session_id"] = json!("00000000-0000-4000-8000-0000000000ff");
        }
        if mode != "missing" {
            let path = tree
                .state
                .join(format!("{:x}.json", Sha256::digest(target.as_bytes())));
            std::fs::write(path, current.to_string()).unwrap();
        }
        let text = tree.answer();
        if mode == "one" {
            assert!(text.contains("PROBE session:launch_id:"), "{text}");
        } else {
            refused(&text);
        }
    }
}

#[test]
fn an_oversized_record_or_linked_routing_directory_refuses() {
    for linked in [false, true] {
        let tree = Tree::start("marked", None);
        let path = tree.record(tree.pid, &tree.born());
        if linked {
            let target = tree.state.join("route-target");
            std::fs::create_dir(&target).unwrap();
            std::os::unix::fs::symlink(target, tree.state.join("name_routes")).unwrap();
        } else {
            let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            record["padding"] = json!("x".repeat(1 << 20));
            std::fs::write(path, record.to_string()).unwrap();
        }
        refused(&tree.answer());
    }
}

/// Both caller paths exercise every route result: live, established not-live,
/// established absence, and incomplete/contradictory/unreadable routing.
#[test]
fn explicit_id_routes_distinguish_live_stale_absent_and_unverifiable_on_both_paths() {
    for marked in [false, true] {
        for case in [
            "live",
            "stale",
            "archived",
            "other-process",
            "absent",
            "name",
            "run",
            "layout",
            "corrupt",
            "badhint",
        ] {
            let mode = match (marked, case == "badhint") {
                (true, false) => "id-marked",
                (false, false) => "id-unmarked",
                (true, true) => "id-marked-badhint",
                (false, true) => "id-unmarked-badhint",
            };
            let tree = Tree::start(mode, None);
            let original = tree.record(tree.pid, &tree.born());
            let mut record: Value =
                serde_json::from_slice(&std::fs::read(&original).unwrap()).unwrap();
            std::fs::remove_file(original).unwrap();
            record["layout"] = json!("id");
            let job = if case == "other-process" {
                Some(Job(Command::new("sleep").arg("30").spawn().unwrap()))
            } else {
                None
            };
            if let Some(job) = &job {
                record["pid"] = json!(job.0.id());
                record["process_start"] =
                    json!(crate::platform::process_start_time(job.0.id()).unwrap());
            }
            match case {
                "stale" => record["process_start"] = json!("stale"),
                "archived" => record["state"] = json!("archived"),
                "name" => record["name"] = json!("claude/example/unrelated"),
                "run" => record["run_id"] = json!("other-run"),
                "layout" => record["layout"] = json!("legacy"),
                _ => (),
            }
            std::fs::create_dir(tree.state.join("sessions")).unwrap();
            if case != "absent" {
                let content = if case == "corrupt" {
                    "not json".into()
                } else {
                    record.to_string()
                };
                std::fs::write(
                    tree.state.join("sessions").join(format!("{ID}.json")),
                    content,
                )
                .unwrap();
            }
            let text = tree.answer();
            let established_not_live = ["stale", "archived", "other-process"].contains(&case);
            if case == "live" && marked {
                assert!(text.contains("PROBE session:launch_id:"), "{case}: {text}");
            } else if !marked && established_not_live {
                person(&text);
            } else if !marked && case == "live" {
                assert!(text.contains("PROBE refused:not_allowed"), "{case}: {text}");
            } else if marked && established_not_live {
                assert!(
                    text.contains("PROBE refused:not_allowed")
                        || text.contains("PROBE refused:invalid_state"),
                    "{case}: {text}"
                );
            } else {
                assert!(text.contains("PROBE refused:"), "{case}: {text}");
            }
            println!(
                "ID_ROUTE marked={marked} case={case} {}",
                text.lines().find(|s| s.starts_with("PROBE ")).unwrap()
            );
        }
    }
}

#[test]
fn renamed_id_layout_needs_the_hint_and_its_original_launch_label() {
    for marked in [false, true] {
        for hint in [false, true] {
            let mode = match (marked, hint) {
                (true, true) => "id-marked",
                (false, true) => "id-unmarked",
                (true, false) => "id-marked-nohint",
                (false, false) => "id-unmarked-nohint",
            };
            let tree = Tree::start(mode, None);
            let path = tree.record(tree.pid, &tree.born());
            let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            std::fs::remove_file(path).unwrap();
            record["layout"] = json!("id");
            record["launch_label"] = json!(NAME);
            record["name"] = json!("claude/example/renamed");
            std::fs::create_dir(tree.state.join("sessions")).unwrap();
            std::fs::write(
                tree.state.join("sessions").join(format!("{ID}.json")),
                record.to_string(),
            )
            .unwrap();
            let text = tree.answer();
            if marked && hint {
                assert!(text.contains("PROBE session:launch_id:"), "{text}");
            } else if hint {
                assert!(text.contains("PROBE refused:not_allowed"), "{text}");
            } else {
                assert!(text.contains("PROBE refused:invalid_state"), "{text}");
            }
            println!(
                "RENAMED_ID marked={marked} hint={hint} {}",
                text.lines().find(|s| s.starts_with("PROBE ")).unwrap()
            );
        }
    }
}

fn route_outcomes(route: &str) {
    for marked in [false, true] {
        for case in [
            "live",
            "stale",
            "archived",
            "other-process",
            "absent",
            "corrupt",
            "run",
        ] {
            let tree = Tree::start(if marked { "id-marked" } else { "id-unmarked" }, None);
            let path = tree.record(tree.pid, &tree.born());
            let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            std::fs::remove_file(&path).unwrap();
            let job = if case == "other-process" {
                Some(Job(Command::new("sleep").arg("30").spawn().unwrap()))
            } else {
                None
            };
            if let Some(job) = &job {
                record["pid"] = json!(job.0.id());
                record["process_start"] =
                    json!(crate::platform::process_start_time(job.0.id()).unwrap());
            }
            if case == "stale" {
                record["process_start"] = json!("stale");
            }
            if case == "archived" {
                record["state"] = json!("archived");
            }
            if case == "run" {
                record["run_id"] = json!("another-run");
            }
            let id_layout = route == "index-id";
            let destination = if id_layout {
                std::fs::create_dir(tree.state.join("sessions")).unwrap();
                record["layout"] = json!("id");
                tree.state.join("sessions").join(format!("{ID}.json"))
            } else {
                path
            };
            if case != "absent" {
                std::fs::write(
                    destination,
                    if case == "corrupt" {
                        "not json".into()
                    } else {
                        record.to_string()
                    },
                )
                .unwrap();
            }
            if route.starts_with("index-") {
                std::fs::create_dir(tree.state.join("ids")).unwrap();
                std::fs::write(
                    tree.state.join("ids").join(format!("{ID}.json")),
                    json!({"version":1,"kind":if id_layout {"id"} else {"record"},"name":NAME})
                        .to_string(),
                )
                .unwrap();
            }
            let text = tree.answer();
            if case == "live" && marked {
                assert!(
                    text.contains("PROBE session:launch_id:"),
                    "{route} {case}: {text}"
                );
            } else if case == "live" {
                assert!(
                    text.contains("PROBE refused:not_allowed"),
                    "{route} {case}: {text}"
                );
            } else if ["stale", "archived", "other-process"].contains(&case) && !marked {
                person(&text);
            } else {
                refused(&text);
            }
            println!(
                "ROUTE {route} marked={marked} case={case} {}",
                text.lines().find(|s| s.starts_with("PROBE ")).unwrap()
            );
        }
    }
}

#[test]
fn the_existing_index_selects_one_verified_id_record() {
    route_outcomes("index-id");
}
#[test]
fn the_existing_index_selects_one_verified_legacy_record_with_an_id_hint() {
    route_outcomes("index-legacy");
}
#[test]
fn two_bounded_probes_reach_a_legacy_record_that_carries_an_id_hint() {
    route_outcomes("probes-legacy");
}

#[test]
fn a_present_index_never_falls_back_and_disagreeing_probes_refuse() {
    for marked in [false, true] {
        for case in ["kind", "name", "corrupt", "missing-selected", "disagree"] {
            let tree = Tree::start(if marked { "id-marked" } else { "id-unmarked" }, None);
            let legacy = tree.record(tree.pid, &tree.born());
            let mut record: Value =
                serde_json::from_slice(&std::fs::read(legacy).unwrap()).unwrap();
            if case == "disagree" {
                std::fs::create_dir(tree.state.join("sessions")).unwrap();
                record["layout"] = json!("id");
                std::fs::write(
                    tree.state.join("sessions").join(format!("{ID}.json")),
                    record.to_string(),
                )
                .unwrap();
            } else {
                std::fs::create_dir(tree.state.join("ids")).unwrap();
                let entry = json!({"version":1,"kind":if case=="kind" {"unknown"} else if case=="missing-selected" {"id"} else {"record"},
                    "name":if case=="name" {"claude/example/wrong"} else {NAME}});
                std::fs::write(
                    tree.state.join("ids").join(format!("{ID}.json")),
                    if case == "corrupt" {
                        "not json".into()
                    } else {
                        entry.to_string()
                    },
                )
                .unwrap();
            }
            let text = tree.answer();
            assert!(
                text.contains("PROBE refused:invalid_state"),
                "{case}: {text}"
            );
        }
    }
}

#[test]
fn a_marked_session_accepts_a_symlinked_config_root_but_not_links_below_it() {
    for mode in ["id-marked-symlink", "id-marked-home-symlink"] {
        let tree = Tree::start(mode, None);
        tree.record(tree.pid, &tree.born());
        let text = tree.answer();
        assert!(text.contains("PROBE session:launch_id:"), "{text}");
    }
    let tree = Tree::start("id-marked", None);
    tree.record(tree.pid, &tree.born());
    let config = tree.temp.path().join("config");
    std::fs::rename(config.join("swarm"), config.join("actual-swarm")).unwrap();
    std::os::unix::fs::symlink(config.join("actual-swarm"), config.join("swarm")).unwrap();
    let text = tree.answer();
    refused(&text);
    assert!(
        !text.contains("ordinary terminal"),
        "marked refusal must explain session proof: {text}"
    );
}

#[test]
fn a_legacy_route_refuses_an_id_layout_record_on_both_caller_paths() {
    for mode in ["marked", "normal"] {
        let tree = Tree::start(mode, None);
        let path = tree.record(tree.pid, &tree.born());
        let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        record["layout"] = json!("id");
        std::fs::write(path, record.to_string()).unwrap();
        let text = tree.answer();
        assert!(text.contains("PROBE refused:invalid_state"), "{text}");
    }
}

/// Whether this machine can run a probe inside a private user namespace.
#[cfg(target_os = "linux")]
fn user_namespace_available(pid_namespace: bool) -> bool {
    let mut command = Command::new("unshare");
    command.args(["--user", "--map-root-user"]);
    if pid_namespace {
        command.args(["--pid", "--fork", "--mount-proc"]);
    }
    command
        .arg("true")
        .status()
        .is_ok_and(|status| status.success())
}

/// Invoked inside the real namespace by the parent test below. Outside that
/// fixture it is inert, so the ordinary host suite retains its positive cases.
#[cfg(target_os = "linux")]
#[test]
fn namespace_refusal_probe() {
    if std::env::var_os("HGS_EXPECT_NAMESPACE_REFUSAL").is_none() {
        return;
    }
    let config = crate::config::Config::load().unwrap();
    let result = caller(&config);
    assert!(
        matches!(result,Caller::Refused(ref problem) if problem.message.contains("initial Linux user namespace")),
        "{result:?}"
    );
    for action in ["on", "off"] {
        assert_eq!(
            crate::teams::dispatch(&config, &["preview".into(), action.into()], false).unwrap(),
            1
        );
        assert!(!crate::teams::enabled(&config.state));
    }
    assert_eq!(
        crate::teams::dispatch(&config, &["preview".into(), "status".into()], false).unwrap(),
        0
    );
    assert_eq!(
        crate::teams::dispatch(&config, &["status".into()], false).unwrap(),
        1
    );
    println!("NAMESPACE_REFUSAL_CONFIRMED");
}

#[cfg(target_os = "linux")]
#[test]
fn user_and_pid_namespace_controls_refuse_instead_of_becoming_the_person() {
    for pid_namespace in [false, true] {
        if !user_namespace_available(pid_namespace) {
            eprintln!("skipped: user namespaces are unavailable");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let config = temp.path().join("config");
        std::fs::create_dir(&state).unwrap();
        std::fs::create_dir(&config).unwrap();
        catalog(&config);
        let mut command = Command::new("unshare");
        command.args(["--user", "--map-current-user"]);
        if pid_namespace {
            command.args(["--pid", "--fork", "--mount-proc"]);
        }
        let output = command
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "teams::caller::tests::regressions::namespace_refusal_probe",
                "--nocapture",
            ])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", temp.path())
            .env("HGS_STATE_DIR", &state)
            .env("HGS_CONFIG_DIR", &config)
            .env("HGS_EXPECT_NAMESPACE_REFUSAL", "1")
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.status.success(),
            "namespace admission or control failed: {text}"
        );
        assert!(text.contains("NAMESPACE_REFUSAL_CONFIRMED"), "{text}");
        println!("NAMESPACE pid_namespace={pid_namespace} {text}");
    }
}

/// Runs the ordinary probe with session markers below `unshare`; `None` when
/// this machine cannot create the namespace.
#[cfg(target_os = "linux")]
fn marked_probe_in_namespace(pid_namespace: bool) -> Option<String> {
    if !user_namespace_available(pid_namespace) {
        return None;
    }
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let config = temp.path().join("config");
    std::fs::create_dir(&state).unwrap();
    std::fs::create_dir(&config).unwrap();
    catalog(&config);
    let mut command = Command::new("unshare");
    command.args(["--user", "--map-current-user"]);
    if pid_namespace {
        command.args(["--pid", "--fork", "--mount-proc"]);
    }
    let output = command
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "teams::caller::tests::probe", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("HGS_STATE_DIR", &state)
        .env("HGS_CONFIG_DIR", &config)
        .env("HGS_TEAMS_CALLER_PROBE", "1")
        .env("HGS_SESSION", NAME)
        .env("HGS_RUN_ID", RUN)
        .env("HGS_SESSION_ID", ID)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    Some(text)
}

#[cfg(target_os = "linux")]
#[test]
fn a_marked_command_in_a_user_only_namespace_is_refused() {
    let Some(text) = marked_probe_in_namespace(false) else {
        eprintln!("skipped: user namespaces are unavailable");
        return;
    };
    refused(&text);
    assert!(
        text.contains("sandbox with its own user namespace"),
        "{text}"
    );
    assert!(text.contains("must run outside it"), "{text}");
    assert!(!text.contains("initial Linux user namespace"), "{text}");
    println!("MARKED_USER_NAMESPACE {text}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_marked_command_in_a_user_and_pid_namespace_is_refused() {
    let Some(text) = marked_probe_in_namespace(true) else {
        eprintln!("skipped: user and pid namespaces are unavailable");
        return;
    };
    refused(&text);
    assert!(
        text.contains("sandbox with its own user namespace"),
        "{text}"
    );
    assert!(text.contains("must run outside it"), "{text}");
    assert!(!text.contains("initial Linux user namespace"), "{text}");
    println!("MARKED_USER_PID_NAMESPACE {text}");
}

/// The coordinator's probe: descriptor 9 holds the initial user namespace
/// before the shell enters a private user and mount namespace. The shell then
/// binds that handle over its own `ns` directory and execs the probe, which
/// keeps the same PID, inside its own private mount namespace.
#[cfg(target_os = "linux")]
const FORGERY: &str = "exec 9</proc/self/ns/user\n\
exec unshare --user --map-root-user --mount --propagation private sh -c '\n\
set -e\n\
mkdir -p \"$1/ns\"\n\
: > \"$1/ns/user\"\n\
mount --bind /proc/self/fd/9 \"$1/ns/user\"\n\
mount --rbind \"$1/ns\" \"/proc/$$/ns\"\n\
exec \"$2\" --exact teams::caller::tests::regressions::forged_namespace_probe --nocapture\n\
' fixture \"$1\" \"$2\"";

/// The forged fixture's answer, or `None` when the private namespace or the
/// overmount could not be made (the control then never sees the initial inode).
#[cfg(target_os = "linux")]
fn forged_namespace(marked: bool) -> Option<String> {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let config = temp.path().join("config");
    std::fs::create_dir(&state).unwrap();
    std::fs::create_dir(&config).unwrap();
    catalog(&config);
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(FORGERY)
        .arg("fixture")
        .arg(temp.path())
        .arg(std::env::current_exe().unwrap())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("HGS_STATE_DIR", &state)
        .env("HGS_CONFIG_DIR", &config)
        .env("HGS_TEAMS_FORGED_NS_PROBE", "1");
    if marked {
        command
            .env("HGS_SESSION", NAME)
            .env("HGS_RUN_ID", RUN)
            .env("HGS_SESSION_ID", ID);
    }
    let output = command.output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !text.contains("FORGERY_CONTROL ok") {
        return None;
    }
    assert!(
        output.status.success(),
        "the forged-namespace probe failed: {text}"
    );
    Some(text)
}

/// Shows the old path read answering with the initial namespace first, then
/// the admission and both caller paths inside the forged fixture.
#[cfg(target_os = "linux")]
#[test]
fn forged_namespace_probe() {
    if std::env::var_os("HGS_TEAMS_FORGED_NS_PROBE").is_none() {
        return;
    }
    use std::os::unix::fs::MetadataExt;
    let old = std::fs::metadata("/proc/self/ns/user")
        .map(|meta| meta.ino())
        .unwrap_or(0);
    println!("FORGERY_OLD_PATH {old}");
    if old != context::USER_NS_INIT_INO {
        println!("FORGERY_CONTROL unavailable");
        return;
    }
    println!("FORGERY_CONTROL ok");
    match context::admitted() {
        Ok(()) => println!("FORGERY_ADMISSION admitted"),
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            println!("FORGERY_ADMISSION EXDEV")
        }
        Err(error) => println!(
            "FORGERY_ADMISSION {}",
            error
                .raw_os_error()
                .map_or_else(|| "other".to_owned(), |code| code.to_string())
        ),
    }
    let config = crate::config::Config::load().unwrap();
    match caller(&config) {
        Caller::Person => println!("FORGERY_CALLER person"),
        Caller::Session(_) => println!("FORGERY_CALLER session"),
        Caller::Refused(problem) => {
            println!("FORGERY_CALLER refused:{}", problem.code);
            println!("FORGERY_CALLER_DETAIL {}", problem.message);
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_forged_namespace_path_cannot_admit_a_caller() {
    for marked in [false, true] {
        let Some(text) = forged_namespace(marked) else {
            eprintln!(
                "skipped: user and mount namespaces or the namespace overmount are unavailable"
            );
            return;
        };
        assert!(text.contains("FORGERY_OLD_PATH 4026531837"), "{text}");
        assert!(text.contains("FORGERY_ADMISSION EXDEV"), "{text}");
        assert!(
            text.contains("FORGERY_CALLER refused:not_allowed"),
            "{text}"
        );
        if marked {
            assert!(
                text.contains("sandbox with its own user namespace"),
                "{text}"
            );
            assert!(text.contains("must run outside it"), "{text}");
        } else {
            assert!(text.contains("initial Linux user namespace"), "{text}");
            assert!(
                !text.contains("sandbox with its own user namespace"),
                "{text}"
            );
        }
        println!("FORGED_NAMESPACE marked={marked} {text}");
    }
}

/// The admission without its mount rules would not be distinguishable from
/// the procfs root test elsewhere; keep that root rule pinned to procfs.
#[cfg(target_os = "linux")]
#[test]
fn only_a_procfs_root_is_admitted() {
    const TMPFS_MAGIC: libc::c_long = 0x0102_1994;
    assert!(context::is_procfs_root(context::PROC_SUPER_MAGIC, 1));
    assert!(!context::is_procfs_root(TMPFS_MAGIC, 1));
    assert!(!context::is_procfs_root(context::PROC_SUPER_MAGIC, 2));
    assert!(context::is_procfs(context::PROC_SUPER_MAGIC));
    assert!(!context::is_procfs(TMPFS_MAGIC));
}

/// The ordinary positive case: the real admission passes a plain command in
/// the initial user namespace.
#[cfg(target_os = "linux")]
#[test]
fn the_admission_passes_in_the_plain_test_process() {
    use std::os::unix::fs::MetadataExt;
    let initial = std::fs::metadata("/proc/self/ns/user")
        .is_ok_and(|meta| meta.ino() == context::USER_NS_INIT_INO);
    if !initial {
        eprintln!("skipped: /proc/self/ns/user is not the initial user namespace");
        return;
    }
    assert_eq!(context::admission(), Ok(()));
}

/// The second `f_type` check: a link whose text is exactly the initial
/// namespace, but whose filesystem is not procfs, must not be admitted.
#[cfg(target_os = "linux")]
#[test]
fn a_lookalike_link_on_another_filesystem_is_refused() {
    use std::os::fd::OwnedFd;
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("self/ns")).unwrap();
    symlink(
        format!("user:[{}]", context::USER_NS_INIT_INO),
        temp.path().join("self/ns/user"),
    )
    .unwrap();
    let root: OwnedFd = std::fs::File::open(temp.path()).unwrap().into();
    assert!(context::admit_from(&root).is_err());
}
