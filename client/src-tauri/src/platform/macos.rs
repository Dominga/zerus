//! macOS: render our own UI at the display refresh rate.
//!
//! WKWebView keeps page rendering updates near 60 fps on ProMotion displays through
//! the internal WebKit preference `PreferPageRenderingUpdatesNear60FPSEnabled`
//! (on by default; WebKit bug 294338). There is no public API. The owner chose
//! (2026-10-27) to disable it through the WebKit SPI before each UI webview is
//! created: `+[WKPreferences _features]` lists the features, and
//! `-[WKPreferences _setEnabled:forFeature:]` sets one (macOS 13.3+; the Zerus floor
//! is 14). Toggling a live page is unreliable, so the configuration is prepared
//! here and passed to `WebviewWindowBuilder::with_webview_configuration`.
//!
//! Every selector and the key are feature-detected, Objective-C exceptions and Rust
//! panics are caught, failures are logged and returned, and the webview is created
//! either way. Third-party pages keep WebKit's default configuration.
//!
//! `display_link_hz` is the native control of the measurement (criterion 3): it
//! counts CVDisplayLink callbacks for the display of the window under test.

use std::ffi::c_void;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{msg_send, sel, ClassType, MainThreadMarker};
use objc2_foundation::{NSArray, NSProcessInfo, NSString};
use objc2_web_kit::{WKPreferences, WKWebViewConfiguration};

/// The WebKit feature key that keeps rendering updates near 60 fps.
pub const FEATURE_KEY: &str = "PreferPageRenderingUpdatesNear60FPSEnabled";

/// Why the preference could not be changed or read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpiError {
    /// This WebKit does not answer a selector the SPI needs.
    MissingSelector(&'static str),
    /// `_features` answered, but none of its features has the key.
    FeatureNotFound { key: String, features: usize },
    /// An Objective-C exception was raised (its description, when there was one).
    Exception(String),
    /// A Rust panic in the bindings (for example a type encoding mismatch).
    Panicked,
    /// The value read back after setting it is not the one set.
    NotApplied { read_back: Option<bool> },
}

impl fmt::Display for SpiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpiError::MissingSelector(name) => write!(f, "WebKit does not respond to {name}"),
            SpiError::FeatureNotFound { key, features } => {
                write!(f, "{key} is not among the {features} WebKit features")
            }
            SpiError::Exception(text) => write!(f, "Objective-C exception: {text}"),
            SpiError::Panicked => write!(f, "panic in the WebKit bindings"),
            SpiError::NotApplied { read_back } => write!(
                f,
                "{FEATURE_KEY} reads back as {read_back:?} after setting it"
            ),
        }
    }
}

impl std::error::Error for SpiError {}

/// What happened to the preference in one configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferenceOutcome {
    /// WebKit's default for the feature (`defaultValue`), when it answers.
    pub default_value: Option<bool>,
    /// The value in the new configuration before it was changed.
    pub before: Option<bool>,
    /// The value read back after setting it.
    pub after: Option<bool>,
    pub error: Option<SpiError>,
}

impl PreferenceOutcome {
    /// True when the configuration renders at the display rate as far as WebKit says.
    #[cfg_attr(
        target_os = "macos",
        allow(
            dead_code,
            reason = "used by client/perf-probe-mac, which includes this file by path"
        )
    )]
    pub fn applied(&self) -> bool {
        self.error.is_none() && self.after == Some(false)
    }
}

// Read-back of the last configuration made by `webview_configuration`:
// 0 unknown or failed, 1 disabled (display rate), 2 still enabled (near 60 fps).
static LAST_STATE: AtomicU8 = AtomicU8::new(0);

/// Runs `body` with Objective-C exceptions and Rust panics turned into errors.
fn guarded<R>(body: impl FnOnce() -> Result<R, SpiError>) -> Result<R, SpiError> {
    match catch_unwind(AssertUnwindSafe(|| {
        objc2::exception::catch(AssertUnwindSafe(body))
    })) {
        Ok(Ok(result)) => result,
        Ok(Err(exception)) => Err(SpiError::Exception(
            exception
                .map(|exception| format!("{exception:?}"))
                .unwrap_or_else(|| "nil".into()),
        )),
        Err(_) => Err(SpiError::Panicked),
    }
}

