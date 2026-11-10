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

fn installation(image: &str) -> Installation {
    Installation {
        configuration: "a".repeat(64),
        sid: "S-1-5-21-1000".into(),
        session: 1,
        image: image.into(),
        command: super::super::registration::expected_command(image, r"C:\c", r"C:\s").unwrap(),
    }
}
fn registry(command: Option<String>) -> BTreeMap<(String, String), String> {
    let class = format!(r"Software\Classes\CLSID\{}", activation::CLSID_TEXT);
    let mut values = BTreeMap::new();
    for (key, name, value) in [
        (APPLICATION_KEY, ABI_VALUE, wire::ABI.to_owned()),
        (APPLICATION_KEY, CONFIGURATION_VALUE, "a".repeat(64)),
        (
            APPLICATION_KEY,
            "CustomActivator",
            activation::CLSID_TEXT.to_owned(),
        ),
        (APPLICATION_KEY, "DisplayName", "Zerus".to_owned()),
        (class.as_str(), ABI_VALUE, wire::ABI.to_owned()),
        (class.as_str(), CONFIGURATION_VALUE, "a".repeat(64)),
    ] {
        values.insert((key.to_owned(), name.to_owned()), value);
    }
    if let Some(command) = command {
        values.insert((format!(r"{class}\LocalServer32"), String::new()), command);
    }
    values
}
fn verdict(
    installation: &Installation,
    values: &BTreeMap<(String, String), String>,
) -> Result<(), &'static str> {
    installation.registered(|key, name| values.get(&(key.to_owned(), name.to_owned())).cloned())
}

#[test]
fn only_the_exact_registration_is_current_and_other_image_is_a_closed_diagnostic() {
    let own = installation(r"C:\Users\Example\.local\bin\hgs.exe");
    let other = installation(r"C:\Other copy\hgs.exe");
    assert_eq!(verdict(&own, &registry(Some(own.command.clone()))), Ok(()));
    assert_eq!(
        verdict(&own, &registry(Some(other.command.clone()))),
        Err("notification_registration_other_image")
    );
    let unavailable = Err("notification_registration_unavailable");
    let same_image_wrong_config =
        super::super::registration::expected_command(&own.image, r"C:\d", r"C:\s").unwrap();
    for (case, values) in [
        ("absent", BTreeMap::new()),
        ("no command", registry(None)),
        ("malformed command", registry(Some("hgs.exe".into()))),
        (
            "same image, wrong config",
            registry(Some(same_image_wrong_config)),
        ),
        (
            "extra argument",
            registry(Some(format!("{} --extra", own.command))),
        ),
        (
            "other image, extra argument",
            registry(Some(format!("{} --extra", other.command))),
        ),
    ] {
        assert_eq!(verdict(&own, &values), unavailable, "{case}");
    }
    // Another image under a foreign ABI or configuration is not the
    // diagnostic either; every non-command value must be exact first.
    let class = format!(r"Software\Classes\CLSID\{}", activation::CLSID_TEXT);
    for (key, name, value) in [
        (APPLICATION_KEY.to_owned(), ABI_VALUE, "foreign-abi"),
        (class.clone(), CONFIGURATION_VALUE, "b"),
        (APPLICATION_KEY.to_owned(), "DisplayName", "Other"),
    ] {
        for command in [own.command.clone(), other.command.clone()] {
            let mut values = registry(Some(command));
            values.insert((key.clone(), name.to_owned()), value.to_owned());
            assert_eq!(verdict(&own, &values), unavailable, "{key} {name}");
        }
    }
    // A registration changed after selection is refused on the next check.
    let mut values = registry(Some(own.command.clone()));
    assert_eq!(verdict(&own, &values), Ok(()));
    values.insert(
        (format!(r"{class}\LocalServer32"), String::new()),
        other.command.clone(),
    );
    assert!(verdict(&own, &values).is_err());
}

const SELECTION_CHILD: &str = "HGS_TEST_NOTIFICATION_SELECTION_CHILD";
const SELECTION: &str =
    "desktop::notifications::windows::native::tests::the_selected_command_is_this_image_and_configuration_whatever_home_or_executable_say";

