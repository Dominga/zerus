//! A startup refusal gets a visible native dialog without constructing the
//! client source, Bootstrap, notifier, or any background writer.
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

pub(crate) fn show(context: tauri::Context<tauri::Wry>) {
    let title = crate::native_text::render(&zerus_client_core::text!("Zerus could not start"));
    let message = crate::native_text::render(&zerus_client_core::text!(
        "Zerus could not confirm desktop ownership. Background work has not started. Close this message, then start Zerus again through its normal launcher. If the problem continues, check the desktop service."
    ));
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let quit = app.handle().clone();
            app.dialog()
                .message(message)
                .title(title)
                .kind(MessageDialogKind::Error)
                .show(move |_| quit.exit(0));
            Ok(())
        })
        .run(context);
    if let Err(error) = result {
        eprintln!("zerus-client: could not show startup error: {error}");
    }
}
