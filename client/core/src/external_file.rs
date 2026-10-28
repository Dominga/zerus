//! Opening files that sessions produced, the way browsers treat downloads,
//! ported from the Qt client's `ExternalFile`. Files from other machines
//! carry the platform's downloaded-file marker (Windows Mark of the Web,
//! macOS quarantine), so the system warns before they run. Types that run
//! programs or scripts are shown in the file manager instead of being
//! opened, on every platform; Linux has no such marker.
//!
//! The open and reveal calls themselves are the caller's ([`open`] takes
//! them), so this module only decides and marks.
use std::path::{Component, Path, PathBuf};

/// Where a file came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    ThisComputer,
    /// Another machine's file: marked as downloaded before it is opened.
    OtherMachine,
}

/// What [`open`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Opened,
    /// It runs programs, so the file manager shows it instead.
    Revealed,
    OpenFailed,
    RevealFailed,
}

/// Types whose default handler runs programs, scripts or installers:
/// Chromium's dangerous download types for each platform, Firefox's
/// executable extensions and the Windows Attachment Manager list.
#[rustfmt::skip]
const RUNNABLE: &[&str] = &[
    // Windows programs, scripts, shortcuts, installers, Office add-ins and mounted images.
    "exe", "com", "scr", "pif", "cpl", "msi", "msp", "mst", "msc", "msu", "msix", "msixbundle", "appx",
    "appxbundle", "application", "appref-ms", "appcontent-ms", "gadget", "xbap", "bat", "cmd", "ps1", "psm1",
    "psd1", "ps1xml", "ps2", "ps2xml", "psc1", "psc2", "msh", "msh1", "msh2", "mshxml", "msh1xml", "msh2xml",
    "vb", "vbs", "vbe", "vbscript", "js", "jse", "wsf", "wsh", "wsc", "ws", "sct", "hta", "htt", "lnk", "url",
    "website", "scf", "shb", "shs", "reg", "rgs", "inf", "inx", "ins", "isp", "isu", "iqy", "job", "chm", "hlp",
    "diagcab", "diagcfg", "diagpkg", "settingcontent-ms", "library-ms", "search-ms", "searchconnector-ms",
    "partial", "paf", "u3p", "spl", "cpi", "xrm-ms", "efi", "iso", "img", "vhd", "vhdx", "xll", "xla", "xlam",
    "ppa", "ppam", "dll", "ocx", "sys", "drv", "vxd", "cab", "cer", "crt", "der", "ade", "adp", "accda",
    "accdb", "accde", "accdr", "mad", "maf", "mag", "mam", "maq", "mar", "mas", "mat", "mau", "mav", "maw",
    "mda", "mdb", "mde", "mdt", "mdw", "mdz",
    // macOS applications, scripts, installers, disk images and file locations.
    "app", "command", "tool", "terminal", "pkg", "mpkg", "workflow", "wflow", "action", "caction", "definition",
    "scpt", "scptd", "applescript", "osas", "osax", "prefpane", "kext", "seplugin", "service", "fileloc",
    "inetloc", "webloc", "afploc", "atloc", "ftploc", "osx", "dylib", "xip", "mobileconfig", "configprofile",
    "internetconnect", "networkconnect", "cpgz", "pax", "dmg", "dmgpart", "cdr", "dart", "dc42", "diskcopy42",
    "dvdr", "imgpart", "ndif", "smi", "sparsebundle", "sparseimage", "toast", "udif",
    // Linux launchers, scripts, packages and images; scripts and Java run anywhere.
    "desktop", "kdelnk", "sh", "bash", "zsh", "csh", "tcsh", "ksh", "fish", "shar", "run", "bin", "out",
    "appimage", "deb", "rpm", "snap", "flatpak", "flatpakref", "flatpakrepo", "pet", "pup", "slp", "so", "apk",
    "crx", "oxt", "jar", "jnlp", "class", "py", "pyw", "pyc", "pyo", "pyd", "pyz", "pyzw", "pl", "rb", "php",
];

/// The name Windows resolves: it drops trailing dots and spaces.
fn resolved_name(name: &str) -> &str {
    name.trim_end_matches(['.', ' '])
}

/// The lower-case type after the last dot of the resolved name, if any.
fn suffix(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = resolved_name(&name);
    name.rfind('.')
        .map(|dot| name[dot + 1..].to_lowercase())
        .unwrap_or_default()
}

/// `QDir::cleanPath`: `.` and `..` resolved by name, no trailing separator.
fn clean(path: &Path) -> PathBuf {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !cleaned.pop() {
                    cleaned.push("..");
                }
            }
            other => cleaned.push(other.as_os_str()),
        }
    }
    cleaned
}

