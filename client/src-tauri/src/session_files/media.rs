//! Task 248: pictures, animations and videos an agent names, as Qt's
//! SessionsWindow::loadMedia reads them: the reference resolves against the
//! session's folder (`session_file_reference`, the dialog's rules); a file on
//! this computer is read in place, a peer's through the download cache. A
//! preview never reads a file over `PREVIEW_LIMIT` here, nor a peer's video
//! before a click. What the webview may then read is one granted file per
//! token (`zerus-file://localhost/media/{token}`), never a path it names.
use super::*;
use std::collections::VecDeque;

/// Files larger than this are not read for a preview; they still open (Qt `MediaFiles::PreviewLimit`).
pub const PREVIEW_LIMIT: u64 = 64 * 1024 * 1024;
/// Tokens kept at once; the oldest goes first.
const GRANTS: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    None,
    Image,
    Animation,
    Video,
}

/// Qt `MediaFiles::kind`: by the file's extension.
pub fn kind(path: &str) -> Kind {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some((_, suffix)) = name.rsplit_once('.') else {
        return Kind::None;
    };
    match suffix.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => Kind::Image,
        "gif" => Kind::Animation,
        "mp4" | "webm" | "mov" | "mkv" | "m4v" | "avi" => Kind::Video,
        _ => Kind::None,
    }
}

/// The files the webview may read, by token.
#[derive(Default)]
pub struct Grants {
    order: VecDeque<String>,
    paths: HashMap<String, PathBuf>,
}

impl Grants {
    pub fn grant(&mut self, path: PathBuf) -> String {
        if let Some((token, _)) = self.paths.iter().find(|(_, granted)| **granted == path) {
            return token.clone();
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        self.paths.insert(token.clone(), path);
        self.order.push_back(token.clone());
        while self.order.len() > GRANTS {
            if let Some(old) = self.order.pop_front() {
                self.paths.remove(&old);
            }
        }
        token
    }
    pub fn path(&self, token: &str) -> Option<PathBuf> {
        (token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())).then(|| self.paths.get(token).cloned()).flatten()
    }
}

/// One answer: `ready` with a token, `loading` (a `session-media` event
/// follows with the same request), `click` (a peer's video waits for a
/// click) or `error` with its code: `invalid`, `unknown_folder`, `missing`,
/// `too_large`, `offline`, `not_media` or `failed` (with the detail).
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct MediaAnswer {
    pub request: u64,
    pub state: String,
    pub code: String,
    pub detail: String,
    pub token: String,
    pub kind: Kind,
    pub size: u64,
    pub name: String,
}

fn error(request: u64, code: &str, detail: String, kind: Kind, name: String) -> MediaAnswer {
    MediaAnswer { request, state: "error".into(), code: code.into(), detail, kind, name, ..Default::default() }
}

/// What a file on this computer allows (Qt's local branch of loadMedia).
pub fn local_answer(request: u64, path: &Path, open: bool, grants: &Mutex<Grants>) -> MediaAnswer {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let kind = kind(&path.to_string_lossy());
    if kind == Kind::None {
        return error(request, "not_media", String::new(), kind, name);
    }
    let size = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => return error(request, "missing", String::new(), kind, name),
    };
    if !open && size > PREVIEW_LIMIT {
        return error(request, "too_large", String::new(), kind, name);
    }
    let token = lock(grants).grant(path.to_path_buf());
    MediaAnswer { request, state: "ready".into(), token, kind, size, name, ..Default::default() }
}

impl SessionFiles {
    pub(super) fn media(
        self: &Arc<Self>,
        request: u64,
        computer: String,
        href: String,
        cwd: String,
        open: bool,
    ) -> MediaAnswer {
        let remote = !computer.is_empty();
        let style = if remote { Style::Posix } else { paths::build() };
        let target = session_file_reference::parse(&href, style);
        let shown = kind(&target.path);
        let name = download_cache::filename(&target.path);
        if target.path.is_empty() {
            return error(request, "invalid", String::new(), shown, name);
        }
        let home = self.app.path().home_dir().ok();
        let resolved = session_file_reference::resolve(&target, &cwd, if remote { None } else { home.as_deref().and_then(Path::to_str) }, style);
        if resolved.is_empty() {
            return error(request, "unknown_folder", String::new(), shown, name);
        }
        if !remote {
            return local_answer(request, Path::new(&resolved), open, &self.media);
        }
        if shown == Kind::None {
            return error(request, "not_media", String::new(), shown, name);
        }
        if !self.online(&computer) {
            return error(request, "offline", String::new(), shown, name);
        }
        // A preview of a peer's video waits for a click: it would download the whole file.
        if !open && shown == Kind::Video {
            return MediaAnswer { request, state: "click".into(), kind: shown, name, ..Default::default() };
        }
        let me = self.clone();
        let root = crate::files::folder(&self.app);
        let answered = name.clone();
        self.downloads.cached(
            root,
            Source { host: computer, path: resolved },
            false,
            Box::new(move |result| {
                let answer = match result {
                    Ok((path, _)) => {
                        external_file::mark_downloaded(&path);
                        let mut answer = local_answer(request, &path, true, &me.media);
                        answer.name = answered;
                        answer
                    }
                    Err(detail) => error(request, "failed", detail, shown, answered),
                };
                let _ = me.app.emit("session-media", answer);
            }),
        );
        MediaAnswer { request, state: "loading".into(), kind: shown, name, ..Default::default() }
    }

