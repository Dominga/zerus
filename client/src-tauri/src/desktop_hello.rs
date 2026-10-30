//! Background setup for the canonical instance's read-only delegation hello.
//! The socket callback uses cached identity only; there is no probe-time child.
use std::sync::Arc;
use zerus_client_core::desktop::{self, Build, Responder};

pub fn prepare(identity: &crate::identity::Identity) -> (crate::instance::Reply, Arc<Responder>) {
    let info = crate::cli::build_info(&crate::cli::BUILD);
    let responder = Arc::new(Responder::new(
        identity.bundle_id.clone(),
        Build {
            version: info["versionName"].as_str().unwrap_or("").into(),
            source_commit: info["sourceCommit"].as_str().unwrap_or("").into(),
            source_dirty: info["sourceDirty"].as_bool(),
        },
        uuid::Uuid::new_v4().to_string(),
        true,
        std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|v| !v.is_empty()),
    ));
    let cached = Arc::clone(&responder);
    let reply: crate::instance::Reply = Arc::new(move |stream, line| {
        desktop::same_user(stream)
            .then(|| cached.reply(line))
            .flatten()
    });
    (reply, responder)
}

/// Only the process that actually claimed the socket calls this, once. Missing
/// help/configuration stays unknown until another app start; no retry storm.
pub fn configure(responder: Arc<Responder>, production: bool) {
    if !production || std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|v| !v.is_empty()) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("zerus-desktop-identity".into())
        .spawn(move || {
            let selected_hgs = zerus_client_core::default_hgs();
            if let Some(configuration) = desktop::selected_configuration(&selected_hgs, &[]) {
                responder.configure_once(configuration);
            }
        });
}
