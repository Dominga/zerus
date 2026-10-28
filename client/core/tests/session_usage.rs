//! The Context chip against `tray/src/SessionUsage.h`. The number tables are
//! the Qt client's own answers (QLocale en_US, Qt 6.12), recorded by a
//! harness running its `compact` and `QLocale::toString` calls.
use serde_json::json;
use zerus_client_core::cache::Tone;
use zerus_client_core::session_usage::{compact, context, fixed, integer};

#[test]
fn counts_read_as_qt_writes_them() {
    for (number, short, long) in [
        (0.0_f64, "0", "0"),
        (7.0_f64, "7", "7"),
        (999.0_f64, "999", "999"),
        (999.9_f64, "999", "999"),
        (1000.0_f64, "1k", "1,000"),
        (1049.0_f64, "1k", "1,049"),
        (1050.0_f64, "1.1k", "1,050"),
        (1150.0_f64, "1.1k", "1,150"),
        (1250.0_f64, "1.3k", "1,250"),
        (1350.0_f64, "1.4k", "1,350"),
        (72345.0_f64, "72.3k", "72,345"),
        (99950.0_f64, "100k", "99,950"),
        (999949.0_f64, "999.9k", "999,949"),
        (999950.0_f64, "1,000k", "999,950"),
        (999999.0_f64, "1,000k", "999,999"),
        (1000000.0_f64, "1M", "1,000,000"),
        (1049999.0_f64, "1M", "1,049,999"),
        (1050000.0_f64, "1.1M", "1,050,000"),
        (1250000.0_f64, "1.3M", "1,250,000"),
        (1999999.0_f64, "2M", "1,999,999"),
        (12345678.0_f64, "12.3M", "12,345,678"),
        (123456789012.0_f64, "123,456.8M", "123,456,789,012"),
    ] {
        assert_eq!(compact(number), short, "compact {number}");
        assert_eq!(integer(number), long, "integer {number}");
    }
}

#[test]
fn shares_round_half_away_from_zero_as_qt_does() {
    for (number, whole, tenth) in [
        (0.0000_f64, "0", "0.0"),
        (0.5000_f64, "1", "0.5"),
        (1.5000_f64, "2", "1.5"),
        (2.5000_f64, "3", "2.5"),
        (49.5000_f64, "50", "49.5"),
        (50.5000_f64, "51", "50.5"),
        (69.9500_f64, "70", "70.0"),
        (70.0000_f64, "70", "70.0"),
        (89.5000_f64, "90", "89.5"),
        (89.9400_f64, "90", "89.9"),
        (90.0000_f64, "90", "90.0"),
        (99.9500_f64, "100", "100.0"),
        (100.0000_f64, "100", "100.0"),
        (123.4500_f64, "123", "123.5"),
    ] {
        assert_eq!(fixed(number, 0), whole, "{number}");
        assert_eq!(fixed(number, 1), tenth, "{number}");
    }
}

#[test]
fn a_known_window_shows_the_share_and_its_tone() {
    let usage = |used: f64| json!({"context": {"used": used, "limit": 200000, "limit_source": "native_statusline", "limit_model": "claude-opus-4"}});
    let chip = context(&usage(85_300.0), false);
    assert!(chip.visible && !chip.warning);
    assert_eq!(chip.text, "43% (85.3k/200k)");
    assert_eq!(chip.tone, Tone::Success);
    assert_eq!(
        chip.tooltip,
        "Latest reported context: 85,300 tokens\nContext window: 200,000 tokens / 42.6% used\nWindow size that Claude Code's status line reports for claude-opus-4.\nOpen session usage in Details."
    );
    assert_eq!(chip.accessible_name, "Context: 43% (85.3k/200k)");
    assert_eq!(context(&usage(140_000.0), false).tone, Tone::Warning);
    assert_eq!(context(&usage(180_000.0), false).tone, Tone::Danger);
    // Recorded values are quiet and say so.
    let recorded = context(&usage(180_000.0), true);
    assert_eq!(recorded.tone, Tone::Quiet);
    assert!(recorded
        .tooltip
        .contains("Last recorded values. Live context is not confirmed."));
}

#[test]
fn an_unknown_window_shows_the_size_with_a_warning() {
    let chip = context(
        &json!({"context": {"used": 1_234_567, "estimated": true, "limit_source": "model_config"}}),
        false,
    );
    assert!(chip.visible && chip.warning);
    assert_eq!(chip.text, "≈1.2M");
    assert_eq!(chip.tone, Tone::Quiet);
    assert_eq!(chip.percent, None);
    assert_eq!(
        chip.tooltip,
        "Latest reported context: 1,234,567 tokens\nThe provider has not reported the window size. Percentage is unavailable.\nApproximate context size reported by the provider.\nWindow size from this model's native configuration.\nOpen session usage in Details."
    );
    assert_eq!(chip.accessible_name, "Context: ≈1.2M (capacity unknown)");
    // A zero window is no window; the status line source needs one.
    let zero = context(
        &json!({"context": {"used": 10, "limit": 0, "limit_source": "native_statusline"}}),
        false,
    );
    assert!(zero.warning && !zero.tooltip.contains("status line"));
}

#[test]
fn without_a_reported_size_the_chip_is_hidden() {
    for usage in [
        json!({}),
        json!({"context": {"used": "12"}}),
        json!({"context": {"used": -1}}),
        json!({"context": {"used": 1e300}}),
    ] {
        let chip = context(&usage, false);
        assert!(!chip.visible, "{usage}");
        assert_eq!(chip.text, "");
        assert_eq!(
            chip.tooltip,
            "The provider has not reported context usage for this conversation yet.\nOpen session usage in Details."
        );
    }
}
