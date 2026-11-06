//! Held executable images for the closed native producers. This supplies no
//! caller authentication, execution consent or operation admission.
#[cfg(target_os = "linux")]
pub mod linux;
