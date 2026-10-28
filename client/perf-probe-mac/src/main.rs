//! Window-less probe of the macOS display-rate preference (Task 7).
//!
//! Prints the macOS version, whether WebKit lists the feature, its default, the
//! value in a new configuration before and after `platform::macos` sets it, the
//! read-back through `preference_state()`, and the own process metrics. Opens no
//! window. `--display` also reads the main screen's maximum frame rate and counts
//! display-link callbacks three times for 2 s (needs the console session: run
//! the probe as an app bundle with `open`, see client/bench/README.md).
//! `--json FILE` also writes the figures as a zerus-perf-v1 result.

#[cfg(target_os = "macos")]
#[path = "../../src-tauri/src/platform/macos.rs"]
#[allow(dead_code)]
mod macos;

#[path = "../../src-tauri/src/metrics.rs"]
#[allow(dead_code)]
mod metrics;

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This probe runs on macOS only.");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use serde_json::json;
    use std::time::Duration;

    let args: Vec<String> = std::env::args().collect();
    let json_path = args.iter().position(|arg| arg == "--json").and_then(|i| args.get(i + 1)).cloned();
    let display = args.iter().any(|arg| arg == "--display");

    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("not on the main thread");
        std::process::exit(1);
    };
    let version = macos::macos_version();
    println!("macOS: {version}");

    let feature_count = macos::features().map(|list| list.len());
    match &feature_count {
        Ok(count) => println!("WKPreferences _features: {count} features"),
        Err(error) => println!("WKPreferences _features: unavailable ({error})"),
    }
    let related = macos::feature_keys_matching(&["60FPS", "FrameRate", "RenderingUpdate"]).unwrap_or_default();
    println!("related keys: {}", if related.is_empty() { "none".into() } else { related.join(", ") });
    let found = macos::find_feature(macos::FEATURE_KEY);
    let (feature_name, feature_default) = match &found {
        Ok((feature, count)) => {
            let name = macos::feature_name(feature);
            let default = macos::feature_default(feature);
            println!("feature {}: found among {count}, name {name:?}, default {default:?}", macos::FEATURE_KEY);
            (name, default)
        }
        Err(error) => {
            println!("feature {}: not found ({error})", macos::FEATURE_KEY);
            (None, None)
        }
    };
    match macos::find_feature("ZerusProbeMissingFeatureEnabled") {
        Ok(_) => println!("missing-key check: unexpectedly found"),
        Err(error) => println!("missing-key check: {error}"),
    }

    // The must-fail control keeps WebKit's behaviour: the preference stays enabled.
    let (_control, control) = macos::configure(mtm, true);
    println!(
        "control configuration (near 60 fps kept): before {:?}, after {:?}, error {:?}",
        control.before,
        control.after,
        control.error.as_ref().map(ToString::to_string)
    );
    println!("preference_state() after the control: {:?}", macos::preference_state());

    let (configuration, outcome) = macos::configure(mtm, false);
    println!(
        "display-rate configuration: before {:?}, after {:?}, applied {}, error {:?}",
        outcome.before,
        outcome.after,
        outcome.applied(),
        outcome.error.as_ref().map(ToString::to_string)
    );
    println!("preference_state(): {:?}", macos::preference_state());
    // SAFETY: preferences is a non-null property of every configuration.
    let preferences = unsafe { configuration.preferences() };
    println!("preference_state_of(configuration.preferences): {:?}", macos::preference_state_of(&preferences));

    // A new configuration starts from WebKit's default again: the change is per configuration.
    let fresh = macos::webview_configuration(mtm);
    // SAFETY: as above.
    let fresh_value = macos::preference_state_of(&*unsafe { fresh.preferences() });
    println!("webview_configuration() read-back: {fresh_value:?}");
    // SAFETY: a configuration made by WKWebViewConfiguration::new on the main thread.
    let untouched = unsafe { objc2_web_kit::WKWebViewConfiguration::new(mtm) };
    // SAFETY: as above.
    let untouched_value = macos::preference_state_of(&*unsafe { untouched.preferences() });
    println!("untouched configuration: {untouched_value:?}");

    let pid = std::process::id();
    let own = metrics::process_tree_metrics_over(pid, Duration::from_millis(200));
    println!("metrics: {own}");
    println!("responsible-process lookup available: {}", metrics::macos::responsible_lookup_available());

    let mut screen_fps = None;
    let mut link_rates = Vec::new();
    if display {
        screen_fps = macos::screen_max_fps(mtm);
        println!("NSScreen maximumFramesPerSecond: {screen_fps:?}");
        for _ in 0..3 {
            let hz = macos::display_link_hz(None, Duration::from_secs(2));
            println!("display link: {hz:?} Hz");
            link_rates.push(hz);
        }
    }

    if let Some(path) = json_path {
        let result = json!({
            "schema": "zerus-perf-v1",
            "client": "probe",
            "harness": "client/perf-probe-mac",
            "created_at_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            "platform": { "os": "macos", "os_version": version },
            "webkit": {
                "features": feature_count.as_ref().ok(),
                "key": macos::FEATURE_KEY,
                "found": found.is_ok(),
                "name": feature_name,
                "default": feature_default,
                "related_keys": related,
                "control": { "before": control.before, "after": control.after, "error": control.error.as_ref().map(ToString::to_string) },
                "disabled": { "before": outcome.before, "after": outcome.after, "applied": outcome.applied(),
                              "error": outcome.error.as_ref().map(ToString::to_string) },
                "webview_configuration_read_back": fresh_value,
                "untouched": untouched_value,
            },
            "display": {
                "measured": display,
                "screen_max_fps": screen_fps,
                "display_link_hz": link_rates,
                "display_link_window_s": 2,
            },
        });
        if let Err(error) = std::fs::write(&path, format!("{result:#}\n")) {
            eprintln!("cannot write {path}: {error}");
            std::process::exit(1);
        }
    }
}
