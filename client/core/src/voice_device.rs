//! Task 102: Qt's chosen microphone as a name. Qt keeps the device's
//! `QAudioDevice::id()`, which names no device of the client's (cpal lists
//! its own ids), so the settings import keeps the device's name instead and
//! the Voice page picks the listed microphone with that name.
//!
//! On Windows Qt's id is the endpoint id of the multimedia device API,
//! `{0.0.1.00000000}.{GUID}` for a capture device. Windows names the device
//! from two properties of `MMDevices\Audio\Capture\{GUID}\Properties`: its
//! description (`Microphone`) and its interface (`Realtek(R) Audio`), as
//! `Microphone (Realtek(R) Audio)`, the name cpal lists too. Elsewhere, and
//! for an id this computer no longer knows, there is no name.

/// The capture endpoint's GUID in a Qt id, braces included.
pub fn capture_guid(id: &str) -> Option<&str> {
    let (flow, guid) = id.trim().split_once("}.")?;
    // The data flow is the third field: 1 is capture, 0 render.
    if !flow.starts_with("{0.0.1.") {
        return None;
    }
    let ok = guid.len() == 38
        && guid.starts_with('{')
        && guid.ends_with('}')
        && guid[1..37].chars().enumerate().all(|(at, c)| {
            if [8, 13, 18, 23].contains(&at) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    ok.then_some(guid)
}

/// The device's name as Windows shows it: `description (interface)`, or the
/// description alone.
pub fn compose(description: &str, interface: &str) -> Option<String> {
    let description = description.trim();
    let interface = interface.trim();
    if description.is_empty()
        || description.chars().count() > 256
        || interface.chars().count() > 256
    {
        return None;
    }
    Some(if interface.is_empty() {
        description.to_owned()
    } else {
        format!("{description} ({interface})")
    })
}

/// The name of the microphone Qt's id `id` names on this computer, read
/// from the registry (read-only); None when there is none.
pub fn qt_microphone_name(id: &str) -> Option<String> {
    let guid = capture_guid(id)?;
    let properties =
        format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Capture\\{guid}\\Properties");
    let description = registry_text(&properties, "{a45c254e-df1c-4efd-8020-67d146a850e0},2")?;
    let interface =
        registry_text(&properties, "{b3f8fa53-0004-438e-9003-51a46e139bfc},6").unwrap_or_default();
    compose(&description, &interface)
}

#[cfg(windows)]
fn registry_text(key: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let key: Vec<u16> = key.encode_utf16().chain(std::iter::once(0)).collect();
    let value: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let mut text = [0u16; 512];
    let mut size = (text.len() * 2) as u32;
    // SAFETY: the names are NUL-terminated and the buffer is `size` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            text.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let units = &text[..(size as usize / 2).min(text.len())];
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

#[cfg(not(windows))]
fn registry_text(_key: &str, _value: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_qt_capture_id_names_its_endpoint_and_the_name_reads_as_windows_shows_it() {
        let id = "{0.0.1.00000000}.{1b15d461-3242-454f-b577-749d304e90b4}";
        assert_eq!(
            capture_guid(id),
            Some("{1b15d461-3242-454f-b577-749d304e90b4}")
        );
        // A render device, a malformed GUID and an id of another platform name nothing.
        assert_eq!(
            capture_guid("{0.0.0.00000000}.{1b15d461-3242-454f-b577-749d304e90b4}"),
            None
        );
        assert_eq!(
            capture_guid("{0.0.1.00000000}.{1b15d461-3242-454f-b577-749d304e90bZ}"),
            None
        );
        assert_eq!(capture_guid("{0.0.1.00000000}.{1b15d461}"), None);
        assert_eq!(
            capture_guid("alsa_input.pci-0000_00_1f.3.analog-stereo"),
            None
        );
        assert_eq!(capture_guid("AppleHDAEngineInput:1B,0,1,0:1"), None);
        assert_eq!(
            compose("Microphone", "Realtek(R) Audio").as_deref(),
            Some("Microphone (Realtek(R) Audio)")
        );
        assert_eq!(compose("Stereo Mix", "").as_deref(), Some("Stereo Mix"));
        assert_eq!(compose("  ", "Realtek(R) Audio"), None);
        assert_eq!(compose(&"x".repeat(300), ""), None);
        // An unknown endpoint has no name, so nothing is imported for it.
        assert_eq!(
            qt_microphone_name("{0.0.1.00000000}.{00000000-0000-0000-0000-000000000000}"),
            None
        );
        assert_eq!(qt_microphone_name(""), None);
    }
}
