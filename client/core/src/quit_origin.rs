//! Original process inputs for a future native quit-origin binding.
//!
//! Capture once at the executable entry, before loaders change the environment.
//! These private bytes are DATA, not configuration, image, caller or admission
//! proof. There is deliberately no successful admission result or raw-value API.
//! No file, process, connection or production anchor is inspected here.
use std::{
    ffi::{OsStr, OsString},
    sync::OnceLock,
};

// Bounds on retained input, not the standard library's temporary OS snapshots.
const MAX_ENVIRONMENT: usize = 1024;
const MAX_ARGUMENTS: usize = 256;
const MAX_BYTES: usize = 1 << 20;
const MAX_STRING: usize = 64 << 10;

static ENTRY: OnceLock<Capture> = OnceLock::new();

/// Preserve raw OS inputs at main entry, before parsing, loaders or workers.
/// Repeated calls never replace an earlier capture, including a failed one.
/// Library entry points that did not pass through main remain unavailable.
pub fn capture_process_entry() {
    ENTRY.get_or_init(|| Capture::read(std::env::vars_os(), std::env::args_os()));
}

/// Closed reasons only: never include environment values or argument text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotCaptured,
    InputLimit,
    DuplicateEnvironment,
    ExplicitCli,
    ExplicitEnvironment,
    NativeOriginUnavailable,
}

/// An input observation can only explain refusal. Even an implicit input scope
/// lacks the shared native image/configuration binding and resource admission.
pub fn refusal() -> Refusal {
    ENTRY.get().map_or(Refusal::NotCaptured, Capture::refusal)
}

// No Clone, Debug, serde or public constructor/accessor for original bytes.
enum Capture {
    Inputs(Inputs),
    Unavailable(Refusal),
}
struct Inputs {
    environment: Vec<(OsString, OsString)>,
    arguments: Vec<OsString>,
}

