//! Bounded, read-only Git display facts. Missing counts and remote evidence
//! remain unknown; a failed read never preserves a clean indication.
use crate::{Change, ComputerId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
pub(crate) mod subscriptions;

pub const FOLDERS: usize = 64;
/// Leave sixteen of the existing 64 detail demands for ordinary detail tabs.
pub const VISIBLE_FOLDERS: usize = 48;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct RowInterest {
    pub computer: String,
    pub key: String,
}
impl RowInterest {
    pub fn valid(&self) -> bool {
        crate::desktop::service::valid_computer(&self.computer)
            && !self.key.is_empty()
            && self.key.len() <= 1024
            && !self.key.chars().any(char::is_control)
    }
}
pub fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && (crate::paths::absolute(path, crate::paths::Style::Posix)
            || crate::paths::absolute(path, crate::paths::Style::Windows))
}
/// The same preference as Qt's gitStatusPath, using native catalog fields.
pub fn path(row: &crate::ViewRow) -> Option<&str> {
    if row.state == "archived" || row.git_metadata_state == "not_repo" {
        return None;
    }
    let path = [&row.git_root, &row.cwd_canonical, &row.cwd]
        .into_iter()
        .find(|p| !p.is_empty())?;
    valid_path(path).then_some(path)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GitStatus {
    pub state: String,
    pub root: String,
    pub branch: String,
    pub upstream: String,
    pub changed_files: Option<u64>,
    pub conflicts: Option<u64>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub detached: bool,
    pub unborn: bool,
    pub remote_state: String,
    pub remote_age_seconds: Option<f64>,
}
impl GitStatus {
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let text = |key: &str, max: usize| -> Option<String> {
            match object.get(key) {
                None | Some(Value::Null) => Some(String::new()),
                Some(Value::String(s)) if s.len() <= max && !s.contains('\0') => Some(s.clone()),
                _ => None,
            }
        };
        let count = |key: &str| -> Option<Option<u64>> {
            match object.get(key) {
                None | Some(Value::Null) => Some(None),
                Some(n) => n.as_u64().map(Some),
            }
        };
        let boolean = |key: &str| match object.get(key) {
            None | Some(Value::Null) => Some(false),
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        };
        let state = text("state", 64)?;
        if state.is_empty() {
            return None;
        }
        let result = Self {
            state,
            root: text("root", 4096)?,
            branch: text("branch", 1024)?,
            upstream: text("upstream", 1024)?,
            changed_files: count("changed_files")?,
            conflicts: count("conflicts")?,
            ahead: count("ahead")?,
            behind: count("behind")?,
            detached: boolean("detached")?,
            unborn: boolean("unborn")?,
            remote_state: text("remote_state", 64)?,
            remote_age_seconds: object
                .get("remote_age_seconds")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0),
        };
        (result.state != "ok" || (result.changed_files.is_some() && !result.root.is_empty()))
            .then_some(result)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GitUpdate {
    State { value: GitStatus, rev: u64 },
    Unavailable { code: String },
}

#[derive(Debug, Default)]
pub struct GitState {
    follows: BTreeSet<String>,
}
impl GitState {
    pub fn follow(&mut self, path: &str) -> bool {
        if self.follows.len() >= FOLDERS && !self.follows.contains(path) {
            return false;
        }
        self.follows.insert(path.into());
        true
    }
    pub fn unfollow(&mut self, path: &str) {
        self.follows.remove(path);
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.follows.iter()
    }
    pub fn state(
        &self,
        computer: &ComputerId,
        path: &str,
        value: &Value,
        rev: u64,
    ) -> Option<Change> {
        self.follows.contains(path).then(|| Change::Git {
            computer: computer.clone(),
            key: path.into(),
            update: GitStatus::parse(value).map_or_else(
                || GitUpdate::Unavailable {
                    code: "invalid_git_state".into(),
                },
                |value| GitUpdate::State { value, rev },
            ),
        })
    }
    pub fn unavailable(&self, computer: &ComputerId, path: &str, code: &str) -> Option<Change> {
        let code = match code {
            "unsupported_topic" => "unsupported_topic",
            "too_many_subscriptions" => "too_many_subscriptions",
            "bad_request" => "bad_request",
            "git_status_failed" => "git_status_failed",
            "subscription_ended" => "subscription_ended",
            "git_gap" => "git_gap",
            _ => "git_unavailable",
        };
        self.follows.contains(path).then(|| Change::Git {
            computer: computer.clone(),
            key: path.into(),
            update: GitUpdate::Unavailable { code: code.into() },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn invalid_and_missing_facts_never_become_clean_or_unbounded() {
        let valid = json!({"state":"ok","root":"/fixture","changed_files":0,
            "ahead":null,"behind":null,"remote_state":"no_upstream"});
        let parsed = GitStatus::parse(&valid).unwrap();
        assert_eq!(parsed.ahead, None);
        for value in [
            json!({"state":"ok"}),
            json!({"state":"ok","root":"/fixture","changed_files":-1}),
            json!({"state":"unavailable","branch":"x".repeat(1025)}),
            Value::Null,
        ] {
            assert!(GitStatus::parse(&value).is_none());
        }
        let mut state = GitState::default();
        for n in 0..FOLDERS {
            assert!(state.follow(&format!("/fixture/{n}")));
        }
        assert!(!state.follow("/overflow"));
        assert!(state
            .state(&ComputerId::local(), "/unknown", &valid, 1)
            .is_none());
        assert!(matches!(
            state.state(&ComputerId::local(), "/fixture/0", &Value::Null, 1),
            Some(Change::Git {
                update: GitUpdate::Unavailable { .. },
                ..
            })
        ));
        state.unfollow("/fixture/0");
        assert!(state
            .state(&ComputerId::local(), "/fixture/0", &valid, 2)
            .is_none());
    }
    #[test]
    fn paths_use_native_checkout_facts_without_display_or_cross_host_rewrites() {
        let mut row = crate::ViewRow {
            cwd: "C:\\fixture\\project\\src".into(),
            cwd_canonical: "C:\\fixture\\project".into(),
            git_root: "C:\\fixture\\checkout".into(),
            folder: "display only".into(),
            ..Default::default()
        };
        assert_eq!(path(&row), Some("C:\\fixture\\checkout"));
        row.git_metadata_state = "not_repo".into();
        assert_eq!(path(&row), None);
        row.git_metadata_state.clear();
        row.state = "archived".into();
        assert_eq!(path(&row), None);
    }
    #[test]
    fn a_failure_wins_over_clean_and_viewport_churn_keeps_the_batch_bounded() {
        let mut pending = crate::Coalescer::new();
        let state = GitStatus {
            state: "ok".into(),
            root: "/fixture".into(),
            changed_files: Some(0),
            ..Default::default()
        };
        pending.push(Change::Git {
            computer: ComputerId::local(),
            key: "/fixture".into(),
            update: GitUpdate::State {
                value: state,
                rev: 1,
            },
        });
        pending.push(Change::Git {
            computer: ComputerId::local(),
            key: "/fixture".into(),
            update: GitUpdate::Unavailable {
                code: "git_status_failed".into(),
            },
        });
        let batch = pending.take();
        assert_eq!(batch.git.len(), 1);
        assert_eq!(batch.git[0].kind, "unavailable");
        assert!(!batch.is_empty());
        for n in 0..1000 {
            pending.push(Change::Git {
                computer: ComputerId::local(),
                key: format!("/fixture/{n}"),
                update: GitUpdate::Unavailable {
                    code: "git_interest_retired".into(),
                },
            });
            assert!(pending.pending() <= FOLDERS * 2);
        }
        let batch = pending.take();
        assert_eq!(batch.git[0].key, "");
        assert_eq!(batch.git[0].payload["code"], "git_batch_overflow");
    }
}
