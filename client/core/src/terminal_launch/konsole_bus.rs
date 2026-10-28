//! Konsole over the session D-Bus (`tray/src/KonsoleBackend.cpp`). Layout,
//! as Konsole 26.08 exports it:
//!
//! ```text
//! org.kde.konsole-PID          one service per konsole process
//! /Windows/N                   org.kde.konsole.Window   newSession, sessionList, setCurrentSession
//! /Sessions/N                  org.kde.konsole.Session  processId, runCommand, sendText
//! /konsole/MainWindow_N        org.qtproject.Qt.QWidget raise, showNormal and properties
//! ```
//!
//! `/Windows/N` and `/konsole/MainWindow_N` share their index. Every call
//! waits at most 3 s: a hung Konsole must not hang the launch.

use super::Konsole;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::{Connection, Proxy};
use std::time::Duration;

const SERVICE_PREFIX: &str = "org.kde.konsole-";
const WINDOW: &str = "org.kde.konsole.Window";
const SESSION: &str = "org.kde.konsole.Session";
const WIDGET: &str = "org.qtproject.Qt.QWidget";
const TIMEOUT: Duration = Duration::from_secs(3);

/// The session bus, connected on first use.
#[derive(Default)]
pub struct DbusKonsole {
    connection: Option<Connection>,
}

impl DbusKonsole {
    pub fn new() -> Self {
        Self::default()
    }

    fn proxy<'a>(
        &'a mut self,
        service: &'a str,
        path: &'a str,
    ) -> Option<Proxy<'a, &'a Connection>> {
        if self.connection.is_none() {
            self.connection = Connection::new_session().ok();
        }
        Some(self.connection.as_ref()?.with_proxy(service, path, TIMEOUT))
    }
}

/// `name: message`, or `no reply` for an error without a name.
fn error_text(error: dbus::Error) -> String {
    match error.name() {
        Some(name) => format!("{name}: {}", error.message().unwrap_or_default()),
        None => "no reply".to_owned(),
    }
}

fn not_connected() -> String {
    "no reply".to_owned()
}

/// The numeric `<node name="N"/>` children of an introspection document.
fn numeric_nodes(xml: &str) -> Vec<i32> {
    let mut nodes: Vec<i32> = xml
        .split("<node")
        .skip(1)
        .filter_map(|element| {
            let element = &element[..element.find('>').unwrap_or(element.len())];
            let start = element.find("name=")? + 5;
            let quote = element[start..].chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let value = &element[start + 1..];
            value[..value.find(quote)?].trim().parse().ok()
        })
        .collect();
    nodes.sort_unstable();
    nodes
}

impl Konsole for DbusKonsole {
    fn services(&mut self) -> Vec<String> {
        let Some(proxy) = self.proxy("org.freedesktop.DBus", "/org/freedesktop/DBus") else {
            return Vec::new();
        };
        let Ok((names,)): Result<(Vec<String>,), _> =
            proxy.method_call("org.freedesktop.DBus", "ListNames", ())
        else {
            return Vec::new();
        };
        let mut services: Vec<String> = names
            .into_iter()
            .filter(|name| name.starts_with(SERVICE_PREFIX))
            .collect();
        // By process ID: the oldest Konsole first, stable between clicks.
        services.sort_by_key(|name| name[SERVICE_PREFIX.len()..].parse::<i64>().unwrap_or(0));
        services
    }

    fn children(&mut self, service: &str, path: &str) -> Vec<i32> {
        let Some(proxy) = self.proxy(service, path) else {
            return Vec::new();
        };
        let reply: Result<(String,), _> =
            proxy.method_call("org.freedesktop.DBus.Introspectable", "Introspect", ());
        reply.map(|(xml,)| numeric_nodes(&xml)).unwrap_or_default()
    }

    fn window_flag(&mut self, service: &str, window: i32, property: &str) -> Option<bool> {
        let path = format!("/konsole/MainWindow_{window}");
        self.proxy(service, &path)?
            .get::<bool>(WIDGET, property)
            .ok()
    }

    fn new_session(&mut self, service: &str, window: i32) -> Result<i32, String> {
        let path = format!("/Windows/{window}");
        let proxy = self.proxy(service, &path).ok_or_else(not_connected)?;
        let reply: Result<(i32,), _> = proxy.method_call(WINDOW, "newSession", ());
        reply.map(|(session,)| session).map_err(error_text)
    }

    fn send_text(&mut self, service: &str, session: i32, text: &str) -> Result<(), String> {
        let path = format!("/Sessions/{session}");
        let proxy = self.proxy(service, &path).ok_or_else(not_connected)?;
        proxy
            .method_call(SESSION, "sendText", (text,))
            .map_err(error_text)
    }

    fn run_command(&mut self, service: &str, session: i32, command: &str) -> Result<(), String> {
        let path = format!("/Sessions/{session}");
        let proxy = self.proxy(service, &path).ok_or_else(not_connected)?;
        proxy
            .method_call(SESSION, "runCommand", (command,))
            .map_err(error_text)
    }

    fn set_current_session(&mut self, service: &str, window: i32, session: i32) {
        let path = format!("/Windows/{window}");
        if let Some(proxy) = self.proxy(service, &path) {
            let _: Result<(), _> = proxy.method_call(WINDOW, "setCurrentSession", (session,));
        }
    }

    fn window_call(&mut self, service: &str, window: i32, method: &str) {
        let path = format!("/konsole/MainWindow_{window}");
        if let Some(proxy) = self.proxy(service, &path) {
            let _: Result<(), _> = proxy.method_call(WIDGET, method, ());
        }
    }

    fn process_id(&mut self, service: &str, session: i32) -> Option<i32> {
        let path = format!("/Sessions/{session}");
        let reply: Result<(i32,), _> =
            self.proxy(service, &path)?
                .method_call(SESSION, "processId", ());
        reply.ok().map(|(pid,)| pid)
    }

    fn session_list(&mut self, service: &str, window: i32) -> Option<Vec<String>> {
        let path = format!("/Windows/{window}");
        let reply: Result<(Vec<String>,), _> =
            self.proxy(service, &path)?
                .method_call(WINDOW, "sessionList", ());
        reply.ok().map(|(sessions,)| sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::numeric_nodes;

    #[test]
    fn only_numeric_children_count() {
        let xml = r#"<!DOCTYPE node><node name="/Windows">
            <interface name="org.freedesktop.DBus.Introspectable"/>
            <node name="3"/><node name='1'/><node name="MainWindow_1"/>
            <node name="2"></node></node>"#;
        assert_eq!(numeric_nodes(xml), vec![1, 2, 3]);
        assert!(numeric_nodes("<node/>").is_empty());
    }
}
