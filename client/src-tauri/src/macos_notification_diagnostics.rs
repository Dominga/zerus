//! Fixed-content callback diagnostics. Never format native notification data.
use objc2_foundation::{ns_string, NSString};
use objc2_user_notifications::{
    UNNotificationDefaultActionIdentifier, UNNotificationDismissActionIdentifier,
};

pub(crate) enum Role {
    #[cfg(feature = "live")]
    Gui,
    Host,
}

// Public Foundation declaration from NSObjCRuntime.h. The generated objc2
// bindings omit this variadic API; every call below has a literal format with
// no conversion specifiers and therefore takes no variadic arguments.
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    fn NSLog(format: *const NSString, ...);
}

/// Called only by an already installed delegate, without changing its behavior.
pub(crate) fn response(role: Role, action: &NSString) {
    // NSLog enters unified logging even when the original host's stderr is a
    // privately retained driver pipe. Never pass native action text as format.
    let message = match (role, kind(action)) {
        #[cfg(feature = "live")]
        (Role::Gui, "default") => {
            ns_string!("zerus-client: notification callback role=gui kind=default")
        }
        #[cfg(feature = "live")]
        (Role::Gui, "dismiss") => {
            ns_string!("zerus-client: notification callback role=gui kind=dismiss")
        }
        #[cfg(feature = "live")]
        (Role::Gui, _) => ns_string!("zerus-client: notification callback role=gui kind=other"),
        (Role::Host, "default") => {
            ns_string!("zerus-client: notification callback role=host kind=default")
        }
        (Role::Host, "dismiss") => {
            ns_string!("zerus-client: notification callback role=host kind=dismiss")
        }
        (Role::Host, _) => ns_string!("zerus-client: notification callback role=host kind=other"),
    };
    // SAFETY: static, live NSString format, with no '%' or variadic arguments.
    unsafe { NSLog(message as *const NSString) };
}

fn kind(action: &NSString) -> &'static str {
    // SAFETY: immutable identifiers supplied by UserNotifications.framework.
    if action.isEqualToString(unsafe { UNNotificationDefaultActionIdentifier }) {
        "default"
    } else if action.isEqualToString(unsafe { UNNotificationDismissActionIdentifier }) {
        "dismiss"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_log_transport_smoke_without_a_native_callback() {
        // This is a logging transport probe, never an observed OS callback.
        println!("UNIFIED_LOG_TRANSPORT_SMOKE_PID {}", std::process::id());
        response(Role::Host, &NSString::from_str("private-fixture-action"));
    }

    #[test]
    fn native_identifiers_are_classified_without_exposing_other_action_text() {
        // SAFETY: immutable SDK constants; no center, permission or notification.
        assert_eq!(
            kind(unsafe { UNNotificationDefaultActionIdentifier }),
            "default"
        );
        assert_eq!(
            kind(unsafe { UNNotificationDismissActionIdentifier }),
            "dismiss"
        );
        for text in ["", "private-fixture-action", "default", "dismiss"] {
            assert_eq!(kind(&NSString::from_str(text)), "other");
        }
    }
}
