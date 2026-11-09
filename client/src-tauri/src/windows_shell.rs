//! Windows parity decisions shared by the native adapter and its unit tests.
//! Qt: AppConfig::chooseHgs/restartArguments and SwarmWorkerSupervisor.
#[cfg(any(feature = "live", test))]
use serde_json::Value;
#[cfg(any(feature = "live", test))]
use std::path::{Path, PathBuf};

#[cfg(any(feature = "live", test))]
pub struct HgsChoice {
    pub path: PathBuf,
    pub refresh: bool,
    pub fallback: PathBuf,
}

#[cfg(any(feature = "live", test))]
fn version(value: &str) -> Option<semver::Version> {
    semver::Version::parse(value).ok()
}

/// The session-host enabled marker is authoritative, not leftover staged files.
/// An explicit CLI selector never stages or switches to another executable.
/// `app`, this app's own canonical image, stages again when the staged binding
/// does not name it (docs/desktop-windows-mode.md): same or newer version only,
/// and staging copies nothing when the CLI is unchanged.
#[cfg(any(feature = "live", test))]
pub fn choose_hgs(
    status: &Value,
    bundled: &Path,
    digest: &str,
    configured: Option<&Path>,
    app: Option<&Path>,
) -> HgsChoice {
    let choice = |path: PathBuf, refresh, fallback: PathBuf| HgsChoice {
        path,
        refresh,
        fallback,
    };
    if let Some(path) = configured {
        return choice(path.into(), false, path.into());
    }
    let cli = &status["cli"];
    let Some(path) = cli["path"].as_str().filter(|p| !p.is_empty()) else {
        return choice(bundled.into(), false, bundled.into());
    };
    if status["enabled"] != true {
        return choice(bundled.into(), false, bundled.into());
    }
    let stable = PathBuf::from(path);
    let staged = cli["sha256"].as_str().unwrap_or_default();
    if cli["installed"] != true || staged.is_empty() {
        return choice(stable, true, bundled.into());
    }
    let differs = !digest.is_empty() && !digest.eq_ignore_ascii_case(staged);
    let newer = version(status["version"].as_str().unwrap_or_default()).is_some_and(|current| {
        version(cli["version"].as_str().unwrap_or_default())
            .is_none_or(|prior| current.cmp_precedence(&prior).is_ge())
    });
    let unbound = app.is_some_and(|app| !bound(&status["frontend"], app));
    choice(stable.clone(), (differs || unbound) && newer, stable)
}