impl Capture {
    fn read(
        environment: impl IntoIterator<Item = (OsString, OsString)>,
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Self {
        let mut inputs = Inputs {
            environment: Vec::new(),
            arguments: Vec::new(),
        };
        let mut bytes = 0;
        for (name, value) in environment {
            if inputs.environment.len() == MAX_ENVIRONMENT
                || !charge(&mut bytes, &name)
                || !charge(&mut bytes, &value)
            {
                return Self::Unavailable(Refusal::InputLimit);
            }
            if inputs
                .environment
                .iter()
                .any(|(original, _)| same_key(original, &name))
            {
                return Self::Unavailable(Refusal::DuplicateEnvironment);
            }
            inputs.environment.push((name, value));
        }
        for argument in arguments {
            if inputs.arguments.len() == MAX_ARGUMENTS || !charge(&mut bytes, &argument) {
                return Self::Unavailable(Refusal::InputLimit);
            }
            inputs.arguments.push(argument);
        }
        Self::Inputs(inputs)
    }

    fn refusal(&self) -> Refusal {
        let inputs = match self {
            Self::Inputs(inputs) => inputs,
            Self::Unavailable(reason) => return *reason,
        };
        if inputs.arguments.iter().skip(1).any(|argument| {
            argument == "--hgs" || argument.as_encoded_bytes().starts_with(b"--hgs=")
        }) {
            return Refusal::ExplicitCli;
        }
        // No prefixed key is an allow flag. Presentation-only values are also
        // retained: proving their irrelevance belongs to the future binding.
        if inputs
            .environment
            .iter()
            .any(|(key, _)| prefixed(key, b"HGS_") || prefixed(key, b"ZERUS_CLIENT_"))
        {
            return Refusal::ExplicitEnvironment;
        }
        Refusal::NativeOriginUnavailable
    }
}

fn charge(total: &mut usize, value: &OsStr) -> bool {
    let size = value.as_encoded_bytes().len();
    if size > MAX_STRING || size > MAX_BYTES.saturating_sub(*total) {
        return false;
    }
    *total += size;
    true
}

fn same_key(left: &OsStr, right: &OsStr) -> bool {
    if cfg!(windows) {
        left.as_encoded_bytes()
            .eq_ignore_ascii_case(right.as_encoded_bytes())
    } else {
        left == right
    }
}

fn prefixed(key: &OsStr, prefix: &[u8]) -> bool {
    let bytes = key.as_encoded_bytes();
    bytes.get(..prefix.len()).is_some_and(|start| {
        if cfg!(windows) {
            start.eq_ignore_ascii_case(prefix)
        } else {
            start == prefix
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_input() -> OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0xff])
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0xd800])
        }
    }

    fn capture(environment: &[(&str, &str)], arguments: &[&str]) -> Capture {
        Capture::read(
            environment
                .iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value))),
            arguments.iter().map(OsString::from),
        )
    }

    #[test]
    fn process_capture_keeps_pre_loader_bytes_and_cannot_be_recaptured() {
        const CHILD: &str = "ZERUS_QUIT_INPUT_CAPTURE_TEST";
        if std::env::var_os(CHILD).is_some() {
            assert_eq!(refusal(), Refusal::NotCaptured);
            capture_process_entry();
            assert_eq!(refusal(), Refusal::ExplicitEnvironment);
            std::env::remove_var("HGS_STATE_DIR");
            std::env::set_var("HOME", "/later/loader/home");
            capture_process_entry();
            let Some(Capture::Inputs(inputs)) = ENTRY.get() else {
                panic!("entry capture unavailable")
            };
            assert!(inputs
                .environment
                .iter()
                .any(|(key, value)| key == "HGS_STATE_DIR" && value.is_empty()));
            assert!(inputs
                .environment
                .iter()
                .any(|(key, value)| key == "HOME" && value == "./original/../home"));
            assert!(inputs
                .environment
                .iter()
                .any(|(key, value)| key == "QUIT_INPUT_RAW_OS" && value == &raw_input()));
            assert_eq!(refusal(), Refusal::ExplicitEnvironment);
            return;
        }
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .arg("--exact")
            .arg("quit_origin::tests::process_capture_keeps_pre_loader_bytes_and_cannot_be_recaptured")
            .env(CHILD, "1")
            .env("HGS_STATE_DIR", "")
            .env("HOME", "./original/../home")
            .env("QUIT_INPUT_RAW_OS", raw_input())
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        let mut child = command.spawn().unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "original-input child failed");
                break;
            }
            if Instant::now() >= end {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("original-input child timed out");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn explicit_inputs_survive_loader_changes_and_do_not_become_defaults() {
        let entry = OnceLock::new();
        entry.get_or_init(|| capture(&[("HGS_STATE_DIR", "")], &["hgs"]));
        entry.get_or_init(|| capture(&[], &["hgs"]));
        assert_eq!(entry.get().unwrap().refusal(), Refusal::ExplicitEnvironment);

        let entry = OnceLock::new();
        entry.get_or_init(|| capture(&[], &["hgs-tray", "--hgs", "/selected/hgs"]));
        entry.get_or_init(|| capture(&[("ZERUS_CLIENT_HGS", "/other/hgs")], &["hgs-tray"]));
        let Capture::Inputs(inputs) = entry.get().unwrap() else {
            panic!("bounded capture refused")
        };
        assert!(inputs.environment.is_empty());
        assert_eq!(inputs.arguments[2], "/selected/hgs");
        assert_eq!(entry.get().unwrap().refusal(), Refusal::ExplicitCli);
    }

    #[test]
    fn default_looking_false_unknown_and_presentation_inputs_supply_no_authority() {
        for key in [
            "HGS_STATE_DIR",
            "HGS_CONFIG_DIR",
            "HGS_INSTALL_PREFIX",
            "HGS_SELF",
            "HGS_PEERS",
            "HGS_SSH",
            "HGS_SESSION",
            "HGS_TEST_FUTURE",
            "HGS_FUTURE",
            "ZERUS_CLIENT_HGS",
            "ZERUS_CLIENT_REPLAY",
            "ZERUS_CLIENT_PERF_FUTURE",
            "ZERUS_CLIENT_FUTURE",
            "HGS_LANG",
            "ZERUS_CLIENT_QUERY",
        ] {
            for value in ["", "0", "false", "/default/looking/path"] {
                assert_eq!(
                    capture(&[(key, value)], &["hgs"]).refusal(),
                    Refusal::ExplicitEnvironment
                );
            }
        }
        for arguments in [vec!["hgs-tray", "--hgs="], vec!["hgs-tray", "--hgs"]] {
            assert_eq!(capture(&[], &arguments).refusal(), Refusal::ExplicitCli);
        }
        assert_eq!(
            capture(&[("HOME", "/default"), ("PATH", "/usr/bin")], &["hgs"]).refusal(),
            Refusal::NativeOriginUnavailable
        );
        assert_eq!(refusal(), Refusal::NotCaptured);
    }

    #[test]
    fn retention_limits_and_duplicates_refuse_the_entire_capture() {
        for entries in [MAX_ENVIRONMENT, MAX_ENVIRONMENT + 1] {
            let captured = Capture::read(
                (0..entries).map(|i| (format!("KEY{i}").into(), OsString::new())),
                [OsString::from("hgs")],
            );
            assert_eq!(
                captured.refusal(),
                if entries == MAX_ENVIRONMENT {
                    Refusal::NativeOriginUnavailable
                } else {
                    Refusal::InputLimit
                }
            );
        }
        assert_eq!(
            capture(&[("HOME", "first"), ("HOME", "second")], &["hgs"]).refusal(),
            Refusal::DuplicateEnvironment
        );
        let too_long = "a".repeat(MAX_STRING + 1);
        assert_eq!(
            capture(&[("HOME", &too_long)], &["hgs"]).refusal(),
            Refusal::InputLimit
        );
        assert_eq!(
            capture(&[], &vec!["arg"; MAX_ARGUMENTS + 1]).refusal(),
            Refusal::InputLimit
        );
        let large = "a".repeat(MAX_STRING);
        assert_eq!(
            capture(&[], &vec![large.as_str(); 17]).refusal(),
            Refusal::InputLimit
        );

        let entry = OnceLock::new();
        entry.get_or_init(|| capture(&[("HOME", &too_long)], &["hgs"]));
        entry.get_or_init(|| capture(&[], &["hgs"]));
        assert_eq!(entry.get().unwrap().refusal(), Refusal::InputLimit);
    }

    #[test]
    fn os_profile_inputs_are_retained_exactly_without_deriving_a_profile() {
        let captured = capture(
            &[
                ("HOME", ""),
                ("XDG_STATE_HOME", "./state/../state"),
                ("PATH", "a::b"),
            ],
            &["./hgs", "--sessions"],
        );
        let Capture::Inputs(inputs) = captured else {
            panic!("capture refused")
        };
        assert_eq!(inputs.environment[0].1, "");
        assert_eq!(inputs.environment[1].1, "./state/../state");
        assert_eq!(inputs.environment[2].1, "a::b");
        assert_eq!(inputs.arguments[0], "./hgs");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_values_and_prefixed_names_are_never_lost() {
        use std::os::unix::ffi::OsStringExt;
        let value = OsString::from_vec(vec![0xff]);
        let captured = Capture::read(
            [(OsString::from("HGS_STATE_DIR"), value.clone())],
            [OsString::from("hgs")],
        );
        assert_eq!(captured.refusal(), Refusal::ExplicitEnvironment);
        let Capture::Inputs(inputs) = captured else {
            panic!("capture refused")
        };
        assert_eq!(inputs.environment[0].1, value);
        assert_eq!(
            Capture::read(
                [(OsString::from_vec(b"HGS_\xff".to_vec()), OsString::new())],
                [OsString::from("hgs")],
            )
            .refusal(),
            Refusal::ExplicitEnvironment
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_selector_names_ignore_ascii_case_and_keep_unpaired_surrogates() {
        use std::os::windows::ffi::OsStringExt;
        let value = OsString::from_wide(&[0xd800]);
        let captured = Capture::read(
            [(OsString::from("hgs_state_dir"), value.clone())],
            [OsString::from("hgs")],
        );
        assert_eq!(captured.refusal(), Refusal::ExplicitEnvironment);
        let Capture::Inputs(inputs) = captured else {
            panic!("capture refused")
        };
        assert_eq!(inputs.environment[0].1, value);
        assert_eq!(
            capture(
                &[("HGS_STATE_DIR", "first"), ("hgs_state_dir", "second")],
                &["hgs"]
            )
            .refusal(),
            Refusal::DuplicateEnvironment
        );
    }
}
