//! Task 68: the native permission catalog of one account profile (Qt's
//! PermissionCatalog and PermissionCatalogDialog). A computer's account row
//! describes its harness in `permission_catalog`: opaque control ids, human
//! labels and the values its own CLI accepts. Zerus stores a selection as
//! `native:<compact sorted JSON object>`; `hgs account permissions --mode`
//! takes it. The webview never parses the catalog: it draws [`Editor`] and
//! asks [`Catalog::check`] about each selection the owner makes.
use crate::text::Text;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const NATIVE_PREFIX: &str = "native:";
pub const MAX_NATIVE: usize = 1024;
/// The editor choice before a selection exists; never a saved mode.
pub const EDITOR: &str = "__editor__";
const MAX_CONTROLS: usize = 64;
const MAX_OPTIONS: usize = 64;
const MAX_LABEL: usize = 400;
const MAX_DESCRIPTION: usize = 2000;

pub type Selection = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Opt {
    pub value: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Control {
    pub id: String,
    pub label: String,
    pub description: String,
    pub options: Vec<Opt>,
    pub conflicts_with: Vec<String>,
}

/// One row of the account's permission selector.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Choice {
    pub label: Text,
    pub value: String,
    pub tooltip: Text,
    /// What the dialog says under the selector while this row is chosen.
    pub explanation: Text,
    pub bypass: bool,
    pub unavailable: bool,
    pub editor: bool,
}

/// One control in the native settings editor. Unknown keys keep their saved
/// value visible until the owner removes them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditorRow {
    pub id: String,
    pub label: Text,
    pub description: Text,
    pub selected: String,
    pub unknown: bool,
    pub unavailable: bool,
    pub options: Vec<Opt>,
}

/// The parsed catalog of one profile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub provider: String,
    pub harness: String,
    pub present: bool,
    pub available: bool,
    pub session_supported: bool,
    pub status_reason: String,
    pub source: String,
    pub controls: Vec<Control>,
    pub selected: Selection,
    pub saved_mode: String,
    pub native_saved: bool,
    pub malformed: bool,
    pub selected_valid: bool,
    pub unknown_keys: Vec<String>,
    pub current_detail: String,
    pub legacy_detail: String,
    pub effective_mode: String,
    pub current_values: Selection,
}

/// What the permissions dialog draws.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Editor {
    pub harness: String,
    pub choices: Vec<Choice>,
    /// The choice the dialog opens on (Qt's `initial`).
    pub initial: String,
    /// The saved mode; saving it again changes nothing.
    pub saved_mode: String,
    pub selected: Selection,
    pub malformed: bool,
    pub reset_label: Text,
    pub source_note: Text,
}

/// The core's answer about one editor selection.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Check {
    /// What the account selector saves: `provider` or `native:{…}`.
    pub mode: String,
    pub summary: Text,
    /// The editor choice's label for this selection.
    pub choice_label: Text,
    /// Why Save is disabled; empty when it may be saved.
    pub problem: Text,
    pub rows: Vec<EditorRow>,
}

fn text(value: &Value, key: &str, limit: usize) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|s| s.chars().take(limit).collect())
        .unwrap_or_default()
}

/// `^[a-z][a-z0-9_]*$`, at most 64 characters.
pub fn valid_control_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A saved `native:` map; None when it is not a bounded map of valid ids to
/// nonempty strings.
pub fn decode_native(mode: &str) -> Option<Selection> {
    let body = mode.strip_prefix(NATIVE_PREFIX)?;
    if mode.len() > MAX_NATIVE {
        return None;
    }
    let object: Map<String, Value> = serde_json::from_str(body).ok()?;
    let mut out = Selection::new();
    for (key, value) in object {
        let value = value.as_str()?;
        if !valid_control_id(&key) || value.is_empty() {
            return None;
        }
        out.insert(key, value.to_owned());
    }
    Some(out)
}

