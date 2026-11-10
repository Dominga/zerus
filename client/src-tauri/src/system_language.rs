//! The user's display language, independent of regional number/date formats.

#[cfg(windows)]
pub fn read() -> String {
    use windows_sys::Win32::Globalization::{
        GetUserDefaultUILanguage, LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES,
    };
    let mut buffer = [0u16; 85];
    let length = unsafe {
        LCIDToLocaleName(
            u32::from(GetUserDefaultUILanguage()),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            LOCALE_ALLOW_NEUTRAL_NAMES,
        )
    };
    if length > 1 {
        String::from_utf16_lossy(&buffer[..length as usize - 1])
    } else {
        String::new()
    }
}

#[cfg(target_os = "macos")]
pub fn read() -> String {
    objc2_foundation::NSLocale::preferredLanguages()
        .firstObject()
        .map(|language| language.to_string())
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
pub fn read() -> String {
    gtk::glib::language_names()
        .first()
        .map(ToString::to_string)
        .unwrap_or_default()
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn read() -> String {
    String::new()
}
