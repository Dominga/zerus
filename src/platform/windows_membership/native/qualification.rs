//! Ignored private fixture only. This is not a product entry, a coverage proof,
//! a selected product budget, or permission to remove anything.
use super::file_users::{private_identity, Observation, Reader, Timing};
use super::*;
use crate::host::{protocol::Integrity, token::Token};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Security::{
        GetTokenInformation, LookupPrivilegeNameW, LookupPrivilegeValueW, TokenElevation,
        TokenElevationType, TokenPrivileges, TokenSessionId, LUID_AND_ATTRIBUTES,
        SE_PRIVILEGE_ENABLED, TOKEN_INFORMATION_CLASS, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    },
    System::{SystemInformation::GetTickCount64, Threading::OpenProcessToken},
};

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Context {
    system: bool,
    elevated: bool,
    elevation_type: u32,
    medium_integrity: bool,
    debug_enabled: bool,
    enabled_privileges: Vec<String>,
    session: u32,
    impersonating: bool,
}
fn context() -> std::io::Result<Context> {
    let primary = Token::current()?;
    let mut raw = std::ptr::null_mut();
    // SAFETY: current-process pseudo handle, query ONLY; no token alteration.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful OpenProcessToken transferred this one query handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let scalar = |class: TOKEN_INFORMATION_CLASS| -> std::io::Result<u32> {
        let (mut value, mut size) = (0u32, 0u32);
        // SAFETY: one owned query token and exact writable DWORD.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                class,
                (&mut value as *mut u32).cast(),
                4,
                &mut size,
            )
        } == 0
            || size != 4
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(value)
    };
    let mut privileges = vec![0u64; 8192];
    let mut used = 0;
    // SAFETY: aligned bounded writable buffer, own query token only.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenPrivileges,
            privileges.as_mut_ptr().cast(),
            65536,
            &mut used,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if !(4..=65536).contains(&used) {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    // SAFETY: complete aligned native header fits the initialized buffer.
    let header = unsafe { &*privileges.as_ptr().cast::<TOKEN_PRIVILEGES>() };
    let count = header.PrivilegeCount as usize;
    let length = count
        .checked_mul(mem::size_of::<LUID_AND_ATTRIBUTES>())
        .and_then(|n| n.checked_add(mem::offset_of!(TOKEN_PRIVILEGES, Privileges)));
    if count > 1024 || length.is_none_or(|n| n > used as usize) {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    // SAFETY: full checked native records remain in the original buffer.
    let privileges = unsafe { std::slice::from_raw_parts(header.Privileges.as_ptr(), count) };
    let mut debug = windows_sys::Win32::Foundation::LUID::default();
    let name: Vec<_> = "SeDebugPrivilege\0".encode_utf16().collect();
    // SAFETY: fixed privilege NAME, no enable/adjust call and full output.
    if unsafe { LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut debug) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Context {
        system: primary.user()?.text()? == "S-1-5-18",
        elevated: scalar(TokenElevation)? != 0,
        elevation_type: scalar(TokenElevationType)?,
        medium_integrity: primary.integrity()?.rid == Integrity::MEDIUM,
        debug_enabled: privileges.iter().any(|p| {
            p.Luid.LowPart == debug.LowPart
                && p.Luid.HighPart == debug.HighPart
                && p.Attributes & SE_PRIVILEGE_ENABLED != 0
        }),
        enabled_privileges: privileges
            .iter()
            .filter(|p| p.Attributes & SE_PRIVILEGE_ENABLED != 0)
            .map(|p| {
                let mut name = [0u16; 128];
                let mut size = name.len() as u32;
                // SAFETY: original native LUID record and bounded output. This
                // only names existing privileges; it never enables any.
                if unsafe {
                    LookupPrivilegeNameW(std::ptr::null(), &p.Luid, name.as_mut_ptr(), &mut size)
                } == 0
                    || size == 0
                    || size as usize >= name.len()
                {
                    return Err(std::io::ErrorKind::InvalidData.into());
                }
                String::from_utf16(&name[..size as usize]).map_err(std::io::Error::other)
            })
            .collect::<std::io::Result<Vec<_>>>()?,
        session: scalar(TokenSessionId)?,
        impersonating: Token::of_thread()?.is_some(),
    })
}
fn metadata(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    nonce: String,
    count: usize,
    volume: u64,
    file: [u8; 16],
}
#[derive(Default, Serialize)]
struct Data {
    objects: usize,
    queries: usize,
    root_query_refusals: usize,
    pid_hits: usize,
    own_pid_hits: usize,
    processes: usize,
    image_reads: usize,
    cwd_reads: usize,
    actual_denials: usize,
    readable_image_denied_cwd: usize,
    other_incomplete: usize,
    census_changed: Option<bool>,
    walk_ms: u128,
    process_ms: u128,
    query_ms: u128,
    elapsed_ms: u128,
    metadata_open_ns: u128,
    object_identity_ns: u128,
    pending_sleep_ns: u128,
    pending_sleep_calls: u64,
    reader: Timing,
}

