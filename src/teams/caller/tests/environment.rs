//! The parser of a macOS `KERN_PROCARGS2` answer and the claim read from it,
//! run on every platform. Each shape is an answer for a recorded agent, whose
//! markers must be found or the read refused: `NoClaim` for a marked agent
//! would fail open. The 35 shapes are the independent review's of 4ecb4f8d2.
use super::*;

#[derive(Debug, PartialEq)]
enum Outcome {
    /// An error carrying the withheld marker: refused below the top.
    Hidden,
    /// Any other error: refused below the top.
    Invalid,
    /// The markers were found; the record check runs.
    Claimed {
        name: String,
        run: String,
        session: Option<String>,
    },
    /// `claim` refused: partial, duplicate or non-UTF-8 markers.
    Refused,
    /// A readable environment without markers.
    NoClaim,
}
use Outcome::*;

fn read(bytes: &[u8], capacity: usize) -> Outcome {
    match procargs_environment(bytes, capacity) {
        Err(error) if is_withheld(&error) => Hidden,
        Err(_) => Invalid,
        Ok(environment) => match claim(&environment) {
            Ok(Some(Claim { name, run, session })) => Claimed { name, run, session },
            Ok(None) => NoClaim,
            Err(_) => Refused,
        },
    }
}

fn parse(bytes: &[u8]) -> Outcome {
    read(bytes, PROCARGS_BUFFER)
}

/// The executable path with its NUL, padded to 8 bytes as the kernel lays it
/// out. Only the fixtures use it; the parser does not rely on it.
fn pad_for(path: &[u8]) -> usize {
    (8 - (path.len() + 1) % 8) % 8
}

fn answer(argc: i32, path: &[u8], args: &[&[u8]], tail: &[u8]) -> Vec<u8> {
    let mut bytes = argc.to_ne_bytes().to_vec();
    bytes.extend_from_slice(path);
    bytes.push(0);
    bytes.extend(std::iter::repeat_n(0, pad_for(path)));
    for arg in args {
        bytes.extend_from_slice(arg);
        bytes.push(0);
    }
    bytes.extend_from_slice(tail);
    bytes
}

/// The kernel's own strings after the environment.
const APPLE: &[u8] = b"ptr_munge=\0main_stack=\0executable_file=0x1a01000009,0x2d4e1b\0dyld_file=0x1a01000009,0xfffffff000\0executable_cdhash=0123456789abcdef\0th_port=\0\0\0\0\0";

