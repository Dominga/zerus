//! Fixed-content callback diagnostics. Never format native notification data.
use objc2_foundation::NSString;
use objc2_user_notifications::{
    UNNotificationDefaultActionIdentifier, UNNotificationDismissActionIdentifier,
};

pub(crate) enum Role {
    #[cfg(feature = "live")]
    Gui,
    Host,
}

/// Called only by an already installed delegate, without changing its behavior.
pub(crate) fn response(role: Role, action: &NSString) {
    let role = match role {
        #[cfg(feature = "live")]
        Role::Gui => "gui",
        Role::Host => "host",
    };
    eprintln!(
        "zerus-client: notification callback role={role} kind={}",
        kind(action)
    );
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
