//! Per-account launch choices. Native configuration and running sessions stay owned by the agent.
use super::*;
use std::io::Read;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Defaults {
    #[serde(default)]
    model: String,
    #[serde(default)]
    effort: String,
}
fn supported(provider: &str) -> bool {
    ["codex", "claude", "kimi"].contains(&provider)
}
fn safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && !value.starts_with('-')
        && !value.chars().any(char::is_control)
}
fn provider_for<'a>(catalog: &'a Catalog, id: &'a str) -> Option<&'a str> {
    if native_id(id) {
        id.strip_prefix("native-")
    } else {
        catalog.profiles.get(id).map(|p| p.provider.as_str())
    }
}
pub(super) fn validate_catalog(catalog: &Catalog) -> Result<()> {
    for (id, value) in &catalog.model_defaults {
        let provider = provider_for(catalog, id).unwrap_or("");
        if !supported(provider)
            || !safe(&value.model)
            || (!value.effort.is_empty()
                && (!["codex", "claude"].contains(&provider)
                    || ![
                        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
                    ]
                    .contains(&value.effort.as_str())))
        {
            return Err(Error::new(1, "Invalid account model defaults."));
        }
    }
    Ok(())
}
pub(super) fn describe(row: &mut Value, catalog: &Catalog) {
    let id = row["id"].as_str().unwrap_or("");
    let provider = row["provider"].as_str().unwrap_or("");
    let defaults = catalog.model_defaults.get(id).cloned().unwrap_or_default();
    row["model_defaults_supported"] = json!(supported(provider));
    row["model_defaults"] = json!(defaults);
}
fn native(provider: &str, home: &Path) -> Defaults {
    let mut text = String::new();
    let name = if provider == "claude" {
        "settings.json"
    } else {
        "config.toml"
    };
    if fs::File::open(home.join(name))
        .and_then(|f| f.take(1024 * 1024 + 1).read_to_string(&mut text))
        .is_err()
        || text.len() > 1024 * 1024
    {
        return Defaults::default();
    }
    if provider == "claude" {
        let data: Value = serde_json::from_str(&text).unwrap_or_default();
        return Defaults {
            model: data["model"]
                .as_str()
                .filter(|v| safe(v))
                .unwrap_or_default()
                .into(),
            effort: data["effortLevel"]
                .as_str()
                .filter(|v| safe(v))
                .unwrap_or_default()
                .into(),
        };
    }
    let Ok(data) = text.parse::<toml_edit::DocumentMut>() else {
        return Defaults::default();
    };
    let profile = data.get("profile").and_then(|v| v.as_str());
    let get = |key: &str| {
        profile
            .filter(|_| provider == "codex")
            .and_then(|p| data.get("profiles")?.get(p)?.get(key)?.as_str())
            .or_else(|| data.get(key).and_then(|v| v.as_str()))
            .filter(|v| safe(v))
            .unwrap_or_default()
            .to_owned()
    };
    Defaults {
        model: get(if provider == "kimi" {
            "default_model"
        } else {
            "model"
        }),
        effort: get(if provider == "kimi" {
            "default_reasoning_effort"
        } else {
            "model_reasoning_effort"
        }),
    }
}
pub(super) fn inspect(config: &Config, id: &str) -> Result<Value> {
    let bytes = read(config)?;
    let catalog = decode(&bytes)?;
    let (profile, home, _) = lookup(config, id)?;
    if !supported(&profile.provider) && profile.provider != "dsh" {
        return Err(Error::new(
            1,
            "This agent manages model defaults in its native settings.",
        ));
    }
    let mut result = if profile.provider == "dsh" {
        let mut result = crate::state::dsh::account_models(&home);
        if result["models"].as_array().is_some_and(|models| models.is_empty()) {
            let fallback = inspection::dsh(&home);
            if fallback["models"].as_array().is_some_and(|models| !models.is_empty()) {
                result["models"] = fallback["models"].clone();
                result["truncated"] = json!(false);
                result.as_object_mut().unwrap().remove("reason");
            }
        }
        result
    } else {
        crate::state::account_models(&profile.provider, &home)
    };
    result["account_id"] = json!(id);
    result["provider"] = json!(profile.provider);
    result["revision"] = json!(revision(&bytes));
    result["defaults"] = json!(catalog.model_defaults.get(id).cloned().unwrap_or_default());
    result["provider_defaults"] = json!(if profile.provider == "dsh" {
        Defaults::default()
    } else {
        native(&profile.provider, &home)
    });
    result["effort_supported"] = json!(["codex", "claude", "dsh"].contains(&profile.provider.as_str()));
    Ok(result)
}
pub(super) fn set(config: &Config, args: &[String], dry: bool) -> Result<Value> {
    let id = args.get(1).map(String::as_str).unwrap_or("");
    let model = arg(args, "--model").ok_or_else(|| {
        Error::new(
            1,
            "Choose --model MODEL, or an empty value to inherit provider settings.",
        )
    })?;
    let effort = arg(args, "--effort").unwrap_or("");
    let _lock = locked(config)?;
    let bytes = read(config)?;
    check_revision(&bytes, args)?;
    let mut catalog = decode(&bytes)?;
    let (profile, home, _) = lookup(config, id)?;
    if !supported(&profile.provider) || catalog.hidden_profiles.contains(id) {
        return Err(Error::new(
            1,
            "Choose an available Codex, Claude or Kimi account.",
        ));
    }
    if model.is_empty() {
        if !effort.is_empty() {
            return Err(Error::new(1, "Choose a model before setting effort."));
        }
        catalog.model_defaults.remove(id);
    } else {
        let models = crate::state::account_models(&profile.provider, &home);
        let selected=models["models"].as_array().into_iter().flatten().find(|entry|entry["id"]==model)
            .ok_or_else(||Error::new(1,"The model is unavailable in this account's native catalog. Open the agent once and refresh."))?;
        if !effort.is_empty()
            && (!effort_supported(&profile.provider)
                || !selected["effort_options"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v == effort)))
        {
            return Err(Error::new(
                1,
                "Effort is unavailable for the selected model.",
            ));
        }
        catalog.model_defaults.insert(
            id.into(),
            Defaults {
                model: model.into(),
                effort: effort.into(),
            },
        );
    }
    validate_catalog(&catalog)?;
    if !dry {
        save(config, &catalog)?;
    }
    list(config)
}
fn effort_supported(provider: &str) -> bool {
    ["codex", "claude"].contains(&provider)
}
pub(super) fn apply(
    config: &Config,
    account: &str,
    provider: &str,
    argv: &mut Vec<String>,
) -> Result<()> {
    let id = if account.is_empty() {
        format!("native-{provider}")
    } else {
        account.into()
    };
    let catalog = decode(&read(config)?)?;
    let Some(defaults) = catalog.model_defaults.get(&id) else {
        return Ok(());
    };
    if provider_for(&catalog, &id) != Some(provider) {
        return Err(Error::new(1, "The account belongs to another provider."));
    }
    apply_argv(provider, defaults, argv);
    Ok(())
}
fn apply_argv(provider: &str, defaults: &Defaults, argv: &mut Vec<String>) {
    let at = argv.iter().position(|v| v == "--").unwrap_or(argv.len());
    if at == 0 {
        return;
    }
    let args = &argv[1..at];
    let config_value = |i: usize, value: &str| -> Option<String> {
        let raw = if value == "-c" || value == "--config" {
            args.get(i + 1).map(String::as_str)
        } else if let Some(v) = value.strip_prefix("--config=") {
            Some(v)
        } else {
            value.strip_prefix("-c").filter(|v| !v.is_empty())
        };
        raw.and_then(|v| v.split_once('='))
            .map(|(key, _)| key.trim().to_owned())
    };
    let explicit_model = args.iter().any(|v| {
        ["--model", "-m", "--profile", "-P"].contains(&v.split('=').next().unwrap_or(""))
            || (v.starts_with("-m") && v.len() > 2)
            || (v.starts_with("-P") && v.len() > 2)
    }) || args
        .iter()
        .enumerate()
        .any(|(i, v)| config_value(i, v).as_deref() == Some("model"));
    if explicit_model {
        return;
    }
    let explicit_effort = args.iter().enumerate().any(|(i, v)| {
        v.split('=').next() == Some("--effort")
            || config_value(i, v).as_deref() == Some("model_reasoning_effort")
    });
    let mut values = vec!["--model".into(), defaults.model.clone()];
    if !defaults.effort.is_empty() && !explicit_effort {
        if provider == "codex" {
            values.extend([
                "-c".into(),
                format!("model_reasoning_effort=\"{}\"", defaults.effort),
            ]);
        } else if provider == "claude" {
            values.extend(["--effort".into(), defaults.effort.clone()]);
        }
    }
    argv.splice(at..at, values);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_choices_and_prompt_text_keep_their_meaning() {
        let defaults = Defaults {
            model: "fixture-model".into(),
            effort: "high".into(),
        };
        for provider in ["codex", "claude"] {
            let mut argv = vec![
                provider.into(),
                "--".into(),
                "--model".into(),
                "prompt-only".into(),
            ];
            apply_argv(provider, &defaults, &mut argv);
            assert_eq!(&argv[1..3], ["--model", "fixture-model"]);
            assert_eq!(&argv[argv.len() - 3..], ["--", "--model", "prompt-only"]);
            for explicit in [
                vec!["--model", "explicit"],
                vec!["-m=explicit"],
                vec!["-c", "model = 'explicit'"],
                vec!["--profile", "explicit"],
            ] {
                let mut argv = vec![provider.into()];
                argv.extend(explicit.iter().map(|v| v.to_string()));
                let before = argv.clone();
                apply_argv(provider, &defaults, &mut argv);
                assert_eq!(argv, before);
            }
        }
        let mut argv = vec![
            "codex".into(),
            "-c".into(),
            "model_reasoning_effort=low".into(),
        ];
        apply_argv("codex", &defaults, &mut argv);
        assert!(argv.contains(&"fixture-model".into()));
        assert_eq!(
            argv.iter()
                .filter(|v| v.contains("model_reasoning_effort"))
                .count(),
            1
        );
    }
}
