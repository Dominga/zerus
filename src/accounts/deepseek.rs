use super::*;
use std::io::{IsTerminal, Read};

pub(super) fn set_key(config: &Config, args: &[String], dry: bool) -> Result<Value> {
    if dry || args.get(1).map(String::as_str) != Some("native-dsh") {
        return Err(Error::new(
            1,
            "Use account set-key native-dsh --json with an API key on stdin.",
        ));
    }
    if std::io::stdin().is_terminal() || !args.iter().any(|a| a == "--json") {
        return Err(Error::new(1, "API key input requires a private JSON pipe."));
    }
    let mut bytes = Vec::new();
    std::io::stdin().take(8193).read_to_end(&mut bytes)?;
    let input: Value =
        serde_json::from_slice(crate::state::without_bom(&bytes)).map_err(|_| Error::new(1, "Invalid API key request."))?;
    let key = input["api_key"].as_str().unwrap_or("");
    if bytes.len() > 8192
        || key.is_empty()
        || key.len() > 4096
        || key.chars().any(char::is_whitespace)
        || key.chars().any(char::is_control)
    {
        return Err(Error::new(
            1,
            "Enter a non-empty API key without spaces or line breaks.",
        ));
    }
    let label = input["label"].as_str();
    if let Some(label) = label {
        validate_label(label)?;
    }
    if !platform::available("dsh") {
        return Err(Error::new(
            1,
            "Install DeepSeek on this machine before saving its API key.",
        ));
    }
    let _lock = locked(config)?;
    let catalog_bytes = read(config)?;
    check_revision(&catalog_bytes, args)?;
    let mut catalog = decode(&catalog_bytes)?;
    let url = crate::state::dsh::native_ui_url("@account").map_err(|_| {
        Error::new(
            1,
            "Could not connect to the native DeepSeek host on this machine.",
        )
    })?;
    let mut command = Command::new("node");
    command.args([
        "--input-type=module",
        "-e",
        concat!(
            include_str!("../dsh/account-client.mjs"),
            "\n",
            include_str!("../dsh/account-key.mjs")
        ),
    ]);
    let output = inspection::capture(
        command,
        json!({"url":url,"api_key":key}).to_string().as_bytes(),
        20,
    )
    .map_err(|_| {
        Error::new(
            1,
            "DeepSeek did not confirm saving the API key. Refresh the account before retrying.",
        )
    })?;
    let result: Value = serde_json::from_slice(&output)
        .map_err(|_| Error::new(1, "Invalid DeepSeek save response."))?;
    if result["ok"] != true {
        return Err(Error::new(
            1,
            result["error"]
                .as_str()
                .unwrap_or("Could not save the DeepSeek API key."),
        ));
    }
    if let Some(label) = label {
        catalog
            .native_labels
            .insert("native-dsh".into(), label.into());
    }
    catalog.hidden_profiles.remove("native-dsh");
    save(config, &catalog)?;
    // Refresh the sanitized status cache after the native service confirms the write.
    let _ = inspection::inspect(
        config,
        &["inspect".into(), "native-dsh".into(), "--refresh".into()],
    );
    list(config)
}
