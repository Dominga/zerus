use std::ffi::{c_char, c_int, c_void};

use gtk::glib::translate::ToGlibPtr;
use webkit2gtk::{WebView, WebViewExt};

const NEAR_60_FPS: &[u8] = b"PreferPageRenderingUpdatesNear60FPS\0";

type GetAllFeatures = unsafe extern "C" fn() -> *mut c_void;
type FindFeature = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type SetFeatureEnabled = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int);
type GetFeatureEnabled = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type UnrefFeatureList = unsafe extern "C" fn(*mut c_void);

struct FeatureApi {
    get_all: GetAllFeatures,
    find: FindFeature,
    set_enabled: SetFeatureEnabled,
    get_enabled: GetFeatureEnabled,
    unref_list: UnrefFeatureList,
}

fn symbol<T>(name: &[u8]) -> Option<T> {
    let pointer = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr().cast()) };
    if pointer.is_null() {
        return None;
    }
    // The WebKit C API uses data and function pointers of the native platform size.
    Some(unsafe { std::mem::transmute_copy(&pointer) })
}

impl FeatureApi {
    fn load() -> Option<Self> {
        Some(Self {
            get_all: symbol(b"webkit_settings_get_all_features\0")?,
            find: symbol(b"webkit_feature_list_find\0")?,
            set_enabled: symbol(b"webkit_settings_set_feature_enabled\0")?,
            get_enabled: symbol(b"webkit_settings_get_feature_enabled\0")?,
            unref_list: symbol(b"webkit_feature_list_unref\0")?,
        })
    }
}

pub fn disable_near_60fps(view: &WebView) -> Result<(), String> {
    let api =
        FeatureApi::load().ok_or_else(|| "WebKit feature-list API is unavailable".to_owned())?;
    let settings = view
        .settings()
        .ok_or_else(|| "the web view has no settings".to_owned())?;
    let settings: *mut webkit2gtk::ffi::WebKitSettings = settings.to_glib_none().0;
    let features = unsafe { (api.get_all)() };
    if features.is_null() {
        return Err("WebKit returned no feature list".to_owned());
    }

    let result = (|| {
        let feature = unsafe { (api.find)(features, NEAR_60_FPS.as_ptr().cast()) };
        if feature.is_null() {
            return Err("WebKit has no PreferPageRenderingUpdatesNear60FPS feature".to_owned());
        }
        unsafe {
            (api.set_enabled)(settings.cast(), feature, 0);
        }
        if unsafe { (api.get_enabled)(settings.cast(), feature) } != 0 {
            return Err("WebKit kept PreferPageRenderingUpdatesNear60FPS enabled".to_owned());
        }
        Ok(())
    })();
    unsafe { (api.unref_list)(features) };
    result
}

/// Runs `beat` on the main thread every `every` (the watchdog's heartbeat): a
/// GLib timeout on the default main context, which nested dialog loops keep
/// dispatching. Call on the main thread.
pub fn heartbeat(every: std::time::Duration, beat: impl Fn() + 'static) {
    gtk::glib::timeout_add_local(every, move || {
        beat();
        gtk::glib::ControlFlow::Continue
    });
}
