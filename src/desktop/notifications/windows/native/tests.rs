//! Real WinRT/registry calls with no notifier, Show or installed registration.
use super::*;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn texts(toast: &ToastNotification) -> Vec<String> {
    let nodes = toast
        .Content()
        .unwrap()
        .GetElementsByTagName(&HSTRING::from("text"))
        .unwrap();
    (0..nodes.Length().unwrap())
        .map(|i| nodes.Item(i).unwrap().InnerText().unwrap().to_string())
        .collect()
}

#[test]
fn toast_document_round_trips_untrusted_text_and_exact_identity() {
    let request = id();
    let title = r#"<b>"Fish & Chips"</b> 'quoted' Ünïcødé"#;
    let body = "</text></binding></visual><actions><action content=\"x\" arguments=\"y\"/></actions><visual><binding template=\"ToastGeneric\"><text>";
    let toast = toast(
        &request,
        title,
        body,
        "0123456789abcdef",
        "fedcba9876543210",
    )
    .unwrap();
    assert_eq!(toast.Tag().unwrap(), "0123456789abcdef");
    assert_eq!(toast.Group().unwrap(), "fedcba9876543210");
    let document = toast.Content().unwrap();
    let root = document.DocumentElement().unwrap();
    assert_eq!(root.TagName().unwrap(), "toast");
    assert_eq!(
        root.GetAttribute(&HSTRING::from("launch")).unwrap(),
        format!("{}{request}", super::super::callbacks::PREFIX).as_str()
    );
    // Untrusted text stays content: exactly two text nodes and no actions.
    assert_eq!(texts(&toast), [title, body]);
    assert_eq!(
        document
            .GetElementsByTagName(&HSTRING::from("action"))
            .unwrap()
            .Length()
            .unwrap(),
        0
    );
}

#[test]
fn malformed_text_refuses_before_any_toast_object_exists() {
    // XML 1.0 forbids C0 controls; the document fails to load, so no
    // ToastNotification, delegate or Show can follow.
    for text in ["bell \u{7}", "nul \u{0} inside"] {
        assert!(matches!(
            toast(&id(), text, "body", "0123456789abcdef", "fedcba9876543210"),
            Err(Refusal::InvalidCommand)
        ));
        assert!(matches!(
            toast(&id(), "title", text, "0123456789abcdef", "fedcba9876543210"),
            Err(Refusal::InvalidCommand)
        ));
    }
}

/// A private HKCU subkey, removed with its values on drop.
struct Key(String);
impl Key {
    fn new() -> Self {
        let path = format!(r"Software\ZerusTests\Notifications\{}", id());
        let mut key = std::ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(&path).as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_VOLATILE,
                KEY_SET_VALUE,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        unsafe { RegCloseKey(key) };
        Self(path)
    }
    fn set(&self, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) {
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                wide(&self.0).as_ptr(),
                wide(name).as_ptr(),
                kind,
                data.as_ptr().cast(),
                data.len() as u32,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
    }
    fn text(&self, name: &str, kind: REG_VALUE_TYPE, value: &[u16]) {
        let bytes: Vec<u8> = value.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        self.set(name, kind, &bytes);
    }
}
impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.0).as_ptr()) };
        unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(&self.0).as_ptr()) };
    }
}

#[test]
fn registration_reader_accepts_only_one_bounded_plain_string() {
    let key = Key::new();
    key.text("plain", REG_SZ, &wide("com.hgdev.hgs-tray"));
    key.text("expand", REG_EXPAND_SZ, &wide("%TEMP%"));
    key.text(
        "inner_nul",
        REG_SZ,
        &[u16::from(b'a'), 0, u16::from(b'b'), 0],
    );
    key.text("large", REG_SZ, &wide(&"x".repeat(40_000)));
    key.set("number", REG_DWORD, &1u32.to_le_bytes());
    assert_eq!(
        registry_string(&key.0, "plain").as_deref(),
        Some("com.hgdev.hgs-tray")
    );
    // No expansion, type coercion, truncation or partial value is accepted.
    for name in ["expand", "inner_nul", "large", "number", "missing"] {
        assert_eq!(registry_string(&key.0, name), None, "{name}");
    }
    let path = key.0.clone();
    drop(key);
    assert_eq!(registry_string(&path, "plain"), None);
}
