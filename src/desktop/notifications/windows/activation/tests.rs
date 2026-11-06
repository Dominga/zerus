//! The real in-process COM objects, called through their native interfaces
//! without class registration, a running service or a notification effect.
use super::super::{
    callbacks::{Candidate, PREFIX},
    wire,
};
use super::*;
use std::sync::{mpsc, Arc};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}
fn activate(
    callback: &INotificationActivationCallback,
    application: &str,
    arguments: &str,
    inputs: usize,
) -> ::windows::core::Result<()> {
    let (application, arguments) = (wide(application), wide(arguments));
    let (key, value) = (wide("reply"), wide("text"));
    let data = vec![
        NOTIFICATION_USER_INPUT_DATA {
            Key: PCWSTR(key.as_ptr()),
            Value: PCWSTR(value.as_ptr()),
        };
        inputs
    ];
    unsafe {
        callback.Activate(
            PCWSTR(application.as_ptr()),
            PCWSTR(arguments.as_ptr()),
            &data,
        )
    }
}
fn create(factory: &IClassFactory) -> ::windows::core::Result<INotificationActivationCallback> {
    unsafe { factory.CreateInstance::<_, INotificationActivationCallback>(None::<&IUnknown>) }
}
fn fixture() -> (Slot, mpsc::Receiver<Candidate>, IClassFactory) {
    let (slot, events) = Slot::new(std::thread::current(), Arc::new(()));
    let factory: IClassFactory = Factory {
        retained: slot.hold().unwrap(),
        slot: slot.clone(),
    }
    .into();
    (slot, events, factory)
}

#[test]
fn activator_admits_exact_clicks_only_between_restoration_and_stop() {
    let (slot, events, factory) = fixture();
    let request = uuid::Uuid::new_v4().to_string();
    let launch = format!("{PREFIX}{request}");
    // Dormant until restoration completes: no object, no queued click.
    assert_eq!(
        create(&factory).unwrap_err().code(),
        CLASS_E_CLASSNOTAVAILABLE
    );
    assert!(slot.finish_restoration());
    let callback = create(&factory).unwrap();
    activate(&callback, wire::APPLICATION, &launch, 0).unwrap();
    assert_eq!(
        events.try_recv().unwrap(),
        Candidate::Clicked(request.clone())
    );
    // Foreign application, input payloads, forged or overlong arguments.
    let long = format!("{PREFIX}{}", "a".repeat(80));
    for (application, arguments, inputs) in [
        ("other.application", launch.as_str(), 0),
        (wire::APPLICATION, launch.as_str(), 1),
        (wire::APPLICATION, "hgs-desktop:not-a-uuid", 0),
        (wire::APPLICATION, request.as_str(), 0),
        (wire::APPLICATION, long.as_str(), 0),
    ] {
        assert_eq!(
            activate(&callback, application, arguments, inputs)
                .unwrap_err()
                .code(),
            E_INVALIDARG,
            "{application} {arguments} {inputs}"
        );
    }
    let null = unsafe { callback.Activate(PCWSTR::null(), PCWSTR::null(), &[]) };
    assert_eq!(null.unwrap_err().code(), E_POINTER);
    assert!(events.try_recv().is_err());
    // Aggregation is refused before any callback object exists.
    let outer: IUnknown = factory.cast().unwrap();
    let aggregated =
        unsafe { factory.CreateInstance::<_, INotificationActivationCallback>(Some(&outer)) };
    assert_eq!(aggregated.unwrap_err().code(), CLASS_E_NOAGGREGATION);
    drop(outer);
    // Stop closes admission for new objects and for an object already handed out.
    slot.request_stop();
    assert_eq!(
        create(&factory).unwrap_err().code(),
        CLASS_E_CLASSNOTAVAILABLE
    );
    assert_eq!(
        activate(&callback, wire::APPLICATION, &launch, 0)
            .unwrap_err()
            .code(),
        E_INVALIDARG
    );
    assert!(events.try_recv().is_err());
}

#[test]
fn every_native_object_holds_its_charge_until_the_last_reference_returns() {
    let (slot, _events, factory) = fixture();
    assert_eq!(slot.retained(), 1);
    assert!(slot.finish_restoration());
    let callback = create(&factory).unwrap();
    let copy = callback.clone();
    assert_eq!(slot.retained(), 2);
    slot.request_stop();
    drop(factory);
    // A COM client still holding the callback keeps its charge after stop.
    assert_eq!(slot.retained(), 1);
    drop(callback);
    assert_eq!(slot.retained(), 1);
    drop(copy);
    assert_eq!(slot.retained(), 0);
}