#[derive(Clone, Copy)]
enum Mode {
    Console,
    HostedCost,
}
impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Console => "console-user",
            Self::HostedCost => "hosted-cost-only",
        }
    }
    fn admits(self, context: &Context) -> bool {
        match self {
            Self::Console => {
                !context.system
                    && !context.elevated
                    && context.medium_integrity
                    && !context.debug_enabled
                    && !context.impersonating
                    && context.session == 1
            }
            // This separate ignored entry supplies timing DATA only. Its actual
            // token is reported and never qualifies the console-user row.
            Self::HostedCost => !context.impersonating,
        }
    }
}

#[test]
#[ignore = "only the coordinator-booked private console-user fixture invokes this exact case"]
fn private_measurement() {
    measure(Mode::Console);
}

#[test]
#[ignore = "only the opt-in hosted read-only timing comparison invokes this exact case"]
fn hosted_measurement() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    measure(Mode::HostedCost);
}

#[test]
fn hosted_diagnostic_admission_does_not_relax_the_console_case() {
    let mut context = Context {
        system: false,
        elevated: false,
        elevation_type: 3,
        medium_integrity: true,
        debug_enabled: false,
        enabled_privileges: vec![],
        session: 1,
        impersonating: false,
    };
    assert!(Mode::Console.admits(&context));
    context.elevated = true;
    assert!(!Mode::Console.admits(&context));
    assert!(Mode::HostedCost.admits(&context));
    context.elevated = false;
    context.system = true;
    assert!(!Mode::Console.admits(&context));
    context.system = false;
    context.medium_integrity = false;
    assert!(!Mode::Console.admits(&context));
    context.medium_integrity = true;
    context.debug_enabled = true;
    assert!(!Mode::Console.admits(&context));
    context.debug_enabled = false;
    context.session = 0;
    assert!(!Mode::Console.admits(&context));
    assert!(Mode::HostedCost.admits(&context));
    context.impersonating = true;
    assert!(!Mode::HostedCost.admits(&context));
}