#[test]
fn the_selected_command_is_this_image_and_configuration_whatever_home_or_executable_say() {
    if let Some(root) = std::env::var_os(SELECTION_CHILD) {
        // Child: HOME, USERPROFILE and HGS_EXECUTABLE all name another place.
        let root = std::path::PathBuf::from(root);
        let config = Config {
            home: root.join("home").to_str().unwrap().into(),
            dir: root.join("config dir ü"),
            state: root.join("state dir ü"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        let image = platform::canonical(std::env::current_exe().unwrap()).unwrap();
        let (selected, command) = selected_command(&config).unwrap();
        assert_eq!(selected, image.to_str().unwrap());
        assert_eq!(
            command,
            super::super::registration::expected_command(
                &selected,
                config.dir.to_str().unwrap(),
                config.state.to_str().unwrap()
            )
            .unwrap()
        );
        return;
    }
    // A link of this test image in a folder with a space and non-ASCII text.
    let here = std::env::current_exe().unwrap();
    let temp = tempfile::tempdir_in(here.parent().unwrap()).unwrap();
    let folder = temp.path().join("Zerus тест image");
    std::fs::create_dir_all(&folder).unwrap();
    let image = folder.join("hgs selection.exe");
    if std::fs::hard_link(&here, &image).is_err() {
        std::fs::copy(&here, &image).unwrap();
    }
    let elsewhere = temp.path().join("elsewhere");
    let output = std::process::Command::new(&image)
        .args([SELECTION, "--exact", "--nocapture", "--test-threads=1"])
        .env(SELECTION_CHILD, temp.path())
        .env("HOME", &elsewhere)
        .env("USERPROFILE", &elsewhere)
        .env("HGS_EXECUTABLE", elsewhere.join(r".local\bin\hgs.exe"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn relative_configuration_formats_exactly_as_the_registration_writer() {
    // Config::load keeps a relative HGS_CONFIG_DIR; the writer makes the
    // definition directories absolute, and the selector must match it.
    let config = Config {
        home: std::env::temp_dir()
            .join("selection home")
            .to_str()
            .unwrap()
            .into(),
        dir: "relative config dir ü".into(),
        state: "relative state dir".into(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    let location = Location::new(&config).unwrap();
    let plan = crate::desktop::registration::windows_plan(&config, &location).unwrap();
    let (image, command) = selected_command(&config).unwrap();
    assert_eq!(
        command,
        super::super::registration::expected_command(
            &image,
            plan["argv"][4].as_str().unwrap(),
            plan["argv"][6].as_str().unwrap()
        )
        .unwrap()
    );
    let here = std::env::current_dir().unwrap();
    assert_eq!(
        plan["argv"][4],
        here.join("relative config dir ü").to_str().unwrap()
    );
    assert_eq!(
        plan["argv"][6],
        here.join("relative state dir").to_str().unwrap()
    );
}

#[test]
fn the_last_native_object_is_released_inside_the_apartment_before_it_is_left() {
    use std::{cell::RefCell, rc::Rc};
    struct Marker(Rc<RefCell<Vec<&'static str>>>);
    impl Drop for Marker {
        fn drop(&mut self) {
            self.0.borrow_mut().push("released");
        }
    }
    let order = Rc::new(RefCell::new(Vec::new()));
    let mut owned = Some(Marker(order.clone()));
    release_then_uninitialize(&mut owned, || order.borrow_mut().push("uninitialized"));
    assert!(owned.is_none());
    assert_eq!(*order.borrow(), ["released", "uninitialized"]);
    // An already released object leaves only the uninitialize step.
    let mut none: Option<Marker> = None;
    release_then_uninitialize(&mut none, || order.borrow_mut().push("uninitialized"));
    assert_eq!(
        *order.borrow(),
        ["released", "uninitialized", "uninitialized"]
    );
}

#[test]
fn a_real_notifier_released_before_the_apartment_ends_leaves_the_process_intact() {
    // Task #45 (team ede724c4): this thread's RoUninitialize ends the
    // process's MTA when it created it, and a WinRT object another test made
    // on a thread without an apartment (windows-rs then joins that implicit
    // MTA without holding it) faults on its next call: the full hgs test run
    // ended with 0xC0000005 when the order fell that way, and a test that
    // made that order crashed every time. The real object runs in a child
    // process of its own, where nothing else holds the MTA.
    const CHILD: &str = "HGS_TEST_MTA_NOTIFIER_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "desktop::notifications::windows::native::tests::a_real_notifier_released_before_the_apartment_ends_leaves_the_process_intact",
                "--exact",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{:?}\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    // The actual WinRT object in its own MTA thread, released by the same
    // helper the backend uses; the reversed order faults (zerus-1an7q).
    std::thread::spawn(|| {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.unwrap();
        let mut notifier = Some(
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(wire::APPLICATION))
                .unwrap(),
        );
        release_then_uninitialize(&mut notifier, || unsafe { RoUninitialize() });
        assert!(notifier.is_none());
    })
    .join()
    .unwrap();
}

#[test]
fn identity_bootstrap_toast_is_suppressed_expiring_and_has_only_its_exact_identity() {
    let ticks = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            / 100
            + 116_444_736_000_000_000
    };
    let before = ticks();
    let value = bootstrap_toast("b0123456789abcde").unwrap();
    let after = ticks();
    assert!(value.SuppressPopup().unwrap());
    assert_eq!(value.Tag().unwrap(), "b0123456789abcde");
    assert_eq!(value.Group().unwrap(), BOOTSTRAP_GROUP);
    let expiry = value
        .ExpirationTime()
        .unwrap()
        .Value()
        .unwrap()
        .UniversalTime as u128;
    assert!(
        (before + BOOTSTRAP_SECONDS * 10_000_000..=after + BOOTSTRAP_SECONDS * 10_000_000)
            .contains(&expiry)
    );
    assert_eq!(texts(&value), ["Zerus"]);
    let document = value.Content().unwrap();
    assert_eq!(
        document
            .DocumentElement()
            .unwrap()
            .GetAttribute(&HSTRING::from("launch"))
            .unwrap(),
        ""
    );
    assert_eq!(
        document
            .GetElementsByTagName(&HSTRING::from("action"))
            .unwrap()
            .Length()
            .unwrap(),
        0
    );
}