    /// The granted file, for the `zerus-file` media route.
    pub fn media_path(&self, token: &str) -> Option<PathBuf> {
        lock(&self.media).path(token)
    }

    /// Qt's "Open in default app": a granted file only.
    pub(super) fn media_external(self: &Arc<Self>, token: &str, remote: bool) -> Result<(), String> {
        let path = self.media_path(token).ok_or_else(|| "This file is no longer available.".to_string())?;
        if !path.is_file() {
            return Err("This file is no longer available.".into());
        }
        self.open(path, if remote { Origin::OtherMachine } else { Origin::ThisComputer }, false);
        Ok(())
    }
}

#[tauri::command(async)]
pub fn session_media(
    files: tauri::State<'_, Arc<SessionFiles>>,
    request: u64,
    computer: String,
    href: String,
    cwd: String,
    open: bool,
) -> MediaAnswer {
    files.media(request, computer, href, cwd, open)
}

#[tauri::command(async)]
pub fn session_media_external(
    files: tauri::State<'_, Arc<SessionFiles>>,
    token: String,
    remote: bool,
) -> Result<(), String> {
    files.media_external(&token, remote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_qt_media_files() {
        assert_eq!(kind("/work/shot.PNG"), Kind::Image);
        assert_eq!(kind("C:\\work\\a.jpeg"), Kind::Image);
        assert_eq!(kind("spin.gif"), Kind::Animation);
        assert_eq!(kind("/tmp/demo.webm"), Kind::Video);
        assert_eq!(kind("/tmp/notes.md"), Kind::None);
        assert_eq!(kind("/tmp/no-extension"), Kind::None);
        assert_eq!(kind("/tmp.d/file"), Kind::None);
    }

    #[test]
    fn grants_are_bounded_tokens_for_granted_files_only() {
        let mut grants = Grants::default();
        let first = grants.grant(PathBuf::from("/work/a.png"));
        assert_eq!(first.len(), 32);
        assert_eq!(grants.grant(PathBuf::from("/work/a.png")), first, "one token per file");
        assert_eq!(grants.path(&first), Some(PathBuf::from("/work/a.png")));
        assert_eq!(grants.path("../../etc/passwd"), None);
        assert_eq!(grants.path(&"0".repeat(32)), None);
        for n in 0..GRANTS {
            grants.grant(PathBuf::from(format!("/work/{n}.png")));
        }
        assert_eq!(grants.path(&first), None, "the oldest token goes first");
        assert!(grants.paths.len() <= GRANTS);
    }

    #[test]
    fn a_local_file_is_granted_within_qt_s_preview_rules() {
        let dir = std::env::temp_dir().join(format!("zerus-media-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let picture = dir.join("shot.png");
        std::fs::write(&picture, [137, 80, 78, 71]).unwrap();
        let grants = Mutex::new(Grants::default());
        let answer = local_answer(7, &picture, false, &grants);
        assert_eq!((answer.request, answer.state.as_str(), answer.kind, answer.size, answer.name.as_str()), (7, "ready", Kind::Image, 4, "shot.png"));
        assert_eq!(lock(&grants).path(&answer.token), Some(picture.clone()));
        let missing = local_answer(8, &dir.join("gone.png"), false, &grants);
        assert_eq!((missing.state.as_str(), missing.code.as_str(), missing.token.as_str()), ("error", "missing", ""));
        let text = dir.join("notes.txt");
        std::fs::write(&text, "x").unwrap();
        assert_eq!(local_answer(9, &text, true, &grants).code, "not_media");
        assert_eq!(local_answer(10, &dir, true, &grants).code, "not_media");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