fn env(entries: &[&[u8]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for entry in entries {
        bytes.extend_from_slice(entry);
        bytes.push(0);
    }
    bytes.extend_from_slice(APPLE);
    bytes
}

const AGENT: &[u8] = b"/opt/homebrew/bin/agent";
const SESSION: &[u8] = b"HGS_SESSION=claude/example/one";
const RUN: &[u8] = b"HGS_RUN_ID=run-1";
const ID: &[u8] = b"HGS_SESSION_ID=00000000-0000-4000-8000-0000000000a1";
const PATH: &[u8] = b"PATH=/usr/bin:/bin";
const HINT: Option<&str> = Some("00000000-0000-4000-8000-0000000000a1");

fn agent(session: Option<&str>) -> Outcome {
    Claimed {
        name: "claude/example/one".into(),
        run: "run-1".into(),
        session: session.map(Into::into),
    }
}

#[test]
fn an_ordinary_agent_answer_is_claimed() {
    let bytes = answer(
        2,
        AGENT,
        &[b"agent", b"--resume"],
        &env(&[PATH, SESSION, RUN, ID]),
    );
    assert_eq!(parse(&bytes), agent(HINT));
}

#[test]
fn empty_middle_and_last_arguments_are_counted() {
    let bytes = answer(3, AGENT, &[b"agent", b"", b"-p"], &env(&[SESSION, RUN, ID]));
    assert_eq!(parse(&bytes), agent(HINT));
    let bytes = answer(3, AGENT, &[b"agent", b"-p", b""], &env(&[SESSION, RUN, ID]));
    assert_eq!(parse(&bytes), agent(HINT));
}

/// An empty first argument is taken as the path's padding, so the count runs
/// into the environment; a marker read as an argument makes it unreadable.
#[test]
fn an_empty_first_argument_that_shifts_the_count_is_unreadable() {
    let bytes = answer(2, AGENT, &[b"", b"x"], &env(&[SESSION, RUN, ID, PATH]));
    assert_eq!(parse(&bytes), Invalid, "HGS_SESSION first");
    let bytes = answer(2, AGENT, &[b"", b"x"], &env(&[ID, SESSION, RUN, PATH]));
    assert_eq!(parse(&bytes), Invalid, "HGS_SESSION_ID first");
    let bytes = answer(
        4,
        AGENT,
        &[b"", b"", b"", b"x"],
        &env(&[SESSION, RUN, ID, PATH]),
    );
    assert_eq!(parse(&bytes), Invalid, "three leading empty arguments");
    let bytes = answer(2, AGENT, &[b"", b"x"], &env(&[PATH, SESSION, RUN, ID]));
    assert_eq!(
        parse(&bytes),
        agent(HINT),
        "PATH first, the markers survive"
    );
}

#[test]
fn an_argument_count_out_of_range_or_a_short_answer_is_invalid() {
    for argc in [0, -1, i32::MIN, 4097, i32::MAX] {
        let bytes = answer(argc, AGENT, &[], &env(&[SESSION, RUN, ID]));
        assert_eq!(parse(&bytes), Invalid, "argc {argc}");
    }
    assert_eq!(parse(&[1, 0]), Invalid, "shorter than the count");
    assert_eq!(parse(&[]), Invalid, "empty answer");
    assert_eq!(
        parse(&answer(1, AGENT, &[], b"")[..12]),
        Invalid,
        "no NUL after the path"
    );
}

/// The count exceeds the strings in the argument area only when the process
/// rewrote that area with fewer separators.
#[test]
fn more_arguments_counted_than_present_are_unreadable() {
    let bytes = answer(4, AGENT, &[b"agent", b"-p"], b"");
    assert_eq!(parse(&bytes), Invalid, "nothing after them");
    let bytes = answer(
        5,
        AGENT,
        &[b"agent", b"-p"],
        &env(&[SESSION, RUN, ID, PATH]),
    );
    assert_eq!(parse(&bytes), Invalid, "markers first after them");
}

#[test]
fn equals_signs_in_arguments_and_values() {
    let bytes = answer(
        3,
        AGENT,
        &[b"sh", b"HGS_SESSION=evil", b"HGS_RUN_ID=evil"],
        &env(&[PATH]),
    );
    assert_eq!(parse(&bytes), Invalid, "marker-shaped arguments");
    let bytes = answer(
        1,
        AGENT,
        &[b"agent"],
        &env(&[b"HGS_SESSION=a=b", b"HGS_RUN_ID=r=1"]),
    );
    let claimed = Claimed {
        name: "a=b".into(),
        run: "r=1".into(),
        session: None,
    };
    assert_eq!(parse(&bytes), claimed, "values that contain '='");
}

/// An answer that fills the buffer may be cut, at either end; it is
/// unreadable. A cut exactly after the arguments reads as withheld.
#[test]
fn an_answer_that_fills_the_buffer_is_unreadable() {
    let full = answer(1, AGENT, &[b"agent"], &env(&[PATH, SESSION, RUN, ID]));
    let cut = 4 + AGENT.len() + 1 + pad_for(AGENT) + 6 + PATH.len() + 1;
    let end_cut = &full[..cut];
    assert_eq!(read(end_cut, end_cut.len()), Invalid, "end cut");
    let mut area = AGENT.to_vec();
    area.push(0);
    area.extend(std::iter::repeat_n(0, pad_for(AGENT)));
    area.extend_from_slice(b"agent\0-a\0--long-argument\0");
    area.extend_from_slice(&env(&[SESSION, RUN, ID, PATH]));
    let skip = AGENT.len() + 1 + pad_for(AGENT) + 6 + 3 + 5;
    let mut head_cut = 3i32.to_ne_bytes().to_vec();
    head_cut.extend_from_slice(&area[skip..]);
    assert_eq!(read(&head_cut, head_cut.len()), Invalid, "head cut");
    let at_arguments = answer(1, AGENT, &[b"agent"], b"");
    assert_eq!(
        parse(&at_arguments),
        Hidden,
        "cut exactly after the arguments"
    );
}

#[test]
fn bytes_that_are_not_utf8() {
    let bytes = answer(
        1,
        b"/tmp/\xff\xfe",
        &[b"\xc3\x28"],
        &env(&[b"LANG=\xff", SESSION, RUN]),
    );
    assert_eq!(parse(&bytes), agent(None), "elsewhere than a marker");
    let bytes = answer(1, AGENT, &[b"agent"], &env(&[b"HGS_SESSION=\xff", RUN]));
    assert_eq!(parse(&bytes), Refused, "in the session marker");
    let bytes = answer(
        1,
        AGENT,
        &[b"agent"],
        &env(&[SESSION, RUN, b"HGS_SESSION_ID=\xff"]),
    );
    assert_eq!(parse(&bytes), Refused, "in the ID hint");
}

/// A tail with no byte other than NUL is withheld, never an empty
/// environment: a really empty one still holds the kernel's own strings.
#[test]
fn a_tail_of_only_nul_bytes_is_withheld_and_kernel_strings_are_read() {
    let bytes = answer(1, AGENT, &[b"agent"], APPLE);
    assert_eq!(
        parse(&bytes),
        NoClaim,
        "an empty environment of an unmarked process"
    );
    let bytes = answer(1, AGENT, &[b"agent"], b"\0");
    assert_eq!(parse(&bytes), Hidden, "one NUL");
    let bytes = answer(1, AGENT, &[b"agent"], &[0; 8]);
    assert_eq!(parse(&bytes), Hidden, "eight NULs");
    let bytes = answer(1, AGENT, &[b"agent"], &env(&[SESSION, RUN]));
    assert_eq!(parse(&bytes), agent(None), "kernel strings after markers");
}

/// The agent's runtime rewrote its own argument area.
#[test]
fn a_rewritten_argument_area() {
    let args: [&[u8]; 4] = [b"node", b"/usr/local/lib/cli.js", b"--flag", b"x"];
    let original = answer(4, AGENT, &args, &env(&[SESSION, RUN, ID, PATH]));
    let start = 4 + AGENT.len() + 1 + pad_for(AGENT);
    let span: usize = args.iter().map(|arg| arg.len() + 1).sum();
    let mut title = original.clone();
    title[start..start + span].fill(0);
    title[start..start + 6].copy_from_slice(b"claude");
    assert_eq!(parse(&title), agent(HINT), "a title padded with NUL bytes");
    let mut joined = original.clone();
    for byte in &mut joined[start..start + span - 1] {
        if *byte == 0 {
            *byte = b' ';
        }
    }
    assert_eq!(parse(&joined), Invalid, "arguments joined with spaces");
    // The stated limit, the same as on Linux: a process that clobbers its
    // arguments and its environment together removes its own markers.
    let mut clobbered = original.clone();
    let environment = SESSION.len() + RUN.len() + ID.len() + PATH.len() + 4;
    clobbered[start..start + span + environment].fill(0);
    clobbered[start..start + 6].copy_from_slice(b"claude");
    assert_eq!(
        parse(&clobbered),
        NoClaim,
        "arguments and environment clobbered"
    );
}

/// The agent's own `getenv` takes the first of two entries and a reader of
/// the block could take another, so a marker named twice is unreadable.
#[test]
fn a_marker_named_twice_is_unreadable() {
    let bytes = answer(
        1,
        AGENT,
        &[b"agent"],
        &env(&[SESSION, RUN, b"HGS_SESSION=claude/example/other"]),
    );
    assert_eq!(parse(&bytes), Refused, "HGS_SESSION twice");
    let bytes = answer(
        1,
        AGENT,
        &[b"agent"],
        &env(&[SESSION, RUN, b"HGS_RUN_ID=run-2"]),
    );
    assert_eq!(parse(&bytes), Refused, "HGS_RUN_ID twice");
    let bytes = answer(1, AGENT, &[b"agent"], &env(&[SESSION, RUN, ID, ID]));
    assert_eq!(parse(&bytes), Refused, "HGS_SESSION_ID twice");
    assert!(claim(b"HGS_SESSION=a\0HGS_RUN_ID=r\0HGS_RUN_ID=r\0").is_err());
    assert!(claim(b"HGS_SESSION=a\0HGS_RUN_ID=r\0HGS_SESSION_ID=x\0").is_ok());
}

#[test]
fn withheld_shapes() {
    let bytes = answer(3, b"/bin/zsh", &[b"-zsh", b"-c", b""], b"");
    assert_eq!(parse(&bytes), Hidden, "last argument empty");
    let bytes = answer(2, b"/bin/sh", &[b"", b"x"], b"");
    assert_eq!(parse(&bytes), Invalid, "first argument empty");
}