fn responds(object: &AnyObject, selector: Sel) -> bool {
    // SAFETY: respondsToSelector: is defined on every NSObject, class objects included.
    let answer: Bool = unsafe { msg_send![object, respondsToSelector: selector] };
    answer.as_bool()
}

fn class_object(class: &AnyClass) -> &AnyObject {
    // SAFETY: a class is an Objective-C object; messages to it reach class methods.
    unsafe { &*(class as *const AnyClass as *const AnyObject) }
}

/// The `_WKFeature` objects of `+[WKPreferences _features]`.
pub fn features() -> Result<Vec<Retained<AnyObject>>, SpiError> {
    guarded(|| {
        let class = class_object(WKPreferences::class());
        if !responds(class, sel!(_features)) {
            return Err(SpiError::MissingSelector("+[WKPreferences _features]"));
        }
        // SAFETY: _features takes no arguments and returns an NSArray of _WKFeature.
        let list: Option<Retained<NSArray<AnyObject>>> = unsafe { msg_send![class, _features] };
        Ok(list.map(|list| list.to_vec()).unwrap_or_default())
    })
}

/// The feature's key (`-[_WKFeature key]`).
pub fn feature_key(feature: &AnyObject) -> Option<String> {
    guarded(|| {
        if !responds(feature, sel!(key)) {
            return Ok(None);
        }
        // SAFETY: key is an NSString property of _WKFeature.
        let value: Option<Retained<NSString>> = unsafe { msg_send![feature, key] };
        Ok(value.map(|value| value.to_string()))
    })
    .ok()
    .flatten()
}

/// The feature's display name (`-[_WKFeature name]`).
#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "used by client/perf-probe-mac, which includes this file by path"
    )
)]
pub fn feature_name(feature: &AnyObject) -> Option<String> {
    guarded(|| {
        if !responds(feature, sel!(name)) {
            return Ok(None);
        }
        // SAFETY: name is an NSString property of _WKFeature.
        let value: Option<Retained<NSString>> = unsafe { msg_send![feature, name] };
        Ok(value.map(|value| value.to_string()))
    })
    .ok()
    .flatten()
}

/// The feature's default value (`-[_WKFeature defaultValue]`).
pub fn feature_default(feature: &AnyObject) -> Option<bool> {
    guarded(|| {
        if !responds(feature, sel!(defaultValue)) {
            return Ok(None);
        }
        // SAFETY: defaultValue is a BOOL property of _WKFeature.
        let value: Bool = unsafe { msg_send![feature, defaultValue] };
        Ok(Some(value.as_bool()))
    })
    .ok()
    .flatten()
}

/// The feature with `key`, and how many features WebKit lists.
pub fn find_feature(key: &str) -> Result<(Retained<AnyObject>, usize), SpiError> {
    let list = features()?;
    let count = list.len();
    guarded(|| {
        list.into_iter()
            .find(|feature| feature_key(feature).as_deref() == Some(key))
            .map(|feature| (feature, count))
            .ok_or_else(|| SpiError::FeatureNotFound {
                key: key.to_string(),
                features: count,
            })
    })
}

/// Keys of WebKit features whose key contains any of `fragments` (diagnostics:
/// shows a renamed key).
#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "used by client/perf-probe-mac, which includes this file by path"
    )
)]
pub fn feature_keys_matching(fragments: &[&str]) -> Result<Vec<String>, SpiError> {
    let list = features()?;
    guarded(|| {
        Ok(list
            .iter()
            .filter_map(|feature| feature_key(feature))
            .filter(|key| fragments.iter().any(|fragment| key.contains(fragment)))
            .collect())
    })
}

/// `-[WKPreferences _isEnabledForFeature:]` for `feature`.
pub fn is_enabled(preferences: &WKPreferences, feature: &AnyObject) -> Result<bool, SpiError> {
    guarded(|| {
        if !responds(preferences, sel!(_isEnabledForFeature:)) {
            return Err(SpiError::MissingSelector(
                "-[WKPreferences _isEnabledForFeature:]",
            ));
        }
        // SAFETY: _isEnabledForFeature: takes a _WKFeature and returns BOOL.
        let value: Bool = unsafe { msg_send![preferences, _isEnabledForFeature: feature] };
        Ok(value.as_bool())
    })
}