fn measure(mode: Mode) {
    assert_eq!(
        std::env::var("ZERUS_MEMBERSHIP_QUALIFICATION").as_deref(),
        Ok("readonly-v1")
    );
    let invocation = Instant::now();
    // The original launcher supplies ONE boot-clock deadline before spawn.
    let tick_end: u64 = std::env::var("ZERUS_MEMBERSHIP_UNTIL_TICK")
        .unwrap()
        .parse()
        .unwrap();
    let ticks = unsafe { GetTickCount64() };
    let remaining = tick_end
        .checked_sub(ticks)
        .filter(|n| *n <= 900000)
        .expect("expired/invalid original deadline");
    let until = invocation + Duration::from_millis(remaining);
    let before = context().unwrap();
    assert!(
        mode.admits(&before),
        "measurement token or execution scope refused: {before:?}"
    );
    let count: usize = std::env::var("ZERUS_MEMBERSHIP_OBJECTS")
        .unwrap()
        .parse()
        .unwrap();
    assert!([128, 4096, 32768].contains(&count));
    let nonce = std::env::var("ZERUS_MEMBERSHIP_NONCE").unwrap();
    uuid::Uuid::parse_str(&nonce).unwrap();
    let scope = PathBuf::from(std::env::var_os("ZERUS_MEMBERSHIP_SCOPE").unwrap());
    assert!(scope.is_absolute() && scope.is_dir());
    let root = scope.join(format!("inventory-{count}"));
    let pass = std::env::var("ZERUS_MEMBERSHIP_PASS").unwrap();
    assert!(["prepare", "first-pass", "warm"].contains(&pass.as_str()));
    let prepare = pass == "prepare";
    let proof = root.join("qualification.json");
    if prepare {
        // New private data ONLY. Never overwrite an existing directory/file.
        fs::create_dir(&root).unwrap();
        for i in 0..count {
            live(until).expect("original preparation deadline");
            let bucket = root.join(format!("bucket-{:04}", i / 128));
            if i % 128 == 0 {
                fs::create_dir(&bucket).unwrap();
            }
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(bucket.join(format!("object-{i:05}")))
                .unwrap()
                .write_all(b"private fixture\n")
                .unwrap();
        }
        let file = metadata(&root).unwrap();
        let id = private_identity(&file, until).unwrap();
        let fixture = Fixture {
            nonce: nonce.clone(),
            count,
            volume: id.volume,
            file: id.file,
        };
        serde_json::to_writer(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&proof)
                .unwrap(),
            &fixture,
        )
        .unwrap();
    }
    if prepare {
        assert_eq!(before, context().unwrap());
        let record = serde_json::json!({"kind":"private-fixture-prepared-v1", "files":count,
            "scope":mode.name(), "context":before, "elapsed_ms":invocation.elapsed().as_millis()});
        let frame = super::super::file_users::private_report_frame(
            &serde_json::to_string(&record).unwrap(),
        );
        assert!(frame.len() <= 8192);
        std::io::stdout().write_all(frame.as_bytes()).unwrap();
        return;
    }
    let mut encoded = Vec::new();
    File::open(&proof)
        .unwrap()
        .take(1025)
        .read_to_end(&mut encoded)
        .unwrap();
    assert!(encoded.len() <= 1024);
    let fixture: Fixture = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(fixture.nonce, nonce);
    assert_eq!(fixture.count, count);
    let guard = metadata(&root).unwrap();
    let root_id = private_identity(&guard, until).unwrap();
    assert_eq!(
        root_id,
        super::super::handles::Object {
            volume: fixture.volume,
            file: fixture.file
        }
    );

    // ONE experimental ceiling (at most 900 s) across census/walk/queries/settlement.
    // This is not a chosen product budget. The external owned-worker launcher
    // must also bound/reap this exact test process and retain actual pipe EOF.
    let entered = Instant::now();
    let mut data = Data::default();
    let api = Api::new().unwrap();
    let mut rows_before = None;
    let mut query_started = None;
    let mut reader_slot = None;
    let result: Result<()> = (|| {
        let before = api.census(until)?;
        data.processes = before.len();
        for process in before.values() {
            live(until)?;
            match Pinned::open(*process, until) {
                Ok(pin) => {
                    let image = pin.image(until);
                    let cwd = pin.cwd(&api, until);
                    data.image_reads += usize::from(image.is_ok());
                    data.cwd_reads += usize::from(cwd.is_ok());
                    data.readable_image_denied_cwd +=
                        usize::from(image.is_ok() && cwd == Err(Error::Denied));
                    for value in [image.map(|_| ()), cwd.map(|_| ())] {
                        match value {
                            Err(Error::Denied) => data.actual_denials += 1,
                            Err(_) => data.other_incomplete += 1,
                            Ok(()) => {}
                        }
                    }
                }
                Err(Error::Denied) => data.actual_denials += 1,
                Err(_) => data.other_incomplete += 1,
            }
        }
        data.process_ms = entered.elapsed().as_millis();
        rows_before = Some(before);
        let walk_started = Instant::now();
        let mut paths = vec![root.clone()];
        let mut index = 0;
        let mut path_bytes = root
            .as_os_str()
            .encode_wide()
            .count()
            .checked_mul(2)
            .ok_or(Error::Incomplete)?;
        if path_bytes > MAX_PATH {
            return Err(Error::Incomplete);
        }
        while index < paths.len() {
            live(until)?;
            let path = paths[index].clone();
            index += 1;
            let meta = fs::symlink_metadata(&path).map_err(|_| Error::Incomplete)?;
            if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(Error::Incomplete);
            }
            if meta.is_dir() {
                for entry in fs::read_dir(&path).map_err(|_| Error::Incomplete)? {
                    live(until)?;
                    let path = entry.map_err(|_| Error::Incomplete)?.path();
                    let bytes = path
                        .as_os_str()
                        .encode_wide()
                        .count()
                        .checked_mul(2)
                        .ok_or(Error::Incomplete)?;
                    path_bytes = path_bytes.checked_add(bytes).ok_or(Error::Incomplete)?;
                    if paths.len() >= 65536 || bytes > MAX_PATH || path_bytes > 16 << 20 {
                        return Err(Error::Incomplete);
                    }
                    paths.push(path);
                }
            } else if !meta.is_file() {
                return Err(Error::Incomplete);
            }
        }
        data.objects = paths.len();
        data.walk_ms = walk_started.elapsed().as_millis();
        query_started = Some(Instant::now());
        let request = uuid::Uuid::new_v4();
        reader_slot = Some(Reader::new(request, until)?);
        for path in paths {
            live(until)?;
            let open_started = Instant::now();
            let opened = metadata(&path);
            data.metadata_open_ns += open_started.elapsed().as_nanos();
            let file = match opened {
                Ok(file) => file,
                Err(_) => {
                    data.root_query_refusals += 1;
                    continue;
                }
            };
            let identity_started = Instant::now();
            let identified = private_identity(&file, until);
            data.object_identity_ns += identity_started.elapsed().as_nanos();
            let object = identified?;
            let mut outcome = reader_slot.as_mut().unwrap().start(file, object);
            loop {
                match outcome {
                    Observation::Positive(users) => {
                        data.queries += 1;
                        data.pid_hits += users.pids.len();
                        data.own_pid_hits += usize::from(users.pids.contains(&std::process::id()));
                        break;
                    }
                    Observation::Incomplete => {
                        data.root_query_refusals += 1;
                        // This original query has physically completed. Keep
                        // its refusal and inspect the NEXT object, never retry
                        // this one or hide readable use elsewhere. Same root
                        // request and original deadline, no budget renewal.
                        let reader = reader_slot.take().unwrap();
                        data.reader.add(reader.private_timing());
                        drop(reader);
                        reader_slot = Some(Reader::new(request, until)?);
                        break;
                    }
                    Observation::Pending { .. } => {
                        // Keep the ORIGINAL native file/buffers through real
                        // completion even after the logical bound. The outer
                        // retained worker/job is the hard failure backstop.
                        let sleep_started = Instant::now();
                        thread::sleep(Duration::from_millis(1));
                        data.pending_sleep_ns += sleep_started.elapsed().as_nanos();
                        data.pending_sleep_calls += 1;
                        outcome = reader_slot.as_mut().unwrap().poll();
                    }
                }
            }
        }
        if private_identity(&guard, until)? != root_id {
            return Err(Error::Incomplete);
        }
        let after = api.census(until)?;
        data.census_changed = Some(rows_before.as_ref() != Some(&after));
        if data.root_query_refusals != 0 {
            return Err(Error::Incomplete);
        }
        live(until)
    })();
    if let Some(reader) = reader_slot.take() {
        data.reader.add(reader.private_timing());
        drop(reader);
    }
    if let Some(started) = query_started {
        data.query_ms = started.elapsed().as_millis();
    }
    data.elapsed_ms = entered.elapsed().as_millis();
    let after = context().unwrap();
    assert_eq!(
        before, after,
        "token context changed during private measurement"
    );
    // No raw SID/PID, path, environment or frame values leave this record.
    let within = Instant::now() < until && unsafe { GetTickCount64() } < tick_end;
    let report = serde_json::json!({"kind":"usage-timing-data-only-v1", "scope":mode.name(), "context":before,
        "pass":pass, "files":count,
        "within_original_deadline": within,
        "invocation_ms":invocation.elapsed().as_millis(),
        "status": if result.is_ok() && within {"data_only"} else {"incomplete"}, "data":data});
    // Direct bounded output is available even if a harness captures macros.
    let frame =
        super::super::file_users::private_report_frame(&serde_json::to_string(&report).unwrap());
    assert!(frame.len() <= 8192);
    std::io::stdout().write_all(frame.as_bytes()).unwrap();
    // Leave only the deliberately retained private fixture for the warm pass;
    // the original launcher owns its bounded final cleanup. No deletion here.
}
