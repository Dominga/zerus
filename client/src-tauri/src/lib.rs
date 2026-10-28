//! Zerus Next: the Tauri shell around the SolidJS frontend.
//!
//! The window is created here rather than in `tauri.conf.json`, so platform
//! code can hand the webview its configuration before it exists (the macOS
//! frame-rate preference, plan Task 7).

#[cfg(feature = "live")]
mod actions;
mod cli;
mod clipboard;
mod commands;
#[cfg(feature = "live")]
mod desk;
mod display;
mod identity;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod instance;
mod lifecycle;
mod metrics;
mod platform;
#[cfg(feature = "live")]
mod pump;
#[cfg(feature = "live")]
mod readsync;
pub mod renderer;
#[cfg(feature = "live")]
mod replay;
#[cfg(feature = "live")]
mod screens;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod signals;

use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The label of the one window this phase opens.
pub const MAIN_WINDOW: &str = "main";

/// Tests that start child processes hold this shared; the tests that sample
/// this process's tree hold it alone, since a child that exits mid-sample
/// leaves the tree's memory total unknown.
#[cfg(all(test, unix))]
pub(crate) static PROCESS_TREE: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// What `run` settled before the app starts.
pub struct Startup {
    pub identity: identity::Identity,
    /// Started with `--sessions`, or a build that opens its window by default.
    pub opens_sessions: bool,
    /// The claimed single-instance socket and the activations it receives.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub instance: Option<instance::Claimed>,
}

/// Parses Qt's command line and answers `--help`, `--version` and
/// `--build-info`. It then takes the single-instance socket, or hands the
/// activation to the running instance and returns, and runs the app.
pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match cli::parse(&args) {
        Ok(cli::Command::Run(options)) => options,
        Ok(cli::Command::Help) => {
            print!("{}", cli::usage());
            return;
        }
        Ok(cli::Command::Version) => {
            print!("{}", cli::version_line(&cli::BUILD));
            return;
        }
        Ok(cli::Command::BuildInfo) => {
            println!("{}", cli::build_info(&cli::BUILD));
            return;
        }
        Err(error) => {
            eprint!("zerus-client: {error}\n\n{}", cli::usage());
            std::process::exit(2);
        }
    };
    for option in &options.ignored {
        eprintln!("zerus-client: {option} is accepted for the Qt GUI's command line and not used");
    }
    // Before any thread starts: the core's `default_hgs` reads it.
    if let Some(hgs) = &options.hgs {
        std::env::set_var("ZERUS_CLIENT_HGS", hgs);
    }
    let context = tauri::generate_context!();
    let identity = identity::current(&context.config().identifier);
    let opens_sessions = options.opens_sessions(identity.is_switch());
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let instance = match identity.instance_name(identity::flag("ZERUS_CLIENT_SINGLE_INSTANCE")) {
        None => None,
        Some(name) => {
            let activation = if opens_sessions {
                instance::Activation::Sessions
            } else {
                instance::Activation::Show
            };
            match instance::claim(&name, identity.launchd_label(), activation) {
                instance::Claim::Claimed(claimed) => Some(claimed),
                instance::Claim::Forwarded => {
                    eprintln!("zerus-client: already running");
                    return;
                }
                instance::Claim::Failed(error) => {
                    eprintln!("zerus-client: cannot claim the socket: {error}");
                    std::process::exit(3);
                }
            }
        }
    };
    #[cfg(target_os = "macos")]
    platform::macos::raise_main_thread_qos();
    launch(
        context,
        Startup {
            identity,
            opens_sessions,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            instance,
        },
    );
}

/// The client's private data directory: the running bundle's identifier
/// (Decision 5). Tauri's `app_data_dir` names the compile-time identifier,
/// which a switch build promotes at signing, so the client never calls it.
#[cfg_attr(not(feature = "live"), allow(dead_code))]
pub(crate) fn data_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    app.state::<identity::Identity>().data_dir_now()
}

