//! Synthetic animated window for GTK resize/pacing regressions. Uses the
//! production renderer, window constructor and presentation, without the GUI
//! entry point, tray, fleet, agents or services. The bundle ID stays canonical.
#[cfg(all(target_os = "linux", feature = "live"))]
mod fixture {
    use gtk::{gdk::prelude::*, prelude::*};
    use serde_json::{json, Value};
    use std::{
        borrow::Cow,
        cell::Cell,
        path::PathBuf,
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
        fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
            match key.as_ref().trim_start_matches('/') {
                "index.html" => Some(Cow::Borrowed(br#"<!doctype html><html><head><meta charset="utf-8"><style>
html,body{width:100%;height:100%;margin:0;background:#15232f;color:white}
body:after{content:'';position:fixed;right:0;bottom:0;width:20px;height:20px;background:#20c060}
@keyframes pulse{to{transform:translateX(200px)}}
#clock{display:inline-block;animation:pulse 1s infinite alternate;background:#6050c0}
#scroll{position:absolute;left:10px;top:50px;width:80%;height:70%;overflow:auto}
</style></head><body><input aria-label="Private test input" value="Isolated resize fixture"><span id="clock"></span><div id="scroll"></div><script src="/fixture.js"></script></body></html>"#)),
                "fixture.js" => Some(Cow::Borrowed(br#"let sequence=0;
function report(phase,width=innerWidth,height=innerHeight){window.__TAURI_INTERNALS__.invoke('viewport_observed',{phase,width,height,sequence:++sequence});}
addEventListener('load',()=>{
 report('load');window.__TAURI_INTERNALS__.invoke('window_theme',{generation:window.__ZERUS_WINDOW_THEME__,revision:1,background:[21,35,47]});
 const scroll=document.querySelector('#scroll');scroll.innerHTML=Array.from({length:1000},(_,n)=>`<p style="background:${n%2?'#243b55':'#34654b'}">Synthetic scroll row ${n}</p>`).join('');
 let frames=0,last=performance.now();function frame(now){scroll.scrollTop=(scroll.scrollTop+3)%(scroll.scrollHeight-scroll.clientHeight);frames++;if(now-last>=1000){report('raf_rate',Math.round(frames*1000/(now-last)),1000);frames=0;last=now;}requestAnimationFrame(frame)}requestAnimationFrame(frame);
 let ticks=0;setInterval(()=>{document.querySelector('#clock').textContent=String(++ticks);report('tick');},200);
});
addEventListener('resize',()=>{report('dom_resize');requestAnimationFrame(()=>requestAnimationFrame(()=>report('dom_frame')));});"#)),
                _ => None,
            }
        }
        fn iter(&self) -> Box<AssetsIter<'_>> {
            Box::new(std::iter::empty())
        }
        fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
            Box::new(std::iter::empty())
        }
    }

    struct Trace {
        at: Instant,
        rows: Mutex<Vec<Value>>,
    }
    impl Trace {
        fn add(&self, phase: &str, width: i32, height: i32) {
            let mut rows = self.rows.lock().unwrap();
            if rows.len() < 4096 {
                rows.push(json!({"us":self.at.elapsed().as_micros(),"phase":phase,"width":width,"height":height}));
            }
        }
    }
    #[tauri::command]
    fn viewport_observed(
        trace: tauri::State<'_, Arc<Trace>>,
        phase: String,
        width: i32,
        height: i32,
        sequence: u64,
    ) {
        if matches!(
            phase.as_str(),
            "load" | "dom_resize" | "dom_frame" | "tick" | "raf_rate"
        ) && sequence < 10000
            && (1..=10000).contains(&width)
            && (1..=10000).contains(&height)
        {
            trace.add(&phase, width, height);
        }
    }

    pub fn run(output: PathBuf) {
        zerus_client_lib::renderer::choose();
        std::fs::create_dir(&output).unwrap();
        let mut context = tauri::generate_context!();
        context.config_mut().identifier = "com.hgdev.hgs-tray".into();
        context.config_mut().build.dev_url = None;
        context.set_assets(Box::new(Assets));
        let trace = Arc::new(Trace {
            at: Instant::now(),
            rows: Mutex::new(Vec::new()),
        });
        let hooks = trace.clone();
        let app = tauri::Builder::default()
            .manage(trace.clone())
            .manage(zerus_client_lib::window_theme::Theme::default())
            .invoke_handler(tauri::generate_handler![
                viewport_observed,
                zerus_client_lib::window_theme::window_theme
            ])
            .setup(move |app| {
                app.manage(zerus_client_lib::settings::Store(
                    zerus_client_core::settings::Settings::open(output.join("settings.json")),
                ));
                let window = zerus_client_lib::create_main_window(app.handle())?;
                window.set_title("Zerus isolated resize fixture")?;
                window.set_size(tauri::LogicalSize::new(1083., 818.))?;
                let sampled = window.clone();
                let app = app.handle().clone();
                window.with_webview(move |platform| {
                    let view = platform.inner();
                    let top = view
                        .toplevel()
                        .unwrap()
                        .downcast::<gtk::ApplicationWindow>()
                        .unwrap();
                    std::fs::write(
                        output.join("backend.txt"),
                        gtk::gdk::Display::default().unwrap().type_().name(),
                    )
                    .unwrap();
                    let observed = hooks.clone();
                    top.connect_configure_event(move |_, event| {
                        let (w, h) = event.size();
                        observed.add("configure", w as i32, h as i32);
                        false
                    });
                    let observed = hooks.clone();
                    top.connect_size_allocate(move |_, a| {
                        observed.add("window_allocate", a.width(), a.height())
                    });
                    let observed = hooks.clone();
                    view.connect_size_allocate(move |_, a| {
                        observed.add("view_allocate", a.width(), a.height())
                    });
                    let draws = Rc::new(Cell::new(0));
                    let count = draws.clone();
                    view.connect_draw(move |_, _| {
                        count.set(count.get() + 1);
                        gtk::glib::Propagation::Proceed
                    });
                    let layouts = Rc::new(Cell::new(0));
                    let after_paints = Rc::new(Cell::new(0));
                    let layout_count = layouts.clone();
                    let after_count = after_paints.clone();
                    let armed = Cell::new(false);
                    view.connect_map(move |view| {
                        if armed.get() {
                            return;
                        }
                        if let Some(clock) = view.frame_clock() {
                            armed.set(true);
                            let count = layout_count.clone();
                            clock.connect_layout(move |_| count.set(count.get() + 1));
                            let count = after_count.clone();
                            clock.connect_after_paint(move |_| count.set(count.get() + 1));
                        }
                    });
                    let rates = hooks.clone();
                    let mut previous_rates = (0, 0, 0);
                    gtk::glib::timeout_add_local(Duration::from_secs(1), move || {
                        let current = (draws.get(), layouts.get(), after_paints.get());
                        rates.add("paint_rate", current.0 - previous_rates.0, 1000);
                        rates.add("layout_rate", current.1 - previous_rates.1, 1000);
                        rates.add("after_paint_rate", current.2 - previous_rates.2, 1000);
                        previous_rates = current;
                        gtk::glib::ControlFlow::Continue
                    });
                    // Dispatch native requests only in a later GLib callback,
                    // after Wry releases its with_webview/window-ID mutex.
                    let at = Instant::now();
                    let delay = std::env::var("ZERUS_RESIZE_DELAY_MS")
                        .ok()
                        .and_then(|v| v.parse::<u64>().ok())
                        .filter(|v| (1500..=10000).contains(v))
                        .unwrap_or(1500);
                    let steps = std::env::var("ZERUS_RESIZE_STEPS")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .filter(|v| (1..=6).contains(v))
                        .unwrap_or(6);
                    let mut step = 0;
                    gtk::glib::timeout_add_local(Duration::from_millis(10), move || {
                        if step < steps
                            && at.elapsed() >= Duration::from_millis(delay + step as u64 * 2000)
                        {
                            let command = match step {
                                0 => json!({"step":step,"width":1500,"height":1000}),
                                1 => json!({"step":step,"width":900,"height":600}),
                                2 => json!({"step":step,"maximize":true}),
                                3 => json!({"step":step,"maximize":false}),
                                4 => json!({"step":step,"width":1300,"height":850}),
                                _ => json!({"step":step,"width":1000,"height":650}),
                            };
                            hooks.add(
                                "request",
                                command["width"].as_i64().unwrap_or(0) as i32,
                                command["height"].as_i64().unwrap_or(0) as i32,
                            );
                            if std::env::var_os("ZERUS_RESIZE_EXTERNAL").is_some() {
                                std::fs::write(output.join("command.tmp"), command.to_string())
                                    .unwrap();
                                std::fs::rename(
                                    output.join("command.tmp"),
                                    output.join("command.json"),
                                )
                                .unwrap();
                            } else if let Some(maximize) = command["maximize"].as_bool() {
                                if maximize {
                                    sampled.maximize().unwrap()
                                } else {
                                    sampled.unmaximize().unwrap()
                                }
                            } else {
                                sampled
                                    .set_size(tauri::LogicalSize::new(
                                        command["width"].as_f64().unwrap(),
                                        command["height"].as_f64().unwrap(),
                                    ))
                                    .unwrap();
                            }
                            step += 1;
                        }
                        if at.elapsed()
                            > Duration::from_millis(delay + (steps as u64 - 1) * 2000 + 2500)
                        {
                            std::fs::write(
                                output.join("trace.json"),
                                serde_json::to_vec_pretty(&*hooks.rows.lock().unwrap()).unwrap(),
                            )
                            .unwrap();
                            app.exit(0);
                            gtk::glib::ControlFlow::Break
                        } else {
                            gtk::glib::ControlFlow::Continue
                        }
                    });
                })?;
                zerus_client_lib::lifecycle::request_main(window.app_handle()).unwrap();
                Ok(())
            })
            .build(context)
            .unwrap();
        app.run(|_, _| {});
        assert!(!trace.rows.lock().unwrap().is_empty());
    }
}
#[cfg(all(target_os = "linux", feature = "live"))]
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--build-info"] {
        println!("{{\"ZerusGUI\":\"tauri\",\"localWebKitChildEnvironment\":1,\"CFBundleIdentifier\":\"com.hgdev.hgs-tray\",\"fixture\":true}}");
        return;
    }
    assert_eq!(
        args.len(),
        1,
        "Usage: resize_fixture FRESH_OUTPUT_DIRECTORY"
    );
    fixture::run(args[0].clone().into());
}
#[cfg(not(all(target_os = "linux", feature = "live")))]
fn main() {
    eprintln!("The resize fixture requires Linux and the live feature");
}
