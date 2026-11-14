//! Installer and UI readiness for the existing desktop-v1 endpoint.
//! IPC readiness is independent of background writer ownership. Starting the
//! passive kernel must never advertise workers which have not been started.
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const ABI: &str = "desktop-lifecycle-v1";

/// Whether this desktop process released, at startup, a console that Windows
/// created for it alone (a logon-task start before Windows 11 24H2). Always
/// false elsewhere: 24H2 and later give the service no console at all.
pub(super) static CONSOLE_RELEASED: AtomicBool = AtomicBool::new(false);

pub(super) fn decorate_mode(value: &mut Value, background_requested: bool) {
    let requested = if background_requested {
        "background"
    } else {
        "passive"
    };
    let effective =
        if value["ownership"] == "background_service" && value["writers_enabled"] == true {
            "background"
        } else {
            "passive"
        };
    // mode is the original requested-mode field. Add observed state without
    // changing its meaning for registration helpers or older clients.
    value["lifecycle"]["mode"] = requested.into();
    value["lifecycle"]["requested_mode"] = requested.into();
    value["lifecycle"]["effective_mode"] = effective.into();
}

pub(super) fn unavailable(configuration: &str) -> Value {
    // A missing endpoint is not proof that every old process has exited.
    json!({"abi":ABI,"phase":"unavailable","ipc_ready":false,
        "pid":null,"configuration":configuration,"epoch":null,"generation":null})
}

pub(super) fn status(configuration: &str, epoch: &str, generation: u64) -> Value {
    json!({
        "abi": ABI,
        "phase": "serving",
        "ipc_ready": true,
        "pid": std::process::id(),
        "configuration": configuration,
        "epoch": epoch,
        "generation": generation,
        "console_released": CONSOLE_RELEASED.load(Ordering::Relaxed),
        "background": {
            "fleet": {"ready": false, "code": "not_started"},
            "presence": {"ready": false, "code": "not_started"},
            "reads": {"ready": false, "code": "not_started"},
            "attention": {"ready": false, "code": "not_started"},
            "approvals": {"ready": false, "code": "not_started"}
        }
    })
}

/// Probe the same selected binary/configuration, even for a nondefault install.
/// The installer supplies this environment to a local child; it is not a shell
/// command, a captured ambient environment, or a new service registration.
pub(super) fn probe(binary: &str, config: &str, state: &str, configuration: &str) -> Value {
    json!({
        "abi": ABI,
        "argv": [binary, "desktop", "status", "--json"],
        "environment": {"HGS_CONFIG_DIR": config, "HGS_STATE_DIR": state},
        "expected_configuration": configuration,
        "configuration_field": "lifecycle.configuration",
        "ready_field": "lifecycle.ipc_ready",
        "epoch_field": "lifecycle.epoch",
        "generation_field": "lifecycle.generation"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_readiness_does_not_claim_background_ownership() {
        let value = status("configuration", "epoch-one", 7);
        assert_eq!(value["ipc_ready"], true);
        assert_eq!(value["pid"], std::process::id());
        assert_eq!(value["configuration"], "configuration");
        assert_eq!(value["epoch"], "epoch-one");
        assert_eq!(value["generation"], 7);
        assert_eq!(value["console_released"], false);
        for component in ["fleet", "presence", "reads", "attention", "approvals"] {
            assert_eq!(value["background"][component]["ready"], false);
            assert_eq!(value["background"][component]["code"], "not_started");
        }
    }

    #[test]
    fn missing_endpoint_is_not_a_process_exit_receipt() {
        let value = unavailable("configuration");
        assert_eq!(value["ipc_ready"], false);
        assert_eq!(value["phase"], "unavailable");
        assert!(value["pid"].is_null());
        assert!(value["epoch"].is_null());
        assert!(value["generation"].is_null());
    }

    #[test]
    fn nondefault_probe_preserves_argv_paths_without_shell_expansion() {
        let value = probe(
            "/opt/example/hgs cli",
            "/private/config $literal",
            "/private/state",
            "expected",
        );
        assert_eq!(
            value["argv"],
            json!(["/opt/example/hgs cli", "desktop", "status", "--json"])
        );
        assert_eq!(
            value["environment"]["HGS_CONFIG_DIR"],
            "/private/config $literal"
        );
        assert_eq!(value["environment"]["HGS_STATE_DIR"], "/private/state");
        assert_eq!(value["environment"].as_object().unwrap().len(), 2);
        assert_eq!(value["expected_configuration"], "expected");
    }

    #[test]
    fn requested_mode_stays_compatible_and_effective_mode_follows_the_live_writer() {
        for requested in [false, true] {
            for (ownership, writer, expected) in [
                ("passive", false, "passive"),
                ("revoked", false, "passive"),
                ("revoked", true, "passive"),
                ("background_readonly", false, "passive"),
                ("background_service", false, "passive"),
                ("background_service", true, "background"),
                ("unavailable", true, "passive"),
            ] {
                let mut value = json!({"ownership":ownership,"writers_enabled":writer,
                    "lifecycle":{"abi":ABI,"phase":"serving","ipc_ready":true}});
                if ownership == "passive" {
                    value["background_start"] =
                        json!({"admitted":false,"code":"explicit_handoff_required"});
                }
                decorate_mode(&mut value, requested);
                let requested = if requested { "background" } else { "passive" };
                assert_eq!(
                    value["lifecycle"]["mode"], requested,
                    "legacy mode must not change"
                );
                assert_eq!(value["lifecycle"]["requested_mode"], requested);
                assert_eq!(
                    value["lifecycle"]["effective_mode"], expected,
                    "ownership={ownership}, writer={writer}"
                );
                assert_eq!(value["lifecycle"]["abi"], ABI);
                assert_eq!(value["lifecycle"]["ipc_ready"], true);
                assert_eq!(value["ownership"], ownership);
                assert_eq!(value["writers_enabled"], writer);
                if ownership == "passive" {
                    assert_eq!(
                        value["background_start"]["code"],
                        "explicit_handoff_required"
                    );
                }
            }
        }
    }
}