/// Builds the app and runs its event loop. A build with a menu-bar item
/// starts hidden unless asked for the window, and runs until it quits.
fn launch(context: tauri::Context<tauri::Wry>, startup: Startup) {
    let identity = startup.identity;
    let opens_sessions = startup.opens_sessions;
    // Only the macOS build has a menu-bar item in this phase; Linux keeps Qt.
    // The item lists the live fleet, so a build without the data source has
    // none and keeps its window as the way back.
    let menu_bar = cfg!(all(target_os = "macos", feature = "live"));
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let (listening, activations) = match startup.instance {
        Some(claimed) => (Some(claimed.listening), Some(claimed.activations)),
        None => (None, None),
    };
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(commands::AppState::default())
        .manage(lifecycle::Lifecycle { menu_bar });
    // tao activates the app when it finishes launching, taking the focus from
    // the app in front; a menu-bar build activates only to show its window.
    #[cfg(target_os = "macos")]
    let builder = builder.activate_ignoring_other_apps(!menu_bar);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let builder = builder.manage(instance::InstanceState(std::sync::Mutex::new(listening)));
    // A page that loads (again) takes what the menu bar sent once it asks.
    #[cfg(feature = "live")]
    let builder = builder.on_page_load(|webview, payload| {
        use tauri::Manager;
        if webview.label() == MAIN_WINDOW
            && matches!(payload.event(), tauri::webview::PageLoadEvent::Started)
        {
            desk::page_loading(webview.app_handle());
        }
    });
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut app = builder
        .invoke_handler(handler())
        .setup(move |app| {
            use tauri::Manager;
            // The one source of the client's data directory (Decision 5).
            app.manage(identity);
            #[cfg(feature = "live")]
            {
                let dir = data_dir(app.handle());
                std::fs::create_dir_all(&dir)?;
                // A replay never touches the real read or attention marks.
                let replay = std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|list| !list.is_empty());
                let actions = actions::Actions::new(dir.join(actions::marks_file(replay)));
                readsync::start(actions.clone());
                app.manage(actions);
                app.manage(actions::ArchiveReads::default());
                app.manage(actions::Attention::new(dir.join(actions::attention_file(replay))));
                let pump = app.state::<commands::AppState>().pump();
                app.manage(screens::Screens::tauri(app.handle().clone(), pump.clone()));
                // The menu-bar item, before the fleet starts so its first catalog reaches it.
                #[cfg(target_os = "macos")]
                {
                    let conf = desk::tray_conf();
                    let configured = conf
                        .as_deref()
                        .and_then(|text| zerus_client_core::menubar::tray_conf_value(text, "icon-color"))
                        .filter(|value| !value.is_empty())
                        .and_then(|value| {
                            let color = zerus_client_core::trayicon::parse_color(&value);
                            if color.is_none() {
                                eprintln!("zerus-client: icon-color {value:?} in tray.conf is not #rgb or #rrggbb; using the menu bar's colour");
                            }
                            color
                        });
                    platform::macos_menubar::install(app.handle(), configured)?;
                    desk::start(app.handle(), &pump, dir.clone(), desk::load_mode(&dir, conf.as_deref()));
                }
                if menu_bar {
                    lifecycle::start_fleet(|| app.state::<commands::AppState>().start_source());
                }
            }
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                if let Some(activations) = activations {
                    forward_activations(app.handle().clone(), activations);
                }
                let handle = app.handle().clone();
                signals::install(move |signal| {
                    eprintln!("zerus-client: quitting on signal {signal}");
                    handle.exit(0);
                })?;
            }
            let display = display::display_name();
            if !display::headless_allowed(std::env::var("ZERUS_CLIENT_HEADLESS").is_ok_and(|v| v == "1"), display.as_deref()) {
                eprintln!("zerus-client: ZERUS_CLIENT_HEADLESS=1 but the display is {display:?}, not an X server; no window opens (set GDK_BACKEND=x11)");
                std::process::exit(3);
            }
            if lifecycle::window_at_start(opens_sessions, menu_bar) {
                let window = lifecycle::show_main(app.handle())?;
                // An unattended measurement needs frames: keep its window in front
                // and focused for the run, which ends by exiting the app.
                if std::env::var("ZERUS_CLIENT_PERF").is_ok_and(|mode| mode == "auto") {
                    if let Err(error) = window.set_always_on_top(true).and_then(|()| window.set_focus()) {
                        eprintln!("zerus-client perf: could not bring the window to the front: {error}");
                    }
                }
            }
            lifecycle::sync_policy(app.handle());
            Ok(())
        })
        .build(context)
        .expect("Zerus Next failed to start");
    #[cfg(target_os = "macos")]
    lifecycle::prepare(&mut app, menu_bar, opens_sessions);
    app.run(|app, event| {
        lifecycle::on_event(app, &event);
    });
}