/// Programs and scripts by their first bytes: ELF, a PE image, Mach-O (also
/// Java class files) and a shebang naming an interpreter path. MIME
/// inheritance is not used: shared-mime-info makes JSON and most source
/// files subtypes of application/x-executable.
#[cfg(not(windows))]
fn program_content(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::with_capacity(64);
    if file.by_ref().take(64).read_to_end(&mut head).is_err() {
        return false;
    }
    const MAGIC: [[u8; 4]; 6] = [
        *b"\x7fELF",
        [0xfe, 0xed, 0xfa, 0xce],
        [0xfe, 0xed, 0xfa, 0xcf],
        [0xce, 0xfa, 0xed, 0xfe],
        [0xcf, 0xfa, 0xed, 0xfe],
        [0xca, 0xfe, 0xba, 0xbe],
    ];
    if head.len() >= 4 && MAGIC.iter().any(|magic| head[..4] == *magic) {
        return true;
    }
    // "#!/bin/sh" runs; Rust's "#![no_std]" does not.
    if let Some(rest) = head.strip_prefix(b"#!") {
        let rest = &rest[..rest.len().min(14)];
        if rest.iter().find(|byte| !matches!(byte, b' ' | b'\t')) == Some(&b'/') {
            return true;
        }
    }
    // A PE image points from offset 0x3c to its "PE\0\0" header; text may
    // start with MZ.
    if !head.starts_with(b"MZ") || head.len() < 64 {
        return false;
    }
    let offset = u32::from_le_bytes([head[60], head[61], head[62], head[63]]);
    let mut signature = [0u8; 4];
    offset < 4096
        && file.seek(SeekFrom::Start(u64::from(offset))).is_ok()
        && file.read_exact(&mut signature).is_ok()
        && signature == *b"PE\0\0"
}

/// A macOS bundle: an .app, an installer or a plug-in folder.
#[cfg(target_os = "macos")]
fn bundle(path: &Path, suffix: &str) -> bool {
    #[rustfmt::skip]
    const PACKAGES: [&str; 17] = [
        "app", "pkg", "mpkg", "prefpane", "kext", "workflow", "action", "scptd", "saver", "plugin", "bundle",
        "appex", "xpc", "osax", "qlgenerator", "mdimporter", "service",
    ];
    PACKAGES.contains(&suffix) || path.join("Contents").join("Info.plist").is_file()
}

#[cfg(windows)]
fn dangerous_on_windows(suffix: &str) -> bool {
    use windows_sys::Win32::UI::Shell::AssocIsDangerous;
    let wide: Vec<u16> = format!(".{suffix}").encode_utf16().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated wide string alive for the call.
    unsafe { AssocIsDangerous(wide.as_ptr()) != 0 }
}

fn classify(path: &Path) -> bool {
    let suffix = suffix(path);
    // Opening a folder shows it, except a macOS bundle.
    if path.is_dir() {
        #[cfg(target_os = "macos")]
        return bundle(path, &suffix);
        #[cfg(not(target_os = "macos"))]
        return false;
    }
    if !suffix.is_empty() && RUNNABLE.contains(&suffix.as_str()) {
        return true;
    }
    #[cfg(windows)]
    {
        // ShellExecute may resolve a name without a type to name.exe or name.bat.
        suffix.is_empty() || dangerous_on_windows(&suffix)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(metadata) = std::fs::metadata(path) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        // Launchers run an executable file with no type of its own; a
        // document type wins.
        if suffix.is_empty() && metadata.permissions().mode() & 0o111 != 0 {
            return true;
        }
        program_content(path)
    }
}

/// Whether the system's default handler for this file can run programs or
/// scripts. What the opener launches is judged: "App.app/." and a link
/// named Report.pdf both reach their target.
pub fn runs_programs(path: &Path) -> bool {
    let path = clean(path);
    if classify(&path) {
        return true;
    }
    let link =
        std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink());
    link && std::fs::canonicalize(&path).is_ok_and(|target| classify(&target))
}

/// The `\\?\` form reaches paths longer than MAX_PATH; it needs a clean
/// absolute path.
#[cfg(windows)]
fn long_path(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let absolute = clean(&absolute);
    let name = absolute
        .file_name()
        .map(|name| resolved_name(&name.to_string_lossy()).to_owned())
        .unwrap_or_default();
    let native = absolute
        .with_file_name(name)
        .to_string_lossy()
        .replace('/', "\\");
    let long = if native.starts_with("\\\\?\\") {
        native
    } else if let Some(share) = native.strip_prefix("\\\\") {
        format!("\\\\?\\UNC\\{share}")
    } else {
        format!("\\\\?\\{native}")
    };
    PathBuf::from(long)
}

