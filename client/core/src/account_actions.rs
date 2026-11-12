//! Account catalog edits from AccountsPage: exact profile, machine and
//! catalog revision, one dispatch. Calls block and belong off the UI thread.
use crate::actions::Invocation;
use crate::{launch, reads, ComputerId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, time::Duration};

pub const CHANGED: &str = "Accounts changed on this machine. Refresh before saving.";
pub const UPDATING: &str = "Accounts are updating. Wait for the update to finish.";
pub const MAX_MEMBERS: usize = 256;
pub mod models;
pub mod permissions;
pub mod setup;
pub mod transfer;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Rename,
    Default,
    Remove,
    Permissions,
    Model,
    /// Share the permission mode across the account's computers, or keep it
    /// per computer (zerus-f0uz8): `mode` is `account` or `computer`.
    Scope,
    /// The account's color and emoji, kept on each of its computers
    /// (zerus-sa9qn.6): `color` and `emoji`, empty to clear.
    Mark,
}

#[derive(Clone, Debug, Default)]
pub struct Values {
    pub label: String,
    pub mode: String,
    pub model: String,
    pub effort: String,
    /// For a shared permission change: the scope the member must still have
    /// (`account`), checked by its hgs under the catalog lock (zerus-f0uz8).
    pub expect_scope: String,
    /// A mark's `#rrggbb` color and emoji; empty clears them.
    pub color: String,
    pub emoji: String,
    /// Set only what the computer has no value for: a desktop moving its own
    /// marks to the computer never replaces the computer's (zerus-sa9qn.6).
    pub if_unset: bool,
}

