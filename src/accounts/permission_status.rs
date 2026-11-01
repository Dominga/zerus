//! Read-only account launch defaults. Per-project, managed policies and explicit
//! session switches may still override these; never present them as live state.
use super::*;

/// Fill the account row with the discovered catalog and the separate account
/// launch projection. Discovery is cached per provider by the caller.
pub(super) fn describe(
    config: &Config,
    row: &mut Value,
    mode: &str,
    catalogs: &mut permission_catalog::Cache,
) {
    let provider = string_field(row, "provider").to_owned();
    let home = PathBuf::from(string_field(row, "home"));
    let base = permission_catalog::catalog_for(config, &provider, &home, catalogs);
    let (catalog, current) = permission_catalog::describe_account(
        config,
        &provider,
        &home,
        row["native"].as_bool().unwrap_or(false),
        mode,
        &base,
    );
    let effective = permission_catalog::effective_mode(
        &provider,
        current["source"].as_str().unwrap_or("provider"),
        mode,
        &current["values"],
    );
    let detail = current["detail"].clone();
    row["permission_catalog"] = catalog;
    row["permission_current"] = current;
    row["permission_mode"] = json!(mode);
    row["effective_permission_mode"] = json!(effective);
    row["permission_detail"] = detail;
}
fn string_field<'a>(v: &'a Value, key: &str) -> &'a str { v[key].as_str().unwrap_or("") }

#[cfg(test)]
mod tests {
    use super::*;
    fn config(home: &Path) -> Config {
        Config {
            home: home.to_string_lossy().into_owned(),
            dir: home.join("config"),
            state: home.join("state"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        }
    }

    #[test]
    fn reads_provider_defaults_without_exposing_or_changing_credentials() {
        let tmp = tempfile::tempdir().unwrap(); let home = tmp.path();
        let config = config(home);
        let mut cache = permission_catalog::Cache::default();
        let path = home.join("config.toml");
        fs::write(&path,"approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\napi_key = 'secret'\n").unwrap();
        let mut row = json!({"id":"native-codex","provider":"codex","home":home});
        describe(&config, &mut row, "provider", &mut cache);
        assert_eq!(row["permission_current"]["values"]["approval"], "never");
        assert_eq!(row["permission_current"]["values"]["sandbox"], "danger-full-access");
        assert_eq!(row["effective_permission_mode"], "bypass");
        assert!(!row.to_string().contains("secret"));

        fs::write(&path,"permission_mode = 'yolo'\napi_key = 'secret'\n").unwrap();
        let mut row = json!({"id":"native-kimi","provider":"kimi","home":home});
        describe(&config, &mut row, "provider", &mut cache);
        assert_eq!(row["permission_current"]["values"]["mode"], "yolo");
        assert_eq!(row["effective_permission_mode"], "default");
        assert!(!row.to_string().contains("secret"));
    }

    #[test]
    fn absent_native_defaults_are_not_guessed() {
        let tmp = tempfile::tempdir().unwrap();
        let config = config(tmp.path());
        let mut cache = permission_catalog::Cache::default();
        let mut row = json!({"id":"native-codex","provider":"codex","home":tmp.path()});
        describe(&config, &mut row, "provider", &mut cache);
        assert!(row["permission_current"]["values"].as_object().unwrap().is_empty());
        assert_eq!(row["effective_permission_mode"], "default");
        assert!(row["permission_current"]["detail"].as_str().unwrap().contains("do not pin"));
    }

    #[test]
    fn claude_native_setting_is_projected_without_collapsing_to_default() {
        let tmp = tempfile::tempdir().unwrap();
        let config = config(tmp.path());
        let mut cache = permission_catalog::Cache::default();
        fs::write(
            tmp.path().join("settings.json"),
            r#"{"permissions":{"defaultMode":"acceptEdits"},"apiKey":"secret"}"#,
        )
        .unwrap();
        let mut row = json!({"id":"native-claude","provider":"claude","native":true,"home":tmp.path()});
        describe(&config, &mut row, "provider", &mut cache);
        assert_eq!(row["permission_current"]["values"]["mode"], "acceptEdits");
        assert_eq!(row["permission_current"]["source"], "provider");
        assert_eq!(row["effective_permission_mode"], "default");
        assert!(!row.to_string().contains("secret"));
    }

    #[test]
    fn saved_native_selection_is_kept_when_options_are_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let config = config(tmp.path());
        let mut cache = permission_catalog::Cache::default();
        let mut row = json!({"id":"native-claude","provider":"claude","home":tmp.path()});
        describe(&config, &mut row, "native:{\"mode\":\"acceptEdits\"}", &mut cache);
        assert_eq!(row["permission_catalog"]["selected"]["mode"], "acceptEdits");
        assert_eq!(row["permission_current"]["source"], "zerus");
    }
}
