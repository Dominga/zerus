//! Isolated synthetic window for the real-display first-frame check. Reuses the
//! production presentation paths and theme/gate, with no tray, fleet, service or GUI claim.
//! The optional layer uses only a synthetic private settings file (no Qt seed).
//! The bundle ID remains canonical. This is not a second Zerus application.
#[cfg(all(target_os = "linux", feature = "live"))]
use zerus_client_lib::{lifecycle, window_theme, MAIN_WINDOW};
#[cfg(all(target_os = "linux", feature = "live"))]
fn main() {
    use gtk::{gdk::prelude::*, prelude::*};
    use std::{
        borrow::Cow,
        cell::Cell,
        rc::Rc,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };
    use tauri::{
        utils::assets::{AssetKey, AssetsIter, CspHash},
        Manager,
    };
    struct Assets;
    impl<R: tauri::Runtime> tauri::Assets<R> for Assets {
        fn get(&self, _: &AssetKey) -> Option<Cow<'_, [u8]>> {
            Some(Cow::Borrowed(
                b"<!doctype html><html><head><meta charset='utf-8'></head><body></body></html>",
            ))
        }
        fn iter(&self) -> Box<AssetsIter<'_>> {
            Box::new(std::iter::empty())
        }
        fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
            Box::new(std::iter::empty())
        }
    }
    let mut args = std::env::args().skip(1);
    let route = args.next().expect("lifecycle or window-layer");
    assert!(matches!(route.as_str(), "lifecycle" | "window-layer"));
    let theme = args.next().expect("dark or light");
    let background = match theme.as_str() {
        "dark" | "system-dark" | "stalled" => [22u8, 27, 33],
        "light" | "system-light" => [245, 247, 249],
        _ => panic!("dark or light"),
    };
    let output = std::path::PathBuf::from(args.next().expect("fresh output directory"));
    std::fs::create_dir(&output).unwrap();
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "com.hgdev.hgs-tray".into();
    context.config_mut().build.dev_url = None;
    context.set_assets(Box::new(Assets));
    let frames = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let selected = frames.clone();
    let app = tauri::Builder::default()
        .manage(window_theme::Theme::default())
        .invoke_handler(tauri::generate_handler![window_theme::window_theme])
        .setup(move |app| {
            eprintln!("fixture setup started");
            if route == "window-layer" {
                app.manage(zerus_client_lib::settings::Store(zerus_client_core::settings::Settings::open(
                    output.join("fixture-settings.json"))));
                zerus_client_lib::window_layer::init(app.handle(), None);
            }
            let generation = window_theme::register(app.handle());
            let [r, g, b] = background;
            gtk::Settings::default().unwrap().set_gtk_theme_name(Some("Adwaita"));
            gtk::Settings::default().unwrap().set_gtk_application_prefer_dark_theme(theme.ends_with("dark"));
            let css = if theme.starts_with("system-") {
                "html{background:#f5f7f9}@media(prefers-color-scheme:dark){html{background:#161b21}}".to_owned()
            } else { format!("html{{background:rgb({r},{g},{b})}}") };
            let script = format!(r#"document.addEventListener('DOMContentLoaded', () => setTimeout(() => {{
                const style = document.createElement('style'); style.textContent = {css:?}; document.head.append(style);
                document.body.style.cssText = 'margin:0;background:inherit;height:100vh';
                const rgb = getComputedStyle(document.documentElement).backgroundColor.match(/\d+/g).slice(0,3).map(Number);
                window.__TAURI_INTERNALS__.invoke('window_theme', {{generation:{generation:?}, revision:1, background:rgb}});
            }}, 700), {{once:true}});"#);
            let script = if theme == "stalled" { String::new() } else { script };
            eprintln!("fixture building webview");
            let window = tauri::WebviewWindowBuilder::new(app, MAIN_WINDOW, tauri::WebviewUrl::App("index.html".into()))
                .title(format!("Zerus theme fixture ({theme})"))
                .visible(false)
                .inner_size(640., 400.)
                .background_color(tauri::window::Color(22, 27, 33, 255))
                .initialization_script(script)
                .build()?;
            // Emulate each OS appearance only in this private process; Tao's
            // portal observation during construction can override GTK's initial
            // preference. Never change the desktop's actual theme.
            window.set_theme(Some(if theme.ends_with("dark") {
                tauri::Theme::Dark
            } else {
                tauri::Theme::Light
            }))?;
            eprintln!("fixture webview built");
            window_theme::hidden(app.handle());
            lifecycle::request_main(app.handle()).unwrap();
            eprintln!("fixture first presentation deferred");
            assert!(!window.is_visible().unwrap(), "unpainted page was exposed");
            eprintln!("fixture hidden state verified");
            let native = window.gtk_window()?;
            eprintln!("fixture native window captured");
            let drawn = Rc::new(Cell::new(false));
            let observed = drawn.clone();
            native.connect_draw(move |_, _| { observed.set(true); gtk::glib::Propagation::Proceed });
            let app = app.handle().clone();
            let at = Instant::now();
            gtk::glib::timeout_add_local(Duration::from_millis(8), move || {
                if at.elapsed() > Duration::from_secs(8) {
                    eprintln!("fixture theme did not become visible before deadline");
                    app.exit(2);
                    return gtk::glib::ControlFlow::Break;
                }
                if !drawn.get() || !window.is_visible().unwrap_or(false) { return gtk::glib::ControlFlow::Continue; }
                let Some(surface) = native.window() else { return gtk::glib::ControlFlow::Continue; };
                let Some(pixels) = surface.pixbuf(native.allocated_width()/2-16, native.allocated_height()/2-16, 32, 32) else { return gtk::glib::ControlFlow::Continue; };
                let bytes = unsafe { pixels.pixels() };
                let stride = pixels.rowstride() as usize;
                let channels = pixels.n_channels() as usize;
                let rgb: Vec<_> = (0..3).map(|i| bytes[16*stride+16*channels+i]).collect();
                let mut recorded = selected.lock().unwrap();
                let frame = recorded.len();
                if matches!(frame, 0|1|5|15|29) {
                    pixels.savev(output.join(format!("frame-{frame}.png")), "png", &[]).unwrap();
                }
                recorded.push(serde_json::json!({"ms":at.elapsed().as_millis(),"rgb":rgb}));
                if recorded.len() == 30 {
                    let result = serde_json::json!({"route":route,"theme":theme,"expected":background,"frames":*recorded});
                    std::fs::write(output.join("result.json"), serde_json::to_vec_pretty(&result).unwrap()).unwrap();
                    app.exit(0);
                    gtk::glib::ControlFlow::Break
                } else { gtk::glib::ControlFlow::Continue }
            });
            Ok(())
        }).build(context).expect("synthetic fixture app");
    app.run(|_, _| {});
    assert_eq!(frames.lock().unwrap().len(), 30);
}
#[cfg(not(all(target_os = "linux", feature = "live")))]
fn main() {
    eprintln!("The WebKitGTK first-frame fixture runs on Linux.");
}
