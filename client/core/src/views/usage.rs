//! Account usage as `hgs [@host] account inspect ID --refresh` reports it,
//! and the cached `account_status` of `account ls`: limit windows, balances,
//! credits and the provider's identity (tray/src/AccountUsage.h). A reset
//! arrives as Unix seconds or an ISO 8601 date (`AccountUsage::reset`,
//! AccountUsage.h:33-36); both become seconds here, so the frontend only
//! compares them with its clock.
use super::{flag, items, number, text, whole};
use serde::Serialize;
use serde_json::Value;

/// The most windows and balances one usage keeps.
const WINDOWS: usize = 16;
const BALANCES: usize = 8;

/// One limit window.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Window {
    pub id: String,
    /// The provider's name ("Sonnet", "Monthly"); empty for the plain 5 h and 7 d windows.
    pub label: String,
    /// The used share as reported, normally 0 to 100.
    pub used: f64,
    /// The window length; None when the provider reports none (Kimi's monthly windows).
    pub minutes: Option<u64>,
    /// The reset in Unix seconds; None when unknown or unreadable.
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Identity {
    pub email: String,
    pub name: String,
    pub plan: String,
    pub organization: String,
    pub account_id: String,
    pub auth_method: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Balance {
    pub kind: String,
    pub balance: String,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Credits {
    pub balance: String,
    pub unlimited: bool,
}

/// One account's usage as the screens show it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Usage {
    /// The account profile id, when the source reports one.
    pub id: String,
    /// `ok`, `loading`, `unavailable`, `offline`, `configured`, `signed_out`,
    /// `expired`, `credentials_locked`, `desktop_session_unavailable` or
    /// `credentials_unavailable`.
    pub status: String,
    pub provider: String,
    pub label: String,
    pub auth_status: String,
    pub checked_at: Option<f64>,
    pub identity: Identity,
    pub windows: Vec<Window>,
    pub balances: Vec<Balance>,
    pub credits: Option<Credits>,
    /// An inspect runs; the values shown are the previous ones.
    pub refreshing: bool,
    /// The last inspect failed; the values shown are the previous ones.
    pub refresh_error: bool,
    /// The machine is offline.
    pub offline: bool,
    /// The windows, balances and credits come from the cached `account_status`
    /// of `account ls`; the view shows "Checked HH:MM" until inspect answers.
    pub seeded: bool,
}

const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn digits(text: &str, from: usize, len: usize) -> Option<i64> {
    let part = text.get(from..from + len)?;
    part.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| part.parse().ok())
        .flatten()
}

/// Unix seconds of an ISO 8601 date and time: `YYYY-MM-DDTHH:MM:SS`, an
/// optional fraction, then `Z`, `+HH:MM`, `+HHMM` or nothing. A time without
/// an offset is read as UTC (the providers behind hgs send one; Qt would read
/// it as local time).
pub fn parse_iso(text: &str) -> Option<f64> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (
        digits(text, 0, 4)?,
        digits(text, 5, 2)?,
        digits(text, 8, 2)?,
    );
    let (hour, minute, second) = (
        digits(text, 11, 2)?,
        digits(text, 14, 2)?,
        digits(text, 17, 2)?,
    );
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let last = if month == 2 && leap(year) {
        29
    } else {
        MONTH_DAYS[month as usize - 1]
    };
    if !(1..=last).contains(&day) {
        return None;
    }
    let mut rest = &text[19..];
    let mut fraction = 0.0;
    if let Some(after) = rest.strip_prefix(['.', ',']) {
        let count = after.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 {
            return None;
        }
        fraction = format!("0.{}", &after[..count]).parse().ok()?;
        rest = &after[count..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        zone => {
            let sign = match zone.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let clock = zone[1..].replace(':', "");
            if clock.len() != 4 {
                return None;
            }
            let (hours, minutes) = (digits(&clock, 0, 2)?, digits(&clock, 2, 2)?);
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let seconds =
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    Some(seconds as f64 + fraction)
}

/// `resets_at` as Unix seconds: a number is seconds, a string an ISO 8601 date.
pub fn reset_seconds(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|n| n.is_finite()),
        Value::String(text) => parse_iso(text),
        _ => None,
    }
}

/// One window, or None when its used share is not a number.
pub fn window(value: &Value) -> Option<Window> {
    Some(Window {
        used: number(value, "used_percent")?,
        id: text(value, "id"),
        label: text(value, "label"),
        minutes: whole(value, "window_minutes").filter(|minutes| *minutes > 0),
        resets_at: reset_seconds(value.get("resets_at")),
    })
}