/// Whether the status binding names `app`, still names the staged CLI and
/// still holds for the file at the app's path (a same-path replacement does
/// not). Informational: the CLI proves the pair itself before admitting it.
#[cfg(any(feature = "live", test))]
fn bound(frontend: &Value, app: &Path) -> bool {
    let plain = |path: &str| {
        path.strip_prefix(r"\\?\")
            .unwrap_or(path)
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    frontend["cli_current"] == true
        && frontend["frontend_current"] == true
        && frontend["path"]
            .as_str()
            .is_some_and(|path| plain(path) == plain(&app.to_string_lossy()))
}

/// Value options are walked as pairs, so a value spelling --sessions survives.
#[cfg(any(feature = "live", test))]
pub fn restart_arguments(args: &[String]) -> Vec<String> {
    let valued = [
        "--hgs",
        "--local-poll",
        "--peer-poll",
        "--terminal",
        "--icon-color",
        "--open-mode",
    ];
    let mut out = vec!["--background".into()];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if valued.contains(&arg.as_str()) {
            out.push(arg.clone());
            if let Some(value) = args.next() {
                out.push(value.clone());
            }
        } else if ![
            "--sessions",
            "--background",
            "--install-login-item",
            "--remove-login-item",
        ]
        .contains(&arg.as_str())
        {
            out.push(arg.clone());
        }
    }
    out
}

/// What a click on the latest tray balloon opens: the update alert opens
/// Updates, as on macOS and Linux (zerus-5wmh.13.14.1); an attention notice
/// goes to the desk, which opens its session.
#[cfg(any(feature = "live", test))]
#[derive(Debug, PartialEq, Eq)]
pub enum BalloonClick {
    Updates,
    Desk(String),
    Nothing,
}

#[cfg(any(feature = "live", test))]
pub fn balloon_click(token: Option<String>) -> BalloonClick {
    match token {
        Some(token) if zerus_client_core::updates::is_update_token(&token) => BalloonClick::Updates,
        Some(token) => BalloonClick::Desk(token),
        None => BalloonClick::Nothing,
    }
}

/// Windows tray clicks do not identify a message; Qt also routes only the latest.
#[cfg(any(feature = "live", test))]
#[derive(Default)]
pub struct NoticeClicks(Option<String>);
#[cfg(any(feature = "live", test))]
impl NoticeClicks {
    pub fn raised(&mut self, token: String) {
        self.0 = Some(token);
    }
    pub fn cleared(&mut self, tokens: &[String]) {
        if self.0.as_ref().is_some_and(|token| tokens.contains(token)) {
            self.0 = None;
        }
    }
    pub fn clicked(&self) -> Option<String> {
        self.0.clone()
    }
}

#[cfg(any(feature = "live", test))]
pub fn restart_delay(previous: u64, uptime: std::time::Duration) -> u64 {
    if previous == 0 || uptime.as_secs() >= 60 {
        1000
    } else {
        (previous * 2).min(300_000)
    }
}

/// The only calls a Windows presentation may make to come to the front. The
/// seam deliberately has no input injection (SendInput/keybd_event), no
/// AttachThreadInput and no foreground-lock change: Tao's set_focus falls back
/// to a synthetic Alt press, which must never reach the app in front.
pub trait Foreground {
    fn minimized(&self) -> bool;
    /// ShowWindow(SW_RESTORE).
    fn restore(&self);
    fn in_front(&self) -> bool;
    /// One SetForegroundWindow call.
    fn take_front(&self) -> bool;
    /// A bounded FlashWindowEx on the taskbar button.
    fn flash(&self);
}
#[derive(Debug, PartialEq, Eq)]
pub enum Front {
    Already,
    Taken,
    Flashed,
}
/// One direct attempt; on refusal only the taskbar button flashes.
pub fn bring_to_front(window: &impl Foreground) -> Front {
    if window.minimized() {
        window.restore();
    }
    if window.in_front() {
        return Front::Already;
    }
    if window.take_front() && window.in_front() {
        return Front::Taken;
    }
    window.flash();
    Front::Flashed
}

/// Windows native strings are bounded in UTF-16 without splitting a surrogate.
#[cfg(any(feature = "live", test))]
pub fn wide_text<const N: usize>(text: &str) -> [u16; N] {
    let mut out = [0; N];
    let mut at = 0;
    for c in text.chars().filter(|c| *c != '\0') {
        if at + c.len_utf16() >= N {
            break;
        }
        let mut units = [0; 2];
        for unit in c.encode_utf16(&mut units) {
            out[at] = *unit;
            at += 1;
        }
    }
    out
}

/// A version heading never displaces the existing bounded tray statistics.
#[cfg(any(feature = "live", test))]
pub fn versioned_tooltip<const N: usize>(title: &str, body: &str) -> String {
    let units = title.encode_utf16().count() + 1 + body.encode_utf16().count();
    if units < N {
        format!("{title}\n{body}")
    } else {
        body.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_balloon_click_opens_updates_for_the_update_alert_and_the_session_otherwise() {
        assert_eq!(
            balloon_click(Some("zerus-update:v1:stable:42".into())),
            BalloonClick::Updates
        );
        assert_eq!(
            balloon_click(Some("zerus-update:v2:dev:7".into())),
            BalloonClick::Updates
        );
        assert_eq!(
            balloon_click(Some("attention:computer:run:key".into())),
            BalloonClick::Desk("attention:computer:run:key".into())
        );
        assert_eq!(balloon_click(None), BalloonClick::Nothing);
    }
    use serde_json::json;
    #[cfg(windows)]
    #[test]
    fn library_test_image_embeds_common_controls_v6() {
        use windows_sys::Win32::{System::LibraryLoader::*, UI::WindowsAndMessaging::RT_MANIFEST};
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            // MAKEINTRESOURCEW(1) encodes an integer, not an aligned string pointer.
            let resource =
                FindResourceW(module, std::ptr::without_provenance::<u16>(1), RT_MANIFEST);
            assert!(
                !resource.is_null(),
                "unit-test image must carry its own manifest"
            );
            let size = SizeofResource(module, resource) as usize;
            assert!((1..=16_384).contains(&size));
            let loaded = LoadResource(module, resource);
            let pointer = LockResource(loaded).cast::<u8>();
            assert!(!pointer.is_null());
            let manifest = std::str::from_utf8(std::slice::from_raw_parts(pointer, size)).unwrap();
            assert!(manifest.contains("Microsoft.Windows.Common-Controls"));
            assert!(manifest.contains("6.0.0.0"));
        }
    }
    #[test]
    fn only_an_enabled_host_stages_and_explicit_selectors_are_preserved() {
        let bin = Path::new("C:/Zerus/hgs.exe");
        let status = json!({"enabled":true,"version":"1.56.0","cli":{"path":"C:/State/hgs.exe","installed":true,"sha256":"old","version":"1.55.0"}});
        let choice = choose_hgs(&status, bin, "new", None, None);
        assert!(choice.refresh);
        assert_eq!(choice.path, Path::new("C:/State/hgs.exe"));
        let mut older = status.clone();
        older["version"] = json!("1.54.0");
        assert!(!choose_hgs(&older, bin, "new", None, None).refresh);
        older["version"] = json!("1.56.0-dev.9");
        older["cli"]["version"] = json!("1.56.0-dev.10");
        assert!(!choose_hgs(&older, bin, "new", None, None).refresh);
        older["enabled"] = json!(false);
        assert_eq!(choose_hgs(&older, bin, "new", None, None).path, bin);
        let explicit = Path::new("D:/Pinned/hgs.exe");
        let choice = choose_hgs(&status, bin, "new", Some(explicit), None);
        assert!(!choice.refresh);
        assert_eq!(choice.path, explicit);
        let mut absent = status;
        absent["cli"]["installed"] = json!(false);
        assert_eq!(choose_hgs(&absent, bin, "", None, None).fallback, bin);
    }
    #[test]
    fn the_portable_app_stages_again_only_to_bind_itself_and_never_downgrades() {
        let bin = Path::new("C:/Zip/hgs.exe");
        let app = Path::new(r"C:\Zip\hgs-tray.exe");
        let status = json!({"enabled":true,"version":"1.56.0","cli":{"path":"C:/State/hgs.exe","installed":true,"sha256":"same","version":"1.56.0"},
            "frontend":{"path":r"C:\zip\HGS-TRAY.EXE","cli_current":true,"frontend_current":true}});
        // Bound to this app and an unchanged CLI: nothing to do.
        assert!(!choose_hgs(&status, bin, "same", None, Some(app)).refresh);
        assert!(
            !choose_hgs(
                &status,
                bin,
                "same",
                None,
                Some(Path::new(r"\\?\C:\Zip\hgs-tray.exe"))
            )
            .refresh
        );
        // No binding, another app's, or one for a CLI since replaced: rebind.
        for frontend in [
            json!(null),
            json!({"path":r"C:\Other\hgs-tray.exe","cli_current":true,"frontend_current":true}),
            json!({"path":r"C:\Zip\hgs-tray.exe","cli_current":false,"frontend_current":true}),
            // The same path now holds another app (replaced after a quit).
            json!({"path":r"C:\Zip\hgs-tray.exe","cli_current":true,"frontend_current":false}),
            json!({"path":r"C:\Zip\hgs-tray.exe"}),
        ] {
            let mut unbound = status.clone();
            unbound["frontend"] = frontend;
            let choice = choose_hgs(&unbound, bin, "same", None, Some(app));
            assert!(choice.refresh);
            assert_eq!(choice.path, Path::new("C:/State/hgs.exe"));
            // Never from an older zip, and never without an app of our own.
            let mut older = unbound.clone();
            older["version"] = json!("1.55.0");
            assert!(!choose_hgs(&older, bin, "other", None, Some(app)).refresh);
            assert!(!choose_hgs(&unbound, bin, "same", None, None).refresh);
            // An explicit selector still never stages.
            assert!(!choose_hgs(&unbound, bin, "same", Some(bin), Some(app)).refresh);
        }
    }
    #[test]
    fn restart_keeps_selector_and_values_but_never_opens_the_window() {
        let args = [
            "--sessions",
            "--hgs",
            "C:/Program Files/Zerus/hgs.exe",
            "--terminal",
            "--sessions",
        ]
        .map(str::to_owned);
        assert_eq!(
            restart_arguments(&args),
            [
                "--background",
                "--hgs",
                "C:/Program Files/Zerus/hgs.exe",
                "--terminal",
                "--sessions"
            ]
        );
    }
    #[test]
    fn worker_backoff_is_bounded_and_healthy_runs_reset_it() {
        use std::time::Duration;
        assert_eq!(restart_delay(0, Duration::ZERO), 1000);
        assert_eq!(restart_delay(1000, Duration::ZERO), 2000);
        assert_eq!(restart_delay(300_000, Duration::ZERO), 300_000);
        assert_eq!(restart_delay(300_000, Duration::from_secs(60)), 1000);
    }
    #[test]
    fn withdrawn_notices_cannot_route_and_replacement_uses_the_latest_token() {
        let mut clicks = NoticeClicks::default();
        assert_eq!(clicks.clicked(), None);
        clicks.raised("old".into());
        clicks.raised("current".into());
        clicks.cleared(&["old".into()]);
        assert_eq!(clicks.clicked().as_deref(), Some("current"));
        clicks.cleared(&["current".into()]);
        assert_eq!(clicks.clicked(), None);
    }
    #[derive(Default)]
    struct Recorded {
        minimized: bool,
        front: std::cell::Cell<bool>,
        grant: bool,
        calls: std::cell::RefCell<Vec<&'static str>>,
    }
    impl Foreground for Recorded {
        fn minimized(&self) -> bool {
            self.calls.borrow_mut().push("minimized");
            self.minimized
        }
        fn restore(&self) {
            self.calls.borrow_mut().push("restore");
        }
        fn in_front(&self) -> bool {
            self.calls.borrow_mut().push("in_front");
            self.front.get()
        }
        fn take_front(&self) -> bool {
            self.calls.borrow_mut().push("take_front");
            self.front.set(self.grant);
            self.grant
        }
        fn flash(&self) {
            self.calls.borrow_mut().push("flash");
        }
    }
    #[test]
    fn a_refused_foreground_makes_one_attempt_and_only_flashes() {
        let refused = Recorded {
            minimized: true,
            ..Default::default()
        };
        assert_eq!(bring_to_front(&refused), Front::Flashed);
        assert_eq!(
            *refused.calls.borrow(),
            ["minimized", "restore", "in_front", "take_front", "flash"]
        );
        let granted = Recorded {
            grant: true,
            ..Default::default()
        };
        assert_eq!(bring_to_front(&granted), Front::Taken);
        assert_eq!(
            *granted.calls.borrow(),
            ["minimized", "in_front", "take_front", "in_front"]
        );
        let already = Recorded::default();
        already.front.set(true);
        assert_eq!(bring_to_front(&already), Front::Already);
        assert_eq!(*already.calls.borrow(), ["minimized", "in_front"]);
    }
    #[test]
    fn windows_presentation_never_injects_input_or_uses_tao_focus() {
        // The adapter and the Windows presentation path are the only callers.
        let adapter = include_str!("platform/windows/foreground.rs");
        for forbidden in [
            "SendInput",
            "keybd_event",
            "AttachThreadInput",
            "LockSetForegroundWindow",
            "SystemParametersInfo",
            "ASFW_ANY",
            "set_focus",
        ] {
            assert!(!adapter.contains(forbidden), "{forbidden}");
        }
        // Tauri's window focus is a disallowed method: Windows all-target
        // Clippy fails on any call Windows can reach, however it is written
        // (through a macro too); the Unix calls and the perf-only hooks allow
        // it inside their gates.
        let config = include_str!("../clippy.toml");
        for path in [
            "\"tauri::WebviewWindow::set_focus\"",
            "\"tauri::Window::set_focus\"",
        ] {
            assert!(config.contains(path), "{path}");
        }
    }
    #[test]
    fn windows_text_limits_preserve_unicode_and_termination() {
        assert_eq!(wide_text::<4>("a😀z"), [97, 0xd83d, 0xde00, 0]);
        assert_eq!(wide_text::<3>("ab😀"), [97, 98, 0]);
        assert_eq!(wide_text::<3>("a\0b"), [97, 98, 0]);
    }

    #[test]
    fn the_version_heading_never_displaces_bounded_tray_statistics() {
        let body = "4 active, 0 need attention, 55 sessions total\n55 here\n0 on mac\n0 on va-kids";
        let dev = "Zerus 0.48.4-dev.123456789012345";
        assert_eq!(
            versioned_tooltip::<128>(dev, body),
            format!("{dev}\n{body}")
        );
        let preview = format!("Zerus 0.48.4-preview.{}.aaaaaaa", "x".repeat(32));
        assert_eq!(versioned_tooltip::<128>(&preview, body), body);

        assert_eq!(versioned_tooltip::<9>("Zerus", "😀"), "Zerus\n😀");
        assert_eq!(versioned_tooltip::<8>("Zerus", "😀"), "😀");
        assert_eq!(wide_text::<9>(&versioned_tooltip::<9>("Zerus", "😀"))[8], 0);
        let error = "Zerus: reconnecting to hgs on this computer";
        assert_eq!(versioned_tooltip::<8>(dev, error), error);
    }
}