/// A `sessions` line from a later start opens and raises the window; `show`
/// changes nothing visible (`main.cpp:424-433`).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn forward_activations(
    app: tauri::AppHandle,
    activations: std::sync::mpsc::Receiver<instance::Activation>,
) {
    std::thread::Builder::new()
        .name("zerus-client-activations".into())
        .spawn(move || {
            for activation in activations {
                eprintln!("zerus-client: activation {activation:?}");
                if activation == instance::Activation::Sessions {
                    let target = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Err(error) = lifecycle::show_main(&target) {
                            eprintln!("zerus-client: could not open the window: {error}");
                        }
                    });
                }
            }
        })
        .expect("the activation thread starts");
}

/// The commands the frontend may call. The session actions need the core's
/// `hgs` runner, so they exist only with the `live` feature.
#[cfg(feature = "live")]
fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::start,
        commands::next_batch,
        commands::follow,
        commands::unfollow,
        commands::metrics,
        commands::perf_config,
        commands::save_perf,
        commands::perf_log,
        commands::display_hz,
        commands::exit_app,
        commands::perf_away,
        renderer::renderer_frame,
        actions::run_action,
        actions::set_notifications,
        actions::inspect_session,
        actions::pending_read_marks,
        actions::queue_read_marks,
        actions::attention_marks,
        actions::update_attention_marks,
        actions::session_reference,
        actions::copy_command,
        actions::archive_page,
        actions::cancel_archive_pages,
        clipboard::copy_text,
        desk::desk_take_pending,
        screens::screen_visible,
        screens::accounts_view,
        screens::machines_view,
        screens::machine_resolve,
        screens::machine_check,
        screens::projects_view,
    ]
}

/// Without the data source there is no menu bar, so nothing waits for the
/// page; the page asks all the same.
#[cfg(not(feature = "live"))]
#[tauri::command]
fn desk_take_pending() -> serde_json::Value {
    serde_json::json!({ "open": null, "notices": [] })
}

#[cfg(not(feature = "live"))]
fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::start,
        commands::next_batch,
        commands::follow,
        commands::unfollow,
        commands::metrics,
        commands::perf_config,
        commands::save_perf,
        commands::perf_log,
        commands::display_hz,
        commands::exit_app,
        commands::perf_away,
        renderer::renderer_frame,
        clipboard::copy_text,
        desk_take_pending,
    ]
}

/// Creates the main window and applies the platform display-rate preference:
/// macOS before the webview is built, Linux through its public settings API.
pub fn create_main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    // ZERUS_CLIENT_QUERY opens a given state (`select=<session name>`) for
    // headless screenshots and scripted measurements, as the mockup's URL
    // parameters do; ZERUS_CLIENT_PERF=auto adds `perf=auto`.
    let mut query: Vec<String> = std::env::var("ZERUS_CLIENT_QUERY")
        .ok()
        .filter(|q| !q.is_empty())
        .into_iter()
        .collect();
    if let Ok(mode) = std::env::var("ZERUS_CLIENT_PERF") {
        if !mode.is_empty() {
            query.push(format!("perf={mode}"));
        }
    }
    let page = match query.is_empty() {
        true => "index.html".to_owned(),
        false => format!("index.html?{}", query.join("&")),
    };
    #[allow(unused_mut)]
    let mut builder = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App(page.into()))
        .title("Zerus Next")
        .inner_size(1440.0, 900.0)
        .min_inner_size(900.0, 560.0);
    // Page rendering at the display rate, not near 60 fps: the preference is set
    // on the configuration before the webview exists (toggling a live page is
    // unreliable). The measurement's control run keeps WebKit's default.
    #[cfg(target_os = "macos")]
    if !keep_default_preference() {
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            builder =
                builder.with_webview_configuration(platform::macos::webview_configuration(mtm));
        }
    }
    let window = builder.build()?;
    #[cfg(target_os = "linux")]
    if !keep_default_preference() {
        window.with_webview(|webview| {
            if let Err(error) = platform::linux::disable_near_60fps(&webview.inner()) {
                eprintln!("zerus-client: could not disable the near-60 fps preference: {error}");
            }
        })?;
    }
    Ok(window)
}

/// `ZERUS_CLIENT_PERF_PREFERENCE=default` leaves the macOS and Linux WebKit
/// frame-rate preference at its default: the must-fail control run of the measurement.
pub fn keep_default_preference() -> bool {
    std::env::var("ZERUS_CLIENT_PERF_PREFERENCE").is_ok_and(|value| value == "default")
}