/// `provider` for an empty map, else the compact sorted `native:` value.
pub fn native_value(values: &Selection) -> String {
    let filtered: Map<String, Value> = values
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    if filtered.is_empty() {
        return "provider".into();
    }
    format!("{NATIVE_PREFIX}{}", Value::Object(filtered))
}

fn bounded(text: &str) -> String {
    if text.chars().count() > 80 {
        format!("{}…", text.chars().take(77).collect::<String>())
    } else {
        text.to_owned()
    }
}

fn unavailable_value(value: &str) -> Text {
    crate::text!("%1 (unavailable)", bounded(value))
}

fn is_bypass_text(value: &str, label: &str) -> bool {
    value.to_lowercase().contains("bypass") || label.to_lowercase().contains("bypass")
}

/// Prose joined with commas: the UI never separates values with dots.
fn join(parts: Vec<Text>) -> Text {
    let mut parts = parts.into_iter();
    let Some(first) = parts.next() else {
        return Text::default();
    };
    parts.fold(first, |all, next| crate::text!("%1, %2", all, next))
}

fn display_label(mode: &str) -> Text {
    match mode {
        "" | "provider" | "default" | "unknown" => crate::text!("Default"),
        "bypass" => crate::text!("Bypass"),
        "auto" => crate::text!("Auto"),
        "plan" => crate::text!("Plan"),
        "mixed" => crate::text!("Varies"),
        other => Text::verbatim(other),
    }
}