/// A mark color: empty or `#rrggbb`.
pub fn mark_color(value: &str) -> bool {
    value.is_empty()
        || (value.len() == 7
            && value.starts_with('#')
            && value[1..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// A mark emoji as hgs keeps it: empty, or short, not plain ASCII and
/// without control or space characters. The webview checks for one emoji.
pub fn mark_emoji(value: &str) -> bool {
    value.is_empty()
        || (value.encode_utf16().count() <= 32
            && !value.is_ascii()
            && !value.chars().any(|c| c.is_control() || c.is_whitespace()))
}

/// Native-only evidence captured when the action opens, never read from a
/// webview-supplied path or an inferred provider identity.
#[derive(Clone, Debug)]
pub struct Binding {
    pub computer: ComputerId,
    pub machine_id: String,
    pub id: String,
    pub provider: String,
    pub home: String,
    pub auth_revision: String,
    pub revision: String,
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn profile<'a>(catalog: &'a Value, id: &str) -> Result<&'a Value, String> {
    let rows = catalog
        .get("profiles")
        .and_then(Value::as_array)
        .ok_or(CHANGED)?;
    if rows.len() > MAX_MEMBERS {
        return Err(CHANGED.into());
    }
    let mut found = rows.iter().filter(|row| text(row, "id") == id);
    let row = found.next().ok_or(CHANGED)?;
    if found.next().is_some() {
        return Err(CHANGED.into());
    }
    Ok(row)
}

impl Binding {
    pub fn capture(
        computer: ComputerId,
        machine_id: String,
        catalog: &Value,
        id: &str,
    ) -> Result<Self, String> {
        if (!computer.is_local() && !crate::fleet::valid_alias(&computer.0))
            || !crate::creation::canonical_uuid(&machine_id)
            || id.is_empty()
            || id.starts_with('-')
        {
            return Err(CHANGED.into());
        }
        let row = profile(catalog, id)?;
        let revision = text(catalog, "revision");
        if revision.is_empty() || revision.len() > 256 || text(row, "provider").is_empty() {
            return Err(CHANGED.into());
        }
        Ok(Self {
            computer,
            machine_id,
            id: id.into(),
            provider: text(row, "provider").into(),
            home: text(row, "home").into(),
            auth_revision: text(row, "auth_revision").into(),
            revision: revision.into(),
        })
    }
    pub fn verify(&self, catalog: &Value) -> Result<(), String> {
        let row = profile(catalog, &self.id)?;
        if text(catalog, "revision") != self.revision
            || text(row, "provider") != self.provider
            || text(row, "home") != self.home
            || text(row, "auth_revision") != self.auth_revision
        {
            return Err(CHANGED.into());
        }
        Ok(())
    }
    fn invocation(&self, args: &[&str]) -> Invocation {
        let mut call = Vec::new();
        if !self.computer.is_local() {
            call.push(format!("@{}", self.computer.0));
        }
        call.extend([
            "--expect-machine".into(),
            self.machine_id.clone(),
            "account".into(),
        ]);
        call.extend(args.iter().map(|s| (*s).into()));
        Invocation {
            args: call,
            stdin: None,
            timeout: Duration::from_secs(25),
            request_id: None,
        }
    }
    pub fn edit(&self, kind: Kind, label: &str) -> Result<Invocation, String> {
        self.edit_values(
            kind,
            &Values {
                label: label.into(),
                ..Default::default()
            },
        )
    }
    pub fn edit_values(&self, kind: Kind, values: &Values) -> Result<Invocation, String> {
        let op = match kind {
            Kind::Rename => "rename",
            Kind::Default => "default",
            Kind::Remove => "rm",
            Kind::Permissions => "permissions",
            Kind::Model => "model-defaults",
            Kind::Scope => "permission-scope",
            Kind::Mark => "mark",
        };
        if kind == Kind::Default && matches!(self.provider.as_str(), "dsh" | "opencode") {
            return Err("This account has no default selection.".into());
        }
        let mut call = self.invocation(&[op, &self.id, "--revision", &self.revision]);
        if kind == Kind::Rename {
            let label = values.label.trim();
            if label.is_empty()
                || label.chars().count() > 160
                || label.chars().any(char::is_control)
            {
                return Err("Account name is required (up to 160 characters).".into());
            }
            call.args.extend(["--label".into(), label.into()]);
        }
        if kind == Kind::Permissions {
            // Task 68: a native selection too, as its compact `native:` map;
            // the captured catalog checks its controls before this is built.
            // `default` returns to the Zerus default (T-6hcfnu).
            if self.provider == "opencode"
                || !(["provider", "bypass", "default"].contains(&values.mode.as_str())
                    || permissions::decode_native(&values.mode).is_some_and(|map| !map.is_empty()))
            {
                return Err("Choose provider or bypass permission mode.".into());
            }
            call.args.extend(["--mode".into(), values.mode.clone()]);
            if !values.expect_scope.is_empty() {
                call.args
                    .extend(["--expect-scope".into(), values.expect_scope.clone()]);
            }
        }
        if kind == Kind::Scope {
            if self.provider == "opencode"
                || !["account", "computer"].contains(&values.mode.as_str())
            {
                return Err("Choose shared or per-computer permissions.".into());
            }
            call.args.extend(["--scope".into(), values.mode.clone()]);
        }
        if kind == Kind::Mark {
            let color = values.color.trim().to_ascii_lowercase();
            let emoji = values.emoji.trim();
            if !mark_color(&color) || !mark_emoji(emoji) {
                return Err("Choose a color as #RRGGBB and one emoji.".into());
            }
            let or_none = |value: &str| {
                if value.is_empty() {
                    "none".to_owned()
                } else {
                    value.to_owned()
                }
            };
            call.args.extend([
                "--color".into(),
                or_none(&color),
                "--emoji".into(),
                or_none(emoji),
            ]);
            if values.if_unset {
                call.args.push("--if-unset".into());
            }
        }
        if kind == Kind::Model {
            if !["codex", "claude", "kimi", "dsh"].contains(&self.provider.as_str())
                || values.model.chars().count() > 256
                || values.effort.chars().count() > 40
                || values
                    .model
                    .chars()
                    .chain(values.effort.chars())
                    .any(char::is_control)
            {
                return Err("Model defaults are unavailable for this account.".into());
            }
            call.args.extend([
                "--model".into(),
                values.model.clone(),
                "--effort".into(),
                values.effort.clone(),
            ]);
        }
        Ok(call)
    }
    /// Fresh read before the revision CAS. No uncertain exit is retried.
    pub fn execute(&self, hgs: &Path, kind: Kind, label: &str) -> Result<Value, String> {
        self.execute_values(
            hgs,
            kind,
            &Values {
                label: label.into(),
                ..Default::default()
            },
        )
    }
    pub fn fresh(&self, hgs: &Path) -> Result<Value, String> {
        let read = self.invocation(&["ls"]);
        let current = run_catalog(hgs, &read)?;
        self.verify(&current)?;
        Ok(current)
    }
    pub fn execute_values(&self, hgs: &Path, kind: Kind, values: &Values) -> Result<Value, String> {
        let edit = self.edit_values(kind, values)?;
        self.fresh(hgs)?;
        run_catalog(hgs, &edit)
    }
}

fn run_catalog(hgs: &Path, call: &Invocation) -> Result<Value, String> {
    let exit = crate::run::run_limited(hgs, call, &[], reads::READ_LIMIT)
        .map_err(|_| launch::INVALID_ACCOUNTS.to_owned())?;
    launch::accounts_reply(hgs, exit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn catalog() -> Value {
        json!({"revision":"r1","profiles":[{"id":"work","provider":"codex","home":"/accounts/example","auth_revision":"a1"}]})
    }
    fn binding() -> Binding {
        Binding::capture(
            ComputerId("studio".into()),
            "11111111-1111-4111-8111-111111111111".into(),
            &catalog(),
            "work",
        )
        .unwrap()
    }
    #[test]
    fn guards_every_call_and_keeps_labels_as_one_argument() {
        let b = binding();
        let inv = b.edit(Kind::Rename, "  Example account  ").unwrap();
        assert_eq!(
            inv.args,
            vec![
                "@studio",
                "--expect-machine",
                "11111111-1111-4111-8111-111111111111",
                "account",
                "rename",
                "work",
                "--revision",
                "r1",
                "--label",
                "Example account"
            ]
        );
        assert!(inv.stdin.is_none());
        assert_eq!(inv.timeout, Duration::from_secs(25));
        for kind in [Kind::Default, Kind::Remove] {
            assert!(b
                .edit(kind, "")
                .unwrap()
                .args
                .windows(2)
                .any(|a| a == ["--revision", "r1"]));
        }
    }
    #[test]
    fn refuses_changed_or_ambiguous_profiles_and_unbound_machines() {
        for field in ["provider", "home", "auth_revision"] {
            let mut v = catalog();
            v["profiles"][0][field] = json!("changed");
            assert!(binding().verify(&v).is_err());
        }
        let mut v = catalog();
        v["revision"] = json!("r2");
        assert!(binding().verify(&v).is_err());
        let mut v = catalog();
        let duplicate = v["profiles"][0].clone();
        v["profiles"].as_array_mut().unwrap().push(duplicate);
        assert!(binding().verify(&v).is_err());
        assert!(Binding::capture(ComputerId::local(), String::new(), &catalog(), "work").is_err());
        assert!(binding().edit(Kind::Rename, "\n").is_err());
    }
    #[test]
    fn permissions_take_provider_bypass_or_a_native_map_as_one_argument() {
        let values = |mode: &str| Values {
            mode: mode.into(),
            ..Default::default()
        };
        let native = r#"native:{"approval":"never"}"#;
        let call = binding()
            .edit_values(Kind::Permissions, &values(native))
            .unwrap();
        assert!(call.args.windows(2).any(|a| a == ["--mode", native]));
        for mode in ["bypass", "provider", "default"] {
            assert!(binding()
                .edit_values(Kind::Permissions, &values(mode))
                .is_ok());
        }
        for mode in ["auto", "native:{}", "native:{oops", "__editor__", ""] {
            assert!(
                binding()
                    .edit_values(Kind::Permissions, &values(mode))
                    .is_err(),
                "{mode}"
            );
        }
    }
    #[test]
    fn a_model_edit_reaches_the_native_deepseek_account() {
        let accounts = json!({"revision":"r1","profiles":[{"id":"native-dsh","provider":"dsh","home":"/accounts/deepseek","auth_revision":""},
            {"id":"native-opencode","provider":"opencode","home":"/accounts/opencode","auth_revision":""}]});
        let model = |id: &str| {
            Binding::capture(
                ComputerId("studio".into()),
                "11111111-1111-4111-8111-111111111111".into(),
                &accounts,
                id,
            )
            .unwrap()
            .edit_values(
                Kind::Model,
                &Values {
                    model: "deepseek-official/deepseek-flash".into(),
                    effort: "max".into(),
                    ..Default::default()
                },
            )
        };
        assert_eq!(
            &model("native-dsh").unwrap().args[3..],
            [
                "account",
                "model-defaults",
                "native-dsh",
                "--revision",
                "r1",
                "--model",
                "deepseek-official/deepseek-flash",
                "--effort",
                "max"
            ]
        );
        assert!(model("native-opencode").is_err());
    }
    #[test]
    fn a_scope_edit_names_shared_or_per_computer_only() {
        let scope = |mode: &str| {
            binding().edit_values(
                Kind::Scope,
                &Values {
                    mode: mode.into(),
                    ..Default::default()
                },
            )
        };
        let args = scope("computer").unwrap().args;
        assert_eq!(
            &args[3..],
            [
                "account",
                "permission-scope",
                "work",
                "--revision",
                "r1",
                "--scope",
                "computer"
            ]
        );
        assert!(scope("account").is_ok());
        for bad in ["", "everywhere", "bypass"] {
            assert!(scope(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn a_mark_edit_names_both_fields_and_clears_empty_ones() {
        let mark = |color: &str, emoji: &str, if_unset: bool| {
            binding().edit_values(
                Kind::Mark,
                &Values {
                    color: color.into(),
                    emoji: emoji.into(),
                    if_unset,
                    ..Default::default()
                },
            )
        };
        assert_eq!(
            &mark(" #2B97FF ", "🦊", false).unwrap().args[3..],
            [
                "account",
                "mark",
                "work",
                "--revision",
                "r1",
                "--color",
                "#2b97ff",
                "--emoji",
                "🦊"
            ]
        );
        assert_eq!(
            &mark("", "", true).unwrap().args[8..],
            ["--color", "none", "--emoji", "none", "--if-unset"]
        );
        for (color, emoji) in [
            ("red", ""),
            ("#2b97f", ""),
            ("", "A"),
            ("", "🦊 🦊"),
            ("", "--color"),
        ] {
            assert!(mark(color, emoji, false).is_err(), "{color} {emoji}");
        }
    }
}