/// `-[WKPreferences _setEnabled:forFeature:]` for `feature`.
pub fn set_enabled(
    preferences: &WKPreferences,
    feature: &AnyObject,
    enabled: bool,
) -> Result<(), SpiError> {
    guarded(|| {
        if !responds(preferences, sel!(_setEnabled:forFeature:)) {
            return Err(SpiError::MissingSelector(
                "-[WKPreferences _setEnabled:forFeature:]",
            ));
        }
        // SAFETY: _setEnabled:forFeature: takes a BOOL and a _WKFeature and returns void.
        let _: () =
            unsafe { msg_send![preferences, _setEnabled: Bool::new(enabled), forFeature: feature] };
        Ok(())
    })
}

/// Sets the near-60 fps preference of `preferences` to `enabled` and reads it back.
pub fn set_near_60fps(preferences: &WKPreferences, enabled: bool) -> PreferenceOutcome {
    let (feature, _) = match find_feature(FEATURE_KEY) {
        Ok(found) => found,
        Err(error) => {
            return PreferenceOutcome {
                default_value: None,
                before: None,
                after: None,
                error: Some(error),
            }
        }
    };
    let default_value = feature_default(&feature);
    let before = is_enabled(preferences, &feature).ok();
    let error = set_enabled(preferences, &feature, enabled).err();
    let after = is_enabled(preferences, &feature).ok();
    let error = error
        .or_else(|| (after != Some(enabled)).then_some(SpiError::NotApplied { read_back: after }));
    PreferenceOutcome {
        default_value,
        before,
        after,
        error,
    }
}

/// The near-60 fps preference of `preferences` (for example a live webview's
/// `configuration.preferences`), or None when the SPI or the key is missing.
#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "used by client/perf-probe-mac, which includes this file by path"
    )
)]
pub fn preference_state_of(preferences: &WKPreferences) -> Option<bool> {
    let (feature, _) = find_feature(FEATURE_KEY).ok()?;
    is_enabled(preferences, &feature).ok()
}

/// The near-60 fps preference read back from the last configuration that
/// `webview_configuration` made: Some(false) renders at the display rate,
/// Some(true) stays near 60 fps, None when the SPI failed or none was made.
#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "used by client/perf-probe-mac, which includes this file by path"
    )
)]
pub fn preference_state() -> Option<bool> {
    match LAST_STATE.load(Ordering::Relaxed) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

/// A webview configuration for our own UI with the near-60 fps preference
/// disabled when WebKit allows it. A failure is logged and the default
/// configuration is returned: the window opens either way.
pub fn webview_configuration(mtm: MainThreadMarker) -> Retained<WKWebViewConfiguration> {
    configure(mtm, false).0
}

/// As `webview_configuration`, with the preference set to `near_60fps` (true is
/// the must-fail control run of the measurement), and what happened.
pub fn configure(
    mtm: MainThreadMarker,
    near_60fps: bool,
) -> (Retained<WKWebViewConfiguration>, PreferenceOutcome) {
    // SAFETY: called on the main thread, as the marker proves.
    let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    // SAFETY: preferences is a non-null property of every configuration.
    let preferences = unsafe { configuration.preferences() };
    let outcome = set_near_60fps(&preferences, near_60fps);
    let state = match outcome.after {
        Some(false) if outcome.error.is_none() => 1,
        Some(true) if outcome.error.is_none() => 2,
        _ => 0,
    };
    LAST_STATE.store(state, Ordering::Relaxed);
    if let Some(error) = &outcome.error {
        eprintln!("zerus-client: WebKit display-rate preference not applied: {error}");
    }
    (configuration, outcome)
}

/// "Version 27.0.1 (Build 26A434)" from NSProcessInfo.
#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "used by client/perf-probe-mac, which includes this file by path"
    )
)]
pub fn macos_version() -> String {
    NSProcessInfo::processInfo()
        .operatingSystemVersionString()
        .to_string()
}

/// `NSScreen.mainScreen.maximumFramesPerSecond` (macOS 12+): the display rate
/// the measurement's T is computed from. Needs a window server connection.
pub fn screen_max_fps(_mtm: MainThreadMarker) -> Option<i64> {
    guarded(|| {
        let Some(class) = AnyClass::get(c"NSScreen") else {
            return Ok(None);
        };
        let class = class_object(class);
        // SAFETY: mainScreen is a class property returning NSScreen or nil.
        let screen: Option<Retained<AnyObject>> = unsafe { msg_send![class, mainScreen] };
        let Some(screen) = screen else {
            return Ok(None);
        };
        if !responds(&screen, sel!(maximumFramesPerSecond)) {
            return Ok(None);
        }
        // SAFETY: maximumFramesPerSecond is an NSInteger property.
        let fps: isize = unsafe { msg_send![&*screen, maximumFramesPerSecond] };
        Ok(Some(fps as i64))
    })
    .ok()
    .flatten()
}

