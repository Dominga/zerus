//! The microphone permission, as Qt's `QMicrophonePermission` asks it: on
//! macOS the system's consent (AVCaptureDevice authorization), asked only
//! when the user starts a recording and the answer is not known yet;
//! elsewhere opening the device decides.

/// The microphone's consent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Granted,
    Denied,
    /// Never asked: [`request_microphone`] asks.
    Undetermined,
}

/// The current consent; never asks.
pub fn microphone() -> Status {
    platform::microphone()
}

/// Asks for consent (the system's prompt) and waits for the answer.
pub fn request_microphone() -> bool {
    platform::request_microphone()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Status;
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    use std::sync::mpsc;

    pub fn microphone() -> Status {
        // SAFETY: AVFoundation exports this constant string.
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
            return Status::Denied;
        };
        // SAFETY: audio is one of the two media types the method accepts.
        let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
        if status == AVAuthorizationStatus::Authorized {
            Status::Granted
        } else if status == AVAuthorizationStatus::NotDetermined {
            Status::Undetermined
        } else {
            Status::Denied
        }
    }

    pub fn request_microphone() -> bool {
        // SAFETY: AVFoundation exports this constant string.
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
            return false;
        };
        let (sender, receiver) = mpsc::channel();
        // The handler runs once, on an arbitrary dispatch queue.
        let handler = RcBlock::new(move |granted: Bool| {
            let _ = sender.send(granted.as_bool());
        });
        // SAFETY: audio is an accepted media type and the handler lives
        // until AVFoundation releases its copy.
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler) };
        receiver.recv().unwrap_or(false)
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Status;

    pub fn microphone() -> Status {
        Status::Granted
    }

    pub fn request_microphone() -> bool {
        true
    }
}