impl Catalog {
    /// Qt's `PermissionCatalog::parse` of one `hgs account ls` profile row.
    pub fn parse(profile: &Value) -> Self {
        let mut catalog = Catalog {
            provider: text(profile, "provider", 40),
            selected_valid: true,
            ..Default::default()
        };
        catalog.harness = match catalog.provider.as_str() {
            "claude" => "Claude Code".into(),
            "codex" => "Codex".into(),
            "kimi" => "Kimi".into(),
            "dsh" | "deepseek" => "DeepSeek".into(),
            "" => "harness".into(),
            other => other.into(),
        };
        catalog.saved_mode = text(profile, "permission_mode", MAX_NATIVE + 1);
        if catalog.saved_mode.is_empty() {
            catalog.saved_mode = "provider".into();
        }
        catalog.native_saved = catalog.saved_mode.starts_with(NATIVE_PREFIX);
        if let Some(current) = profile.get("permission_current").filter(|v| v.is_object()) {
            catalog.current_detail = text(current, "detail", MAX_DESCRIPTION);
            if let Some(values) = current.get("values").and_then(Value::as_object) {
                for (key, value) in values.iter().take(MAX_CONTROLS) {
                    if let Some(value) = value.as_str() {
                        catalog
                            .current_values
                            .insert(key.clone(), value.chars().take(MAX_LABEL).collect());
                    }
                }
            }
        }
        catalog.effective_mode = text(profile, "effective_permission_mode", 40);
        catalog.legacy_detail = text(profile, "permission_detail", MAX_DESCRIPTION);

        let raw = profile.get("permission_catalog").and_then(Value::as_object);
        catalog.present = raw.is_some_and(|r| !r.is_empty());
        let mut reported: Option<Selection> = None;
        if let (true, Some(raw)) = (catalog.present, raw) {
            let raw = Value::Object(raw.clone());
            let status = text(&raw, "status", 40);
            catalog.status_reason = text(&raw, "status_reason", MAX_DESCRIPTION);
            catalog.source = text(&raw, "source", MAX_LABEL);
            catalog.session_supported = raw["session_supported"].as_bool().unwrap_or(false);
            for control in raw["controls"]
                .as_array()
                .into_iter()
                .flatten()
                .take(MAX_CONTROLS)
            {
                let id = text(control, "id", 65);
                if !valid_control_id(&id) || catalog.controls.iter().any(|c| c.id == id) {
                    continue;
                }
                let mut label = text(control, "label", MAX_LABEL);
                if label.is_empty() {
                    label = id.clone();
                }
                let conflicts_with = control["conflicts_with"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|id| valid_control_id(id))
                    .map(str::to_owned)
                    .take(MAX_CONTROLS)
                    .collect();
                let options = control["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(MAX_OPTIONS)
                    .filter_map(|option| {
                        let value = text(option, "value", MAX_LABEL);
                        if value.is_empty() {
                            return None;
                        }
                        let label = text(option, "label", MAX_LABEL);
                        Some(Opt {
                            label: if label.is_empty() {
                                value.clone()
                            } else {
                                label
                            },
                            description: text(option, "description", MAX_DESCRIPTION),
                            value,
                        })
                    })
                    .collect();
                catalog.controls.push(Control {
                    id,
                    label,
                    description: text(control, "description", MAX_DESCRIPTION),
                    options,
                    conflicts_with,
                });
            }
            catalog.available = status == "available" && !catalog.controls.is_empty();
            if let Some(selected) = raw["selected"].as_object() {
                let mut map = Selection::new();
                for (key, value) in selected.iter().take(MAX_CONTROLS) {
                    match value.as_str() {
                        Some(value) if valid_control_id(key) && !value.is_empty() => {
                            map.insert(key.clone(), value.to_owned());
                        }
                        _ => {
                            catalog.malformed = true;
                            catalog.selected_valid = false;
                        }
                    }
                }
                reported = Some(map);
            }
        }

        if catalog.native_saved {
            match decode_native(&catalog.saved_mode) {
                None => {
                    catalog.malformed = true;
                    catalog.selected_valid = false;
                }
                // A backend-decoded selection is authoritative.
                Some(decoded) => catalog.selected = reported.unwrap_or(decoded),
            }
        }

        if !catalog.malformed {
            if catalog.available {
                let selected = catalog.selected.clone();
                for (key, value) in &selected {
                    match catalog.control(key) {
                        None => {
                            catalog.unknown_keys.push(key.clone());
                            catalog.selected_valid = false;
                        }
                        Some(control) if !control.options.iter().any(|o| &o.value == value) => {
                            catalog.selected_valid = false;
                        }
                        Some(_) => {}
                    }
                }
            } else if catalog.native_saved && !catalog.selected.is_empty() {
                // Without a catalog the fallback can only preserve the choice.
                catalog.selected_valid = false;
                catalog.unknown_keys = catalog.selected.keys().cloned().collect();
            }
        }
        if catalog.available && catalog.selected_valid && !catalog.selected.is_empty() {
            catalog.selected_valid = catalog.problem(&catalog.selected).is_none();
        }
        catalog
    }

    fn control(&self, id: &str) -> Option<&Control> {
        self.controls.iter().find(|c| c.id == id)
    }

    fn option(&self, control: &str, value: &str) -> Option<&Opt> {
        self.control(control)?
            .options
            .iter()
            .find(|o| o.value == value)
    }

    pub fn harness_settings(&self) -> Text {
        crate::text!("Use %1 settings", self.harness.clone())
    }

    /// Several controls (or unknown saved ones) are chosen in the editor; a
    /// single control's options are offered in the selector itself.
    fn uses_editor(&self) -> bool {
        self.controls.len() > 1 || !self.unknown_keys.is_empty()
    }

    /// Qt's `summary(catalog, values)`.
    pub fn summary(&self, values: &Selection) -> Text {
        if self.malformed {
            return unavailable_value(&self.saved_mode);
        }
        if values.is_empty() {
            if self.native_saved {
                return display_label("provider");
            }
            if self.saved_mode == "provider" {
                if !self.available && !self.current_values.is_empty() {
                    let raw: Vec<Text> = self
                        .current_values
                        .iter()
                        .map(|(k, v)| Text::verbatim(bounded(&format!("{k}={v}"))))
                        .collect();
                    return crate::text!("%1 (provider)", join(raw));
                }
                if !self.effective_mode.is_empty() {
                    return display_label(&self.effective_mode);
                }
                return display_label("provider");
            }
            if self.saved_mode == "auto" {
                return crate::text!("%1 (unavailable)", crate::text!("Auto"));
            }
            return display_label(&self.saved_mode);
        }
        if self.available {
            let mut labels = Vec::new();
            for control in &self.controls {
                let Some(value) = values.get(&control.id) else {
                    continue;
                };
                labels.push(match self.option(&control.id, value) {
                    Some(option) => Text::verbatim(option.label.clone()),
                    None => unavailable_value(value),
                });
            }
            for (key, value) in values {
                if self.control(key).is_none() {
                    labels.push(unavailable_value(&format!("{key}={value}")));
                }
            }
            if labels.is_empty() {
                return display_label("provider");
            }
            return join(labels);
        }
        let raw: Vec<Text> = values
            .iter()
            .map(|(k, v)| Text::verbatim(bounded(&format!("{k}={v}"))))
            .collect();
        crate::text!("%1 (unavailable)", join(raw))
    }

    fn editor_label(&self, values: &Selection) -> Text {
        if values.is_empty() {
            crate::text!("Native permission settings…")
        } else {
            crate::text!("Native permission settings… (%1)", self.summary(values))
        }
    }

    /// Qt's `selectionProblem`: why this selection cannot be saved.
    pub fn problem(&self, selection: &Selection) -> Option<Text> {
        if self.malformed {
            return Some(crate::text!(
                "Saved native permission settings cannot be read. Choose “%1” to reset them.",
                self.harness_settings()
            ));
        }
        if native_value(selection).len() > MAX_NATIVE {
            return Some(crate::text!(
                "Too many native settings are selected; remove some before saving."
            ));
        }
        for (key, value) in selection {
            let Some(control) = self.control(key).filter(|_| valid_control_id(key)) else {
                return Some(crate::text!(
                    "Unknown setting “%1” must be removed.",
                    key.clone()
                ));
            };
            if !control.options.iter().any(|o| &o.value == value) {
                return Some(crate::text!(
                    "“%1” is no longer offered for %2.",
                    value.clone(),
                    control.label.clone()
                ));
            }
        }
        for control in &self.controls {
            if !selection.contains_key(&control.id) {
                continue;
            }
            if let Some(other) = control
                .conflicts_with
                .iter()
                .find(|id| selection.contains_key(*id))
            {
                let other = self
                    .control(other)
                    .map(|c| c.label.clone())
                    .unwrap_or_else(|| other.clone());
                return Some(crate::text!(
                    "%1 cannot be combined with %2. Choose “%3” for one of them.",
                    control.label.clone(),
                    other,
                    self.harness_settings()
                ));
            }
        }
        None
    }

    /// Qt's `editorRows`: every control, then each unknown saved setting.
    pub fn rows(&self, selection: &Selection) -> Vec<EditorRow> {
        let mut rows: Vec<EditorRow> = self
            .controls
            .iter()
            .map(|control| {
                let selected = selection.get(&control.id).cloned().unwrap_or_default();
                EditorRow {
                    unavailable: !selected.is_empty()
                        && !control.options.iter().any(|o| o.value == selected),
                    id: control.id.clone(),
                    label: Text::verbatim(control.label.clone()),
                    description: Text::verbatim(control.description.clone()),
                    selected,
                    unknown: false,
                    options: control.options.clone(),
                }
            })
            .collect();
        for (key, value) in selection {
            if self.control(key).is_some() {
                continue;
            }
            rows.push(EditorRow {
                id: key.clone(),
                label: crate::text!("Unknown setting “%1”", key.clone()),
                description: crate::text!("This saved setting is not part of the current harness. Remove it before saving."),
                selected: value.clone(),
                unknown: true,
                unavailable: false,
                options: Vec::new(),
            });
        }
        rows
    }

    /// The editor's answer about a selection: its mode, summary and problem.
    /// Unknown saved settings left in it keep Save disabled.
    pub fn check(&self, selection: &Selection) -> Check {
        let rows = self.rows(selection);
        let problem = if self.malformed {
            crate::text!("Reset the unreadable saved settings before saving.")
        } else if rows.iter().any(|row| row.unknown) {
            crate::text!("Remove each unknown saved setting before saving.")
        } else {
            self.problem(selection).unwrap_or_default()
        };
        Check {
            mode: native_value(selection),
            summary: self.summary(selection),
            choice_label: self.editor_label(selection),
            problem,
            rows,
        }
    }

    fn provider_explanation(&self, machine: &str) -> Text {
        let detail = if self.saved_mode == "bypass" || self.native_saved {
            String::new()
        } else if self.current_detail.is_empty() {
            self.legacy_detail.clone()
        } else {
            self.current_detail.clone()
        };
        crate::text!(
            "Inherit %1 settings on %2. %3",
            self.harness.clone(),
            machine.to_owned(),
            detail
        )
    }

    fn bypass_explanation(&self) -> Text {
        match self.provider.as_str() {
            "kimi" => crate::text!("Run tools and answer questions automatically (Never Ask)."),
            "codex" | "dsh" => crate::text!("Full access without approval prompts or a sandbox."),
            _ => crate::text!("Run tools without permission prompts."),
        }
    }

    fn unavailable_choice(&self) -> Choice {
        Choice {
            label: crate::text!("Unavailable: %1", bounded(&self.saved_mode)),
            value: self.saved_mode.clone(),
            tooltip: crate::text!("This saved choice is not offered by the current harness. Choose another option before saving."),
            explanation: crate::text!("This saved choice is not offered by the current harness. Choose another option."),
            bypass: false,
            unavailable: true,
            editor: false,
        }
    }

    /// Qt's `accountChoices`, each with its explanation.
    pub fn choices(&self, machine: &str) -> Vec<Choice> {
        let mut choices = vec![
            Choice {
                label: self.harness_settings(),
                value: "provider".into(),
                tooltip: crate::text!(
                    "Use %1's own permission settings instead of a selection saved in Zerus.",
                    self.harness.clone()
                ),
                explanation: self.provider_explanation(machine),
                bypass: false,
                unavailable: false,
                editor: false,
            },
            Choice {
                label: crate::text!("Bypass permissions"),
                value: "bypass".into(),
                tooltip: crate::text!("Run tools without approval prompts."),
                explanation: self.bypass_explanation(),
                bypass: true,
                unavailable: false,
                editor: false,
            },
        ];
        if self.available {
            if !self.uses_editor() {
                let control = &self.controls[0];
                for option in &control.options {
                    let selection = Selection::from([(control.id.clone(), option.value.clone())]);
                    let tooltip = if !option.description.is_empty() {
                        Text::verbatim(option.description.clone())
                    } else if !control.description.is_empty() {
                        Text::verbatim(control.description.clone())
                    } else {
                        crate::text!("Native option from %1.", self.source.clone())
                    };
                    choices.push(Choice {
                        label: Text::verbatim(option.label.clone()),
                        value: native_value(&selection),
                        explanation: if option.description.is_empty() {
                            Text::verbatim(option.label.clone())
                        } else {
                            Text::verbatim(option.description.clone())
                        },
                        tooltip,
                        bypass: is_bypass_text(&option.value, &option.label),
                        unavailable: false,
                        editor: false,
                    });
                }
            } else {
                choices.push(Choice {
                    label: self.editor_label(&self.selected),
                    value: if self.selected.is_empty() {
                        EDITOR.into()
                    } else {
                        native_value(&self.selected)
                    },
                    tooltip: crate::text!("Choose each native permission option."),
                    explanation: crate::text!(
                        "Choose each native permission option for this account."
                    ),
                    bypass: false,
                    unavailable: !self.selected_valid,
                    editor: true,
                });
            }
        }
        if self.native_saved {
            if self.malformed
                || (!self.selected.is_empty()
                    && (!self.available || (!self.selected_valid && !self.uses_editor())))
            {
                choices.push(self.unavailable_choice());
            }
        } else if self.saved_mode == "auto" {
            choices.push(Choice {
                label: crate::text!("Auto (saved, unavailable)"),
                value: "auto".into(),
                tooltip: crate::text!(
                    "This saved legacy mode is honored for existing sessions but is not offered by the current harness."
                ),
                explanation: crate::text!("This saved choice is not offered by the current harness. Choose another option."),
                bypass: false,
                unavailable: true,
                editor: false,
            });
        } else if self.saved_mode != "provider" && self.saved_mode != "bypass" {
            choices.push(self.unavailable_choice());
        }
        choices
    }

    /// Qt's `sourceNote`.
    pub fn source_note(&self, machine: &str) -> Text {
        if !self.present {
            return crate::text!(
                "This computer does not report native permission options. Harness settings and Bypass remain available."
            );
        }
        if !self.available {
            let note = crate::text!(
                "Native permissions could not be discovered on %1. Harness settings and Bypass remain available.",
                machine.to_owned()
            );
            return if self.status_reason.is_empty() {
                note
            } else {
                crate::text!("%1 %2", note, self.status_reason.clone())
            };
        }
        let source = if self.source.is_empty() {
            crate::text!("this harness")
        } else {
            Text::verbatim(self.source.clone())
        };
        let note = crate::text!(
            "Native options come from %1 on %2.",
            source,
            machine.to_owned()
        );
        if self.session_supported {
            note
        } else {
            crate::text!(
                "%1 %2",
                note,
                crate::text!("This computer applies them to account defaults; a per-session choice is unavailable.")
            )
        }
    }

    /// The dialog for this profile on `machine`.
    pub fn editor(&self, machine: &str) -> Editor {
        let choices = self.choices(machine);
        let initial = if self.native_saved && !self.malformed && !self.selected.is_empty() {
            let value = native_value(&self.selected);
            // An editor or single-option row carries the map; an unavailable
            // row keeps the saved text.
            if choices.iter().any(|c| c.value == value) {
                value
            } else {
                self.saved_mode.clone()
            }
        } else {
            self.saved_mode.clone()
        };
        let mut choices = choices;
        if !choices.iter().any(|c| c.value == initial) {
            // Keep a saved selector visible even when it is no longer offered.
            choices.push(self.unavailable_choice());
        }
        Editor {
            harness: self.harness.clone(),
            choices,
            initial,
            saved_mode: self.saved_mode.clone(),
            selected: self.selected.clone(),
            malformed: self.malformed,
            reset_label: self.harness_settings(),
            source_note: self.source_note(machine),
        }
    }

    /// The mode Save sends: provider, bypass or a selection this catalog
    /// offers, in its canonical form. The saved mode itself is kept as it is.
    pub fn mode_to_save(&self, mode: &str) -> Result<String, String> {
        match mode {
            "provider" | "bypass" => return Ok(mode.into()),
            _ if mode == self.saved_mode
                && !self.malformed
                && self.selected_valid
                && self.native_saved =>
            {
                return Ok(mode.into())
            }
            _ => {}
        }
        let selection = decode_native(mode).ok_or("Choose an offered permission option.")?;
        if !self.available || selection.is_empty() || self.problem(&selection).is_some() {
            return Err("Choose an offered permission option.".into());
        }
        Ok(native_value(&selection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn codex(mode: &str) -> Value {
        json!({"id":"work","provider":"codex","permission_mode":mode,
            "permission_catalog":{"status":"available","source":"codex 0.130","session_supported":true,"controls":[
                {"id":"approval","label":"Approval","description":"When Codex asks","options":[
                    {"value":"on-request","label":"On request"},{"value":"never","label":"Never","description":"Never asks"}],
                 "conflicts_with":["bypass_all"]},
                {"id":"sandbox","label":"Sandbox","options":[{"value":"workspace-write","label":"Workspace write"},{"value":"read-only"}]},
                {"id":"bypass_all","label":"Bypass all","options":[{"value":"on","label":"Bypass everything"}]},
                {"id":"Bad","options":[{"value":"x"}]}]}})
    }

    #[test]
    fn native_values_are_compact_sorted_and_bounded() {
        let selection = Selection::from([
            ("sandbox".into(), "workspace-write".into()),
            ("approval".into(), "on-request".into()),
        ]);
        assert_eq!(
            native_value(&selection),
            r#"native:{"approval":"on-request","sandbox":"workspace-write"}"#
        );
        assert_eq!(native_value(&Selection::new()), "provider");
        assert_eq!(decode_native(&native_value(&selection)).unwrap(), selection);
        assert!(decode_native("native:{\"Mode\":\"auto\"}").is_none());
        assert!(decode_native("native:[]").is_none());
        assert!(decode_native("native:{\"mode\":1}").is_none());
        assert!(decode_native(&format!(
            "native:{{\"mode\":\"{}\"}}",
            "x".repeat(MAX_NATIVE)
        ))
        .is_none());
        assert!(
            valid_control_id("a1_b")
                && !valid_control_id("1a")
                && !valid_control_id("A")
                && !valid_control_id("")
        );
    }

    #[test]
    fn several_controls_open_the_editor_and_its_rows() {
        let catalog = Catalog::parse(&codex("provider"));
        assert!(catalog.available);
        assert_eq!(
            catalog.controls.len(),
            3,
            "an invalid control id is dropped"
        );
        let editor = catalog.editor("studio");
        let values: Vec<_> = editor.choices.iter().map(|c| c.value.as_str()).collect();
        assert_eq!(values, ["provider", "bypass", EDITOR]);
        assert_eq!(editor.initial, "provider");
        assert_eq!(editor.choices[0].label.english(), "Use Codex settings");
        assert_eq!(
            editor.source_note.english(),
            "Native options come from codex 0.130 on studio."
        );
        let rows = catalog.rows(&Selection::new());
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["approval", "sandbox", "bypass_all"]
        );
        assert_eq!(
            rows[1].options[1].label, "read-only",
            "an option without a label shows its value"
        );
    }

    #[test]
    fn the_check_names_conflicts_withdrawn_values_and_unknown_settings() {
        let catalog = Catalog::parse(&codex("provider"));
        let ok = Selection::from([
            ("approval".into(), "never".into()),
            ("sandbox".into(), "read-only".into()),
        ]);
        let check = catalog.check(&ok);
        assert!(check.problem.is_empty());
        assert_eq!(
            check.mode,
            r#"native:{"approval":"never","sandbox":"read-only"}"#
        );
        assert_eq!(check.summary.english(), "Never, read-only");
        assert_eq!(
            check.choice_label.english(),
            "Native permission settings… (Never, read-only)"
        );
        let conflict = Selection::from([
            ("approval".into(), "never".into()),
            ("bypass_all".into(), "on".into()),
        ]);
        assert_eq!(
            catalog.check(&conflict).problem.english(),
            "Approval cannot be combined with Bypass all. Choose “Use Codex settings” for one of them."
        );
        let withdrawn = Selection::from([("approval".into(), "always".into())]);
        assert_eq!(
            catalog.check(&withdrawn).problem.english(),
            "“always” is no longer offered for Approval."
        );
        assert!(catalog.check(&withdrawn).rows[0].unavailable);
        let unknown = Selection::from([("gone".into(), "x".into())]);
        let check = catalog.check(&unknown);
        assert_eq!(
            check.problem.english(),
            "Remove each unknown saved setting before saving."
        );
        assert!(check.rows.last().unwrap().unknown);
        assert_eq!(catalog.check(&Selection::new()).mode, "provider");
    }

    #[test]
    fn a_saved_native_map_opens_on_the_editor_choice() {
        let catalog = Catalog::parse(&codex(r#"native:{"approval":"never"}"#));
        let editor = catalog.editor("studio");
        assert_eq!(editor.initial, r#"native:{"approval":"never"}"#);
        let row = editor.choices.iter().find(|c| c.editor).unwrap();
        assert_eq!(row.value, editor.initial);
        assert_eq!(row.label.english(), "Native permission settings… (Never)");
        assert!(!row.unavailable);
        // An unknown saved key keeps the editor row, marked unavailable.
        let catalog = Catalog::parse(&codex(r#"native:{"gone":"x"}"#));
        let editor = catalog.editor("studio");
        assert!(
            editor
                .choices
                .iter()
                .find(|c| c.editor)
                .unwrap()
                .unavailable
        );
    }

    #[test]
    fn one_control_offers_its_options_in_the_selector() {
        let profile = json!({"provider":"claude","permission_mode":r#"native:{"mode":"plan"}"#,
            "permission_catalog":{"status":"available","source":"claude","controls":[{"id":"mode","label":"Mode","options":[
                {"value":"acceptEdits","label":"Accept edits","description":"Edits without asking"},
                {"value":"bypassPermissions","label":"Bypass"}]}]}});
        let catalog = Catalog::parse(&profile);
        let editor = catalog.editor("this computer");
        let values: Vec<_> = editor.choices.iter().map(|c| c.value.as_str()).collect();
        assert_eq!(
            values,
            [
                "provider",
                "bypass",
                r#"native:{"mode":"acceptEdits"}"#,
                r#"native:{"mode":"bypassPermissions"}"#,
                r#"native:{"mode":"plan"}"#
            ]
        );
        assert!(editor.choices[3].bypass, "a native bypass option is marked");
        assert_eq!(
            editor.choices[2].explanation.english(),
            "Edits without asking"
        );
        let saved = editor.choices.last().unwrap();
        assert!(
            saved.unavailable,
            "the withdrawn saved option stays visible and unavailable"
        );
        assert_eq!(editor.initial, saved.value);
        assert!(editor
            .source_note
            .english()
            .ends_with("a per-session choice is unavailable."));
    }

    #[test]
    fn unreadable_and_legacy_saved_modes_stay_visible() {
        let malformed = Catalog::parse(&codex("native:{oops"));
        assert!(malformed.malformed);
        let editor = malformed.editor("studio");
        assert_eq!(editor.initial, "native:{oops");
        assert!(editor.choices.last().unwrap().unavailable);
        assert!(malformed
            .check(&Selection::new())
            .problem
            .english()
            .starts_with("Reset"));
        let auto =
            Catalog::parse(&json!({"provider":"claude","permission_mode":"auto"})).editor("studio");
        assert_eq!(auto.initial, "auto");
        assert_eq!(
            auto.choices.last().unwrap().label.english(),
            "Auto (saved, unavailable)"
        );
        let absent = Catalog::parse(&json!({"provider":"kimi"}));
        assert_eq!(
            absent.source_note("studio").english(),
            "This computer does not report native permission options. Harness settings and Bypass remain available."
        );
        assert_eq!(
            absent.choices("studio")[1].explanation.english(),
            "Run tools and answer questions automatically (Never Ask)."
        );
    }

    #[test]
    fn save_takes_only_offered_modes_in_canonical_form() {
        let catalog = Catalog::parse(&codex("provider"));
        assert_eq!(catalog.mode_to_save("provider").unwrap(), "provider");
        assert_eq!(catalog.mode_to_save("bypass").unwrap(), "bypass");
        assert_eq!(
            catalog
                .mode_to_save(r#"native:{"sandbox":"read-only","approval":"never"}"#)
                .unwrap(),
            r#"native:{"approval":"never","sandbox":"read-only"}"#
        );
        assert!(catalog.mode_to_save(EDITOR).is_err());
        assert!(catalog.mode_to_save("auto").is_err());
        assert!(catalog
            .mode_to_save(r#"native:{"approval":"always"}"#)
            .is_err());
        assert!(catalog
            .mode_to_save(r#"native:{"approval":"never","bypass_all":"on"}"#)
            .is_err());
        assert!(catalog.mode_to_save("native:{}").is_err());
        assert!(Catalog::parse(&json!({"provider":"codex"}))
            .mode_to_save(r#"native:{"approval":"never"}"#)
            .is_err());
    }
}