/// The CGDirectDisplayID of the screen a window (an NSWindow) is on.
#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "used by client/perf-probe-mac, which includes this file by path"
    )
)]
pub fn window_display_id(window: &AnyObject) -> Option<u32> {
    guarded(|| {
        // SAFETY: screen returns the window's NSScreen or nil.
        let screen: Option<Retained<AnyObject>> = unsafe { msg_send![window, screen] };
        let Some(screen) = screen else {
            return Ok(None);
        };
        // SAFETY: deviceDescription is an NSDictionary; NSScreenNumber holds an NSNumber.
        let description: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*screen, deviceDescription] };
        let Some(description) = description else {
            return Ok(None);
        };
        let key = NSString::from_str("NSScreenNumber");
        let number: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*description, objectForKey: &*key] };
        let Some(number) = number else {
            return Ok(None);
        };
        let id: u32 = unsafe { msg_send![&*number, unsignedIntValue] };
        Ok(Some(id))
    })
    .ok()
    .flatten()
}

type CVDisplayLinkRef = *mut c_void;
type CVReturn = i32;
type OutputCallback = extern "C" fn(
    CVDisplayLinkRef,
    *const c_void,
    *const c_void,
    u64,
    *mut u64,
    *mut c_void,
) -> CVReturn;

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVDisplayLinkCreateWithCGDisplay(display: u32, link: *mut CVDisplayLinkRef) -> CVReturn;
    fn CVDisplayLinkSetOutputCallback(
        link: CVDisplayLinkRef,
        callback: OutputCallback,
        context: *mut c_void,
    ) -> CVReturn;
    fn CVDisplayLinkStart(link: CVDisplayLinkRef) -> CVReturn;
    fn CVDisplayLinkStop(link: CVDisplayLinkRef) -> CVReturn;
    fn CVDisplayLinkRelease(link: CVDisplayLinkRef);
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
}

extern "C" fn count_frame(
    _link: CVDisplayLinkRef,
    _now: *const c_void,
    _output: *const c_void,
    _flags: u64,
    _flags_out: *mut u64,
    context: *mut c_void,
) -> CVReturn {
    // SAFETY: context is the AtomicU64 that display_link_hz keeps alive until the link is released.
    let counter = unsafe { &*(context as *const AtomicU64) };
    counter.fetch_add(1, Ordering::Relaxed);
    0
}

/// Display-link callbacks per second on `display` (the main display when None),
/// counted for `duration` after a 200 ms warm-up. Blocks the calling thread; the
/// callbacks run on CoreVideo's own thread, so any thread may call it. Needs a
/// display; None when CoreVideo refuses.
#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "the display-link control of the client/bench/README.md procedure"
    )
)]
pub fn display_link_hz(display: Option<u32>, duration: Duration) -> Option<f64> {
    // SAFETY: plain CoreVideo calls; the counter outlives the link (stopped and released below).
    unsafe {
        let display = display.unwrap_or_else(|| CGMainDisplayID());
        let mut link: CVDisplayLinkRef = std::ptr::null_mut();
        if CVDisplayLinkCreateWithCGDisplay(display, &mut link) != 0 || link.is_null() {
            return None;
        }
        let counter = Arc::new(AtomicU64::new(0));
        let context = Arc::as_ptr(&counter) as *mut c_void;
        if CVDisplayLinkSetOutputCallback(link, count_frame, context) != 0
            || CVDisplayLinkStart(link) != 0
        {
            CVDisplayLinkRelease(link);
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
        let start_count = counter.load(Ordering::Relaxed);
        let start = Instant::now();
        std::thread::sleep(duration);
        let frames = counter.load(Ordering::Relaxed) - start_count;
        let elapsed = start.elapsed().as_secs_f64();
        CVDisplayLinkStop(link);
        CVDisplayLinkRelease(link);
        // A callback already running when the link stopped may still read the counter.
        std::thread::sleep(Duration::from_millis(50));
        drop(counter);
        (elapsed > 0.0).then(|| frames as f64 / elapsed)
    }
}