/// The usage of one inspect reply, or of a cached `account_status`.
pub fn usage(value: &Value) -> Usage {
    let identity = value.get("identity").unwrap_or(&Value::Null);
    Usage {
        id: text(value, "id"),
        status: text(value, "status"),
        provider: text(value, "provider"),
        label: text(value, "label"),
        auth_status: text(value, "auth_status"),
        checked_at: number(value, "checked_at"),
        identity: Identity {
            email: text(identity, "email"),
            name: text(identity, "name"),
            plan: text(identity, "plan"),
            organization: text(identity, "organization"),
            account_id: text(identity, "account_id"),
            auth_method: text(identity, "auth_method"),
        },
        windows: items(value, "windows")
            .iter()
            .filter_map(window)
            .take(WINDOWS)
            .collect(),
        balances: items(value, "balances")
            .iter()
            .take(BALANCES)
            .map(|b| Balance {
                kind: text(b, "kind"),
                balance: text(b, "balance"),
                currency: text(b, "currency"),
            })
            .collect(),
        credits: value
            .get("credits")
            .filter(|c| c.as_object().is_some_and(|o| !o.is_empty()))
            .map(|c| Credits {
                balance: text(c, "balance"),
                unlimited: flag(c, "unlimited"),
            }),
        ..Usage::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn iso_and_number_resets_become_seconds() {
        assert_eq!(parse_iso("2026-10-28T17:00:00+00:00"), Some(1793206800.0));
        assert_eq!(parse_iso("2026-10-28T20:00:00+03:00"), Some(1793206800.0));
        assert_eq!(parse_iso("2026-10-28T17:00:00.250Z"), Some(1793206800.25));
        assert_eq!(parse_iso("2026-11-04T09:30:00-0130"), Some(1793790000.0));
        assert_eq!(parse_iso("2024-02-29T00:00:00Z"), Some(1709164800.0));
        assert_eq!(parse_iso("1970-01-01T00:00:00"), Some(0.0));
        for bad in [
            "",
            "tomorrow",
            "2026-02-30T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-10-28T24:00:00Z",
            "2026-10-28T17:00:00+5",
            "2026-10-28T17:00:00.Z",
            "2026-10-28T17:00:00 UTC",
        ] {
            assert_eq!(parse_iso(bad), None, "{bad:?}");
        }
        assert_eq!(reset_seconds(Some(&json!(1793206800))), Some(1793206800.0));
        assert_eq!(
            reset_seconds(Some(&json!("2026-10-28T17:00:00Z"))),
            Some(1793206800.0)
        );
        assert_eq!(reset_seconds(Some(&json!(null))), None);
        assert_eq!(reset_seconds(Some(&json!(true))), None);
        assert_eq!(reset_seconds(None), None);
    }

    // Review Focus 3: ISO, null and unlengthed resets keep their labels, and no NaN reaches the view.
    #[test]
    fn windows_keep_iso_null_and_unlengthed_resets() {
        let reply = json!({"status": "ok", "provider": "kimi", "checked_at": 1793000000, "windows": [
        {"id": "limit_5h", "label": "", "used_percent": 34.5, "window_minutes": 300, "resets_at": "2026-10-28T17:00:00Z"},
        {"id": "limit_7d", "label": "", "used_percent": 12, "window_minutes": 10080.0, "resets_at": null},
        {"id": "limit_month_total", "label": "Monthly", "used_percent": 50, "window_minutes": null, "resets_at": 1793206800},
        {"id": "broken", "used_percent": "lots"}],
        "balances": [{"kind": "Topped up", "balance": "18.40", "currency": "USD"}], "credits": {"balance": "3", "unlimited": false}});
        let u = usage(&reply);
        assert_eq!(
            u.windows.len(),
            3,
            "a window without a numeric share is left out"
        );
        assert_eq!(
            u.windows[0],
            Window {
                id: "limit_5h".into(),
                label: String::new(),
                used: 34.5,
                minutes: Some(300),
                resets_at: Some(1793206800.0)
            }
        );
        assert_eq!(
            (u.windows[1].minutes, u.windows[1].resets_at),
            (Some(10080), None)
        );
        assert_eq!(
            (
                u.windows[2].label.as_str(),
                u.windows[2].minutes,
                u.windows[2].resets_at
            ),
            ("Monthly", None, Some(1793206800.0))
        );
        assert!(u.windows.iter().all(|w| w.used.is_finite()));
        assert_eq!(
            u.balances,
            vec![Balance {
                kind: "Topped up".into(),
                balance: "18.40".into(),
                currency: "USD".into()
            }]
        );
        assert_eq!(
            u.credits,
            Some(Credits {
                balance: "3".into(),
                unlimited: false
            })
        );
        assert_eq!(u.checked_at, Some(1793000000.0));
        let value = serde_json::to_value(&u).unwrap();
        assert_eq!(value["windows"][1]["resets_at"], json!(null));
        assert_eq!(value["windows"][2]["minutes"], json!(null));
        assert_eq!(usage(&json!({"status": "ok", "credits": {}})).credits, None);
    }
}