#[cfg(windows)]
fn zone_stream(path: &Path) -> PathBuf {
    let mut stream = long_path(path).into_os_string();
    stream.push(":Zone.Identifier");
    PathBuf::from(stream)
}

/// Whether an administrator asked Windows not to keep zone information.
#[cfg(windows)]
fn zones_disabled() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Attachments");
    let name = wide("SaveZoneInformation");
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: a DWORD read into a local of the size given.
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            std::ptr::from_mut(&mut value).cast(),
            &mut size,
        )
    } == 0;
    found && value == 1
}

/// Adds the downloaded-file marker; false where the platform or file
/// system has none. An existing macOS quarantine is kept, as it may record
/// the user's approval.
pub fn mark_downloaded(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::io::Write;
        if zones_disabled() {
            return false;
        }
        // Mark of the Web for the Internet zone, as browsers write it.
        const ZONE: &[u8] = b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=about:internet\r\n";
        let Ok(mut stream) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(zone_stream(path))
        else {
            return false;
        };
        stream.write_all(ZONE).is_ok() && stream.set_len(ZONE.len() as u64).is_ok()
    }
    #[cfg(target_os = "macos")]
    {
        macos::quarantine(path)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSDictionary, NSString, NSURLQuarantinePropertiesKey, NSURL};
    use std::path::Path;

    /// Quarantines the file as an "other download", the way Qt's
    /// CFURLSetResourcePropertyForKey call does; keeps an existing one.
    pub(super) fn quarantine(path: &Path) -> bool {
        let Ok(absolute) = std::path::absolute(path) else {
            return false;
        };
        let Some(text) = absolute.to_str() else {
            return false;
        };
        let url = NSURL::fileURLWithPath(&NSString::from_str(text));
        // SAFETY: AppKit's constant key; reading and writing a resource
        // value of a file URL.
        unsafe {
            let key = NSURLQuarantinePropertiesKey;
            let mut current: Option<Retained<AnyObject>> = None;
            if url.getResourceValue_forKey_error(&mut current, key).is_ok() && current.is_some() {
                return true;
            }
            // kLSQuarantineTypeKey and kLSQuarantineTypeOtherDownload, by value.
            let properties = NSDictionary::<NSString, AnyObject>::from_retained_objects(
                &[&*NSString::from_str("LSQuarantineType")],
                &[Retained::<AnyObject>::from(NSString::from_str(
                    "LSQuarantineTypeOtherDownload",
                ))],
            );
            url.setResourceValue_forKey_error(Some(&properties), key)
                .is_ok()
        }
    }
}

/// The raw marker (the Zone.Identifier stream or the quarantine
/// attribute), or empty; at most 1 KiB.
pub fn download_marker(path: &Path) -> Vec<u8> {
    #[cfg(windows)]
    {
        use std::io::Read;
        let mut marker = Vec::new();
        match std::fs::File::open(zone_stream(path)) {
            Ok(file) => {
                let _ = file.take(1024).read_to_end(&mut marker);
                marker
            }
            Err(_) => marker,
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(name) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return Vec::new();
        };
        let mut buffer = [0u8; 1024];
        // SAFETY: reads at most the buffer's size into it.
        let size = unsafe {
            libc::getxattr(
                name.as_ptr(),
                c"com.apple.quarantine".as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
                0,
            )
        };
        if size > 0 {
            buffer[..size as usize].to_vec()
        } else {
            Vec::new()
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
        Vec::new()
    }
}

/// Opens a file as Qt does: another machine's file is marked first (again
/// on each open, as Windows may clear it after a reputation check), and a
/// file that runs programs is revealed instead. `opener` and `revealer` are
/// the system calls.
pub fn open(
    path: &Path,
    origin: Origin,
    opener: impl FnOnce(&Path) -> bool,
    revealer: impl FnOnce(&Path) -> bool,
) -> Outcome {
    if origin == Origin::OtherMachine {
        mark_downloaded(path);
    }
    if runs_programs(path) {
        return if revealer(path) {
            Outcome::Revealed
        } else {
            Outcome::RevealFailed
        };
    }
    if opener(path) {
        Outcome::Opened
    } else {
        Outcome::OpenFailed
    }
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The notice after a program type was shown in the file manager.
pub fn revealed_notice(path: &Path) -> String {
    format!(
        "{} can run programs, so it is shown in the file manager instead of being opened.",
        name(path)
    )
}

/// The notice when the file manager could not show a program type.
pub fn reveal_failed_notice(path: &Path) -> String {
    let native = if cfg!(windows) {
        path.to_string_lossy().replace('/', "\\")
    } else {
        path.to_string_lossy().into_owned()
    };
    format!(
        "{} can run programs, and the file manager could not show it. Its copy is at {native}.",
        name(path)
    )
}
