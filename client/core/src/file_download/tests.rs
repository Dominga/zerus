use super::*;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "zerus-file-download-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn reply(host: &str, remote: &str, path: &Path, bytes: Value) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"host":host,"path":remote,"local_path":path.to_str().unwrap(),"bytes":bytes}),
    )
    .unwrap()
}

#[test]
fn literal_transfer_args_keep_remote_path_bytes_with_the_qt_budget() {
    let remote = "~/a report's $(literal); [1].md";
    let invocation = invocation("example-peer", remote, Path::new("/tmp/example-copy.md")).unwrap();
    assert_eq!(
        invocation.args,
        vec![
            "@example-peer",
            "download",
            "--path",
            remote,
            "--output",
            "/tmp/example-copy.md"
        ]
    );
    assert_eq!(invocation.timeout, Duration::from_secs(135));
    assert_eq!(invocation.stdin, None);
    assert_eq!(invocation.request_id, None);
    for host in [
        "",
        "-bad",
        "@bad",
        "peer name",
        "peer\nother",
        "user@peer",
        "peer:2",
    ] {
        assert!(super::invocation(host, "/srv/example/report", Path::new("copy")).is_err());
    }
    for remote in ["relative/report", "", "/srv/example/\0file"] {
        assert!(super::invocation("example-peer", remote, Path::new("copy")).is_err());
    }
}

#[test]
fn success_requires_exact_identity_readable_file_and_size_including_empty_files() {
    let scratch = Scratch::new();
    let file = scratch.0.join("report.md");
    for payload in [b"".as_slice(), b"report"] {
        fs::write(&file, payload).unwrap();
        confirm(
            "example-peer",
            "/srv/example/report",
            &file,
            &reply(
                "example-peer",
                "/srv/example/report",
                &file,
                json!(payload.len()),
            ),
        )
        .unwrap();
        confirm(
            "example-peer",
            "/srv/example/report",
            &file,
            &reply(
                "example-peer",
                "/srv/example/report",
                &file,
                json!(payload.len() as f64),
            ),
        )
        .unwrap();
        for bytes in [
            Value::Null,
            json!("6"),
            json!(-1),
            json!(1.5),
            json!(payload.len() + 1),
        ] {
            assert_eq!(
                confirm(
                    "example-peer",
                    "/srv/example/report",
                    &file,
                    &reply("example-peer", "/srv/example/report", &file, bytes)
                )
                .unwrap_err(),
                UNCONFIRMED
            );
        }
    }
    let valid = reply("example-peer", "/srv/example/report", &file, json!(6));
    for field in ["host", "path", "local_path"] {
        let mut changed: Value = serde_json::from_slice(&valid).unwrap();
        changed[field] = json!("different");
        assert_eq!(
            confirm(
                "example-peer",
                "/srv/example/report",
                &file,
                &serde_json::to_vec(&changed).unwrap()
            )
            .unwrap_err(),
            UNCONFIRMED
        );
    }
    fs::remove_file(&file).unwrap();
    let mut missing: Value = serde_json::from_slice(&valid).unwrap();
    missing.as_object_mut().unwrap().remove("bytes");
    assert_eq!(
        confirm(
            "example-peer",
            "/srv/example/report",
            &file,
            &serde_json::to_vec(&missing).unwrap()
        )
        .unwrap_err(),
        UNCONFIRMED,
        "a missing file and missing byte count cannot confirm each other"
    );
    fs::create_dir(&file).unwrap();
    assert_eq!(
        confirm("example-peer", "/srv/example/report", &file, &valid).unwrap_err(),
        UNCONFIRMED
    );
}

#[test]
fn malformed_or_oversized_responses_and_failed_exits_keep_the_qt_failure_detail() {
    let scratch = Scratch::new();
    let file = scratch.0.join("report.md");
    for bytes in [b"invalid".as_slice(), b"[]", b"null", b"{\"host\":"] {
        assert_eq!(
            confirm("example-peer", "/srv/example/report", &file, bytes).unwrap_err(),
            INVALID_RESPONSE
        );
    }
    assert_eq!(
        confirm(
            "example-peer",
            "/srv/example/report",
            &file,
            &vec![b' '; LIMIT + 1]
        )
        .unwrap_err(),
        INVALID_RESPONSE
    );
    let failures = [
        (
            Exit::Code {
                code: 1,
                stdout: vec![],
                stderr: "hgs: The source is unavailable.\n".into(),
            },
            "The source is unavailable.",
        ),
        (
            Exit::Code {
                code: 1,
                stdout: b"hgs: A transfer failed.".to_vec(),
                stderr: String::new(),
            },
            "A transfer failed.",
        ),
        (Exit::TimedOut, "The file transfer timed out."),
        (
            Exit::WriteFailed,
            "Could not send the file transfer request.",
        ),
    ];
    for (exit, text) in failures {
        assert_eq!(
            answer("example-peer", "/srv/example/report", &file, exit).unwrap_err(),
            text
        );
    }
}

#[cfg(unix)]
#[test]
fn selected_fake_hgs_downloads_only_into_private_fixtures_and_checks_real_output() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let hgs = scratch.0.join("fake-hgs");
    fs::write(&hgs,br##"#!/usr/bin/python3
import json,os,sys,tempfile
args=sys.argv[1:]
assert args[0]=='@example-peer' and args[1]=='download' and args[2]=='--path' and args[4]=='--output'
remote,output=args[3],args[5]
mode=os.environ['ZERUS_TEST_DOWNLOAD_MODE']
if mode=='failure':
    print('hgs: Synthetic remote failure.',file=sys.stderr);sys.exit(1)
if mode=='oversized':
    print('x'*65537);sys.exit(0)
if mode=='invalid':
    print('{invalid');sys.exit(0)
payload=b'private fixture bytes'
if mode!='missing':
    fd,temporary=tempfile.mkstemp(dir=os.path.dirname(output))
    with os.fdopen(fd,'wb') as f:f.write(payload)
    os.replace(temporary,output)
reply={'host':'example-peer','path':remote,'local_path':output,'bytes':len(payload)}
if mode in ('host','path','local_path'):reply[mode]='different'
if mode=='bytes':reply['bytes']+=1
if mode=='missing':reply.pop('bytes')
print(json.dumps(reply))
"##).unwrap();
    fs::set_permissions(&hgs, fs::Permissions::from_mode(0o700)).unwrap();
    for mode in [
        "valid",
        "host",
        "path",
        "local_path",
        "bytes",
        "missing",
        "invalid",
        "oversized",
        "failure",
    ] {
        let output = scratch.0.join(format!("saved {mode}.md"));
        let result = download(
            &hgs,
            &[("ZERUS_TEST_DOWNLOAD_MODE", mode)],
            "example-peer",
            "~/literal $(name)'s report.md",
            &output,
        );
        match mode {
            "valid" => {
                result.unwrap();
                assert_eq!(fs::read(&output).unwrap(), b"private fixture bytes");
            }
            "invalid" | "oversized" => assert_eq!(result.unwrap_err(), INVALID_RESPONSE),
            "failure" => assert_eq!(result.unwrap_err(), "Synthetic remote failure."),
            _ => assert_eq!(result.unwrap_err(), UNCONFIRMED),
        }
    }
}
