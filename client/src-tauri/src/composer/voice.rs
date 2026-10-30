//! Voice uses this composer's current authoritative target and local-row owner.
//! Recording locks edits and text sends, without persisting any recording flag.
use super::Composers;
use zerus_client_core::voice_composer::Target;

impl Composers {
    pub(crate) fn voice_target(&self, handle: u64) -> Option<Target> {
        let hub = self.lock();
        let ctx = self.context(&hub)?;
        let (_, input_visible) = self.question_part(&hub);
        let composer = hub.composers.get(&ctx.key).filter(|c| c.handle == handle)?;
        let local_windows = cfg!(windows) && ctx.session.computer.is_empty();
        let reason = if local_windows {
            "Voice messages cannot be sent to sessions on this Windows computer yet.".into()
        } else {
            ctx.reason().unwrap_or_default()
        };
        let details = ctx.details.as_deref().unwrap_or(&serde_json::Value::Null);
        let warning = if zerus_client_core::cache::expired(details, self.now()) {
            format!(
                "{}\n\nSend this voice message with the full conversation context?",
                zerus_client_core::cache::warning(details, self.now())
            )
        } else {
            String::new()
        };
        let (run, conversation) = (ctx.run(), ctx.conversation());
        let archived = ctx.session.archived();
        Some(Target {
            key: ctx.key,
            host: ctx.session.computer,
            session: ctx.session.name,
            run,
            conversation,
            available: !local_windows
                && ctx.online
                && !archived
                && reason.is_empty()
                && input_visible
                && composer.loaded
                && !composer.sending
                && composer.waiting.is_none()
                && !composer.renaming
                && !composer.attaching,
            reason,
            warning,
            draft_only: false,
        })
    }

    pub(crate) fn voice_recording(&self, owner: &str, key: Option<&str>) {
        let mut hub = self.lock();
        let mut changed = false;
        for composer in hub.composers.values_mut() {
            let next = if Some(composer.key.as_str()) == key {
                Some(owner.to_owned())
            } else if composer.recording_owner.as_deref() == Some(owner) {
                None
            } else {
                continue;
            };
            if composer.recording_owner != next {
                composer.recording_owner = next;
                changed = true;
            }
        }
        drop(hub);
        if changed {
            self.changed();
        }
    }

    pub(crate) fn voice_message(&self, key: &str, message: &serde_json::Value) {
        self.lock().local.voice_message(key, message);
        self.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Composer, Hub};
    use super::*;

    #[test]
    fn recording_is_an_in_memory_edit_and_send_lock() {
        let mut composer = Composer::new(1, "peer\ncodex/example/review".into());
        composer.loaded = true;
        assert!(!composer.busy());
        composer.recording_owner = Some("private-voice-owner".into());
        assert!(composer.busy());
        assert!(!composer.stored().sending);
        let mut hub = Hub::default();
        hub.composers.insert(composer.key.clone(), composer);
        assert!(Composers::editable(&mut hub, 1).is_none());
        hub.composers.values_mut().next().unwrap().recording_owner = None;
        assert!(Composers::editable(&mut hub, 1).is_some());
    }
}
