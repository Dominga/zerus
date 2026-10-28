//! The composer's Context chip, ported from the Qt client's
//! `SessionUsage::ContextButton`: the latest context size the provider
//! reported, as a share of its window when the window is known.
//!
//! Numbers read as Qt's English locale writes them: digits grouped by
//! commas, and one decimal rounded half away from zero ("1.25k" is "1.3k").
use crate::cache::Tone;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The composer's Context chip; hidden when not `visible`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextChip {
    /// The provider reported a context size.
    pub visible: bool,
    /// "≈42% (85.3k/200k)", or the size alone without a window.
    pub text: String,
    pub tooltip: String,
    /// The window size is unknown: the chip shows a warning icon.
    pub warning: bool,
    /// Quiet without a window or for recorded values, otherwise by share:
    /// success, warning from 70%, danger from 90%.
    pub tone: Tone,
    /// The share of the window in percent, when known.
    pub percent: Option<f64>,
    pub accessible_name: String,
}

/// A count as JSON gives it: a finite number from 0 to 2^53 - 1.
fn valid(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && (0.0..=9_007_199_254_740_991.0).contains(number))
}

/// Digits grouped by commas.
fn group(digits: &str) -> String {
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// A count as Qt's `QLocale::toString(qint64)` writes it, truncated.
pub fn integer(number: f64) -> String {
    group(&(number.max(0.0).trunc() as u64).to_string())
}

/// `number` with `decimals` digits, rounded half away from zero on its
/// exact value, grouped (`QLocale::toString(double, 'f', decimals)`).
pub fn fixed(number: f64, decimals: usize) -> String {
    let negative = number < 0.0;
    // The exact decimal expansion: every finite double has one, and the
    // counts and shares here need far fewer than 80 digits after the point.
    let exact = format!("{:.80}", number.abs());
    let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut digits: Vec<u8> = whole
        .bytes()
        .chain(fraction.bytes().take(decimals))
        .map(|digit| digit - b'0')
        .collect();
    let rest = &fraction.as_bytes()[decimals.min(fraction.len())..];
    if rest.first().is_some_and(|digit| *digit >= b'5') {
        let mut index = digits.len();
        loop {
            if index == 0 {
                digits.insert(0, 1);
                break;
            }
            index -= 1;
            if digits[index] == 9 {
                digits[index] = 0;
            } else {
                digits[index] += 1;
                break;
            }
        }
    }
    let split = digits.len() - decimals;
    let whole: String = digits[..split]
        .iter()
        .map(|digit| char::from(b'0' + digit))
        .collect();
    let whole = whole.trim_start_matches('0');
    let mut text = group(if whole.is_empty() { "0" } else { whole });
    if decimals > 0 {
        text.push('.');
        text.extend(digits[split..].iter().map(|digit| char::from(b'0' + digit)));
    }
    if negative
        && text
            .bytes()
            .any(|byte| byte.is_ascii_digit() && byte != b'0')
    {
        text.insert(0, '-');
    }
    text
}

/// Qt's compact count: "999", "72.3k", "1M", "12.3M".
pub fn compact(number: f64) -> String {
    if number < 1000.0 {
        return integer(number);
    }
    let millions = number >= 1_000_000.0;
    let mut text = fixed(number / if millions { 1_000_000.0 } else { 1000.0 }, 1);
    if text.ends_with('0') {
        text.truncate(text.len() - 2);
    }
    text.push(if millions { 'M' } else { 'k' });
    text
}

/// The Context chip for an inspection's `session_usage` object; `recorded`
/// when the values are the last recorded ones rather than live.
pub fn context(usage: &Value, recorded: bool) -> ContextChip {
    let context = &usage["context"];
    let used = valid(context.get("used"));
    let limit = valid(context.get("limit")).filter(|limit| *limit > 0.0);
    let estimated = context
        .get("estimated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let approximate = if estimated { "≈" } else { "" };
    let percent = used.zip(limit).map(|(used, limit)| 100.0 * used / limit);
    let text = match (used, limit, percent) {
        (Some(used), Some(limit), Some(percent)) => format!(
            "{approximate}{}% ({}/{})",
            fixed(percent, 0),
            compact(used),
            compact(limit)
        ),
        (Some(used), _, _) => format!("{approximate}{}", compact(used)),
        _ => String::new(),
    };
    let mut hint = Vec::new();
    if let Some(used) = used {
        hint.push(format!("Latest reported context: {} tokens", integer(used)));
        match (limit, percent) {
            (Some(limit), Some(percent)) => hint.push(format!(
                "Context window: {} tokens / {}% used",
                integer(limit),
                fixed(percent, 1)
            )),
            _ => hint.push(
                "The provider has not reported the window size. Percentage is unavailable.".into(),
            ),
        }
        if estimated {
            hint.push("Approximate context size reported by the provider.".into());
        }
        let source = context.get("limit_source").and_then(Value::as_str);
        if source == Some("model_config") {
            hint.push("Window size from this model's native configuration.".into());
        }
        if limit.is_some() && source == Some("native_statusline") {
            hint.push(format!(
                "Window size that Claude Code's status line reports for {}.",
                context
                    .get("limit_model")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            ));
        }
        if recorded {
            hint.push("Last recorded values. Live context is not confirmed.".into());
        }
    } else {
        hint.push("The provider has not reported context usage for this conversation yet.".into());
    }
    hint.push("Open session usage in Details.".into());
    let tone = match percent {
        _ if used.is_none() || limit.is_none() || recorded => Tone::Quiet,
        Some(percent) if percent >= 90.0 => Tone::Danger,
        Some(percent) if percent >= 70.0 => Tone::Warning,
        _ => Tone::Success,
    };
    let warning = used.is_some() && limit.is_none();
    ContextChip {
        visible: used.is_some(),
        accessible_name: format!(
            "Context: {text}{}",
            if warning { " (capacity unknown)" } else { "" }
        ),
        text,
        tooltip: hint.join("\n"),
        warning,
        tone,
        percent,
    }
}
