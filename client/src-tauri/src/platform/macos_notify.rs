//! Attention notifications under the app's identity, as Qt posts them
//! (`tray/src/MacAttentionNotifier.mm`): `UNUserNotificationCenter` gets one
//! request per token (`zerus.attention.<token>`) with the default sound,
//! withdrawn from pending and delivered when the attention clears; the
//! default action opens the token's session. The decisions live in
//! `notifier::Notifier` and the core's `attention::Delivery`; this module
//! only talks to the system. It also tells the desk whether Zerus is the
//! active app, the tracker's foreground test (`AttentionTracker.cpp:44-48`).
use std::ptr::NonNull;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationDidBecomeActiveNotification,
    NSApplicationDidResignActiveNotification,
};
use objc2_foundation::{
    ns_string, NSArray, NSDictionary, NSError, NSNotification, NSNotificationCenter, NSString,
};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSettings, UNNotificationSound,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use tauri::{AppHandle, Manager};
use zerus_client_core::attention::{self, Notice};

use crate::desk::DeskInput;
use crate::notifier::{Center, Done, Notifier};

pub struct Ivars {
    app: AppHandle,
    desk: Sender<DeskInput>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; Delegate has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "ZerusAttentionDelegate"]
    #[ivars = Ivars]
    pub struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Banners also while Zerus is in front (`MacAttentionNotifier.mm:14-19`).
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        /// The default action activates Zerus and hands the token to the
        /// desk, which decodes it off the main thread (`MacAttentionNotifier.mm:20-28`,
        /// `:38-43`; `TrayAgent.cpp:176-177`).
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            if let Some(token) = clicked_token(response) {
                let _ = self.ivars().app.run_on_main_thread(activate);
                let _ = self.ivars().desk.send(DeskInput::Clicked(token));
            }
            completion.call(());
        }
    }
);

impl Delegate {
    fn new(app: AppHandle, desk: Sender<DeskInput>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { app, desk });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// `[NSApp activateIgnoringOtherApps:YES]`, as Qt's click handler does.
fn activate() {
    if let Some(mtm) = MainThreadMarker::new() {
        // The call Qt makes; `activate` alone may yield to the app in front.
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

/// The token of a default action, when its `userInfo` holds one as a string.
fn clicked_token(response: &UNNotificationResponse) -> Option<String> {
    // SAFETY: the framework's constant action identifier.
    let default = unsafe { UNNotificationDefaultActionIdentifier };
    if !response.actionIdentifier().isEqualToString(default) {
        return None;
    }
    let info = response.notification().request().content().userInfo();
    let key: &AnyObject = ns_string!("hgsAttentionToken");
    let value = info.objectForKey(key)?;
    value
        .downcast::<NSString>()
        .ok()
        .map(|token| token.to_string())
}

fn describe(error: *mut NSError) -> Option<String> {
    // SAFETY: the system passes null or a valid error.
    unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string())
}

/// A completion the system calls through an `Fn` block, at most once used.
fn once<T>(done: Done<T>) -> Mutex<Option<Done<T>>> {
    Mutex::new(Some(done))
}

fn take<T>(done: &Mutex<Option<Done<T>>>) -> Option<Done<T>> {
    done.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// `UNUserNotificationCenter.currentNotificationCenter`, asked at each use.
struct System;

impl Center for System {
    fn authorize(&self, done: Done<(bool, Option<String>)>) {
        let done = once(done);
        let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let Some(done) = take(&done) else { return };
            // SAFETY: the system passes a valid settings object.
            let status = unsafe { settings.as_ref() }.authorizationStatus();
            if status == UNAuthorizationStatus::NotDetermined {
                let done = once(done);
                let reply = RcBlock::new(move |granted: Bool, error: *mut NSError| {
                    if let Some(done) = take(&done) {
                        done((granted.as_bool(), describe(error)));
                    }
                });
                UNUserNotificationCenter::currentNotificationCenter()
                    .requestAuthorizationWithOptions_completionHandler(
                        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                        &reply,
                    );
            } else {
                done((
                    status == UNAuthorizationStatus::Authorized
                        || status == UNAuthorizationStatus::Provisional,
                    None,
                ));
            }
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .getNotificationSettingsWithCompletionHandler(&block);
    }

    fn add(&self, token: &str, title: &str, body: &str, done: Done<Option<String>>) {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let value = NSString::from_str(token);
        let info = NSDictionary::<NSString, NSString>::from_slices(
            &[ns_string!("hgsAttentionToken")],
            &[&*value],
        );
        // SAFETY: a dictionary of strings is a valid userInfo; the cast only
        // drops its element types.
        unsafe {
            let info: Retained<NSDictionary> = Retained::cast_unchecked(info);
            content.setUserInfo(&info);
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&attention::request_id(token)),
            &content,
            None,
        );
        let done = once(done);
        let reply = RcBlock::new(move |error: *mut NSError| {
            if let Some(done) = take(&done) {
                done(describe(error));
            }
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&reply));
    }

    fn remove(&self, token: &str) {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let ids =
            NSArray::from_retained_slice(&[NSString::from_str(&attention::request_id(token))]);
        center.removePendingNotificationRequestsWithIdentifiers(&ids);
        center.removeDeliveredNotificationsWithIdentifiers(&ids);
    }
}

/// The notifier, in the app state.
pub struct NotifierState(Notifier);

/// Sets the delegate before the first notification posts and starts
/// following whether Zerus is the active app. Call on the main thread, from
/// setup: a click that launched the app reaches the delegate once set.
pub fn install(app: &AppHandle, desk: Sender<DeskInput>) {
    let delegate = Delegate::new(app.clone(), desk.clone());
    UNUserNotificationCenter::currentNotificationCenter()
        .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center keeps a weak reference: the delegate lives as long as the app.
    std::mem::forget(delegate);
    let main = app.clone();
    let notifier = Notifier::new(
        Box::new(System),
        Arc::new(move |work| {
            let _ = main.run_on_main_thread(work);
        }),
        Box::new(move |token, detail| {
            let _ = desk.send(DeskInput::Failed { token, detail });
        }),
    );
    app.manage(NotifierState(notifier));
    follow_activity(app);
}

/// Withdraws, then posts, on the main thread (`TrayAgent::observeAttention`).
pub fn deliver(app: &AppHandle, cleared: Vec<String>, raised: Vec<Notice>) {
    if let Some(state) = app.try_state::<NotifierState>() {
        state.0.deliver(&cleared, &raised);
    }
}

/// Qt's foreground test is `QGuiApplication::applicationState() ==
/// Qt::ApplicationActive` (`AttentionTracker.cpp:44-48`): the app's active
/// state, followed from AppKit's notifications.
fn follow_activity(app: &AppHandle) {
    if let Some(mtm) = MainThreadMarker::new() {
        crate::desk::set_focused(app, NSApplication::sharedApplication(mtm).isActive());
    }
    let became = app.clone();
    let active = RcBlock::new(move |_note: NonNull<NSNotification>| {
        crate::desk::set_focused(&became, true);
    });
    let resigned = app.clone();
    let inactive = RcBlock::new(move |_note: NonNull<NSNotification>| {
        crate::desk::set_focused(&resigned, false);
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: AppKit's constant notification names; no object filter; the
    // blocks run on the posting (main) thread and capture only Send state.
    unsafe {
        let observers = [
            center.addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidBecomeActiveNotification),
                None,
                None,
                &active,
            ),
            center.addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidResignActiveNotification),
                None,
                None,
                &inactive,
            ),
        ];
        // Observed for the life of the app.
        std::mem::forget(observers);
    }
}
