//! Qt's temporary KWin scripting adapter (KWinWindowLayer.cpp). No global
//! rules or installed plugins. All D-Bus and file work runs off the UI thread.
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

const SCRIPTING: &str = "org.kde.kwin.Scripting";
const LIMIT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub id: String,
    pub on: bool,
    pub error: String,
}
impl Reply {
    fn error(error: &str) -> Self {
        Self {
            id: String::new(),
            on: false,
            error: error.into(),
        }
    }
}

struct Pending {
    owner: String,
    token: String,
    sender: mpsc::SyncSender<Reply>,
}
#[derive(Clone, Default)]
struct Reports(Arc<Mutex<Option<Pending>>>);
#[zbus::interface(name = "org.hgdev.Zerus.WindowLayer")]
impl Reports {
    fn report(
        &self,
        token: &str,
        id: &str,
        on: bool,
        error: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) {
        let pending = self.0.lock().unwrap();
        let Some(p) = pending.as_ref() else { return };
        if header.sender().map(|s| s.as_str()) != Some(p.owner.as_str()) || token != p.token {
            return;
        }
        let reply = if !id.is_empty() && uuid::Uuid::parse_str(id).is_err() {
            Reply::error("unsupported")
        } else {
            Reply {
                id: id.into(),
                on,
                error: error.into(),
            }
        };
        let _ = p.sender.try_send(reply);
    }
}

struct ScriptFile(PathBuf);
impl ScriptFile {
    fn new(script: &str) -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("zerus-window-layer-{}.js", uuid::Uuid::new_v4()));
        let mut file: File = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let guard = Self(path);
        file.write_all(script.as_bytes())?;
        file.flush()?;
        Ok(guard)
    }
}
impl Drop for ScriptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub struct KWin {
    bus: Connection,
    reports: Reports,
    path: String,
    pid: u32,
    app_id: String,
}
impl KWin {
    pub fn session(app_id: String) -> zbus::Result<Self> {
        Self::connect(zbus::blocking::connection::Builder::session()?, app_id)
    }
    fn connect(
        builder: zbus::blocking::connection::Builder<'_>,
        app_id: String,
    ) -> zbus::Result<Self> {
        let path = format!(
            "/org/hgdev/Zerus/WindowLayer/{}",
            uuid::Uuid::new_v4().simple()
        );
        let reports = Reports::default();
        let bus = builder
            .method_timeout(Duration::from_secs(2))
            .serve_at(path.as_str(), reports.clone())?
            .build()?;
        Ok(Self {
            bus,
            reports,
            path,
            pid: std::process::id(),
            app_id,
        })
    }
    fn owner(&self) -> zbus::Result<String> {
        Proxy::new(
            &self.bus,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )?
        .call("GetNameOwner", &("org.kde.KWin",))
    }
    pub fn request(&self, caption: &str, id: &str, desired: Option<bool>) -> Reply {
        self.run_script(caption, id, desired)
            .unwrap_or_else(|_| Reply::error("unavailable"))
    }
    fn run_script(
        &self,
        caption: &str,
        id: &str,
        desired: Option<bool>,
    ) -> Result<Reply, Box<dyn std::error::Error>> {
        let owner = self.owner()?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let plugin = format!("zerus-window-layer-{token}");
        let config = json!({ "pid": self.pid, "appId": self.app_id, "caption": caption, "id": id,
            "desired": desired, "service": self.bus.unique_name().unwrap().as_str(), "path": self.path,
            "token": token, "plugin": plugin, "kwin": owner });
        let file = ScriptFile::new(&script(&config))?;
        let scripting = Proxy::new(&self.bus, owner.as_str(), "/Scripting", SCRIPTING)?;
        let (sender, receive) = mpsc::sync_channel(1);
        *self.reports.0.lock().unwrap() = Some(Pending {
            owner: owner.clone(),
            token,
            sender,
        });
        let result = (|| {
            let number: i32 = scripting.call(
                "loadScript",
                &(file.0.to_string_lossy().as_ref(), plugin.as_str()),
            )?;
            if number < 0 {
                return Ok(Reply::error("unavailable"));
            }
            let path = format!("/Scripting/Script{number}");
            let runner = Proxy::new(
                &self.bus,
                owner.as_str(),
                path.as_str(),
                "org.kde.kwin.Script",
            )?;
            runner.call::<_, _, ()>("run", &())?;
            Ok::<_, zbus::Error>(
                receive
                    .recv_timeout(LIMIT)
                    .unwrap_or_else(|_| Reply::error("unavailable")),
            )
        })();
        self.reports.0.lock().unwrap().take();
        // Also unload on rejection, timeout and late replies. The pinned owner
        // prevents unloading anything on a replacement compositor.
        let _ = scripting.call::<_, _, bool>("unloadScript", &(plugin.as_str(),));
        Ok(result?)
    }
    pub fn inspect(&self, id: &str) -> Reply {
        self.inspect_inner(id)
            .unwrap_or_else(|_| Reply::error("unavailable"))
    }
    fn inspect_inner(&self, id: &str) -> zbus::Result<Reply> {
        let owner = self.owner()?;
        let proxy = Proxy::new(&self.bus, owner.as_str(), "/KWin", "org.kde.KWin")?;
        let info: HashMap<String, OwnedValue> = proxy.call("getWindowInfo", &(id,))?;
        let text = |key| info.get(key).and_then(|v| <&str>::try_from(v).ok());
        let pid = info.get("pid").and_then(|v| {
            u32::try_from(v)
                .ok()
                .or_else(|| i64::try_from(v).ok().and_then(|n| u32::try_from(n).ok()))
                .or_else(|| i32::try_from(v).ok().and_then(|n| u32::try_from(n).ok()))
        });
        if text("uuid") != Some(id)
            || pid != Some(self.pid)
            || text("resourceClass") != Some(self.app_id.as_str())
        {
            return Ok(Reply::error("missing"));
        }
        Ok(
            match info.get("keepAbove").and_then(|v| bool::try_from(v).ok()) {
                Some(on) => Reply {
                    id: id.into(),
                    on,
                    error: String::new(),
                },
                None => Reply::error("unsupported"),
            },
        )
    }
}

pub(crate) fn script(config: &Value) -> String {
    format!(
        "const config = {config};\n{}",
        include_str!("kwin_window_layer.js")
    )
}

#[cfg(test)]
#[path = "linux_window_layer_tests.rs"]
mod tests;
