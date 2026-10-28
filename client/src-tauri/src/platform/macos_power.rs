//! System sleep and wake for the main-thread watchdog, as Qt's
//! `tray/src/MacSystemPower.{h,cpp}`. IOKit's root power domain notifies a
//! serial dispatch queue of its own, never the main thread, which macOS may
//! not schedule while it goes to sleep or in a dark wake. Sleep is never
//! vetoed: queries and sleep notices are acknowledged at once. IOKit has no
//! binding among the client's crates, so the few calls are declared here.
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::watchdog::{now_ms, Pulse};

/// `io_object_t`, `io_connect_t` and `io_service_t`: Mach port names.
type IoObject = u32;
type NotificationPort = *mut c_void;
type InterestCallback =
    extern "C" fn(refcon: *mut c_void, service: IoObject, message: u32, argument: *mut c_void);

// `IOKit/IOMessage.h`: iokit_common_msg(n) is 0xe0000000 | n.
const CAN_SYSTEM_SLEEP: u32 = 0xe000_0270;
const SYSTEM_WILL_SLEEP: u32 = 0xe000_0280;
const SYSTEM_HAS_POWERED_ON: u32 = 0xe000_0300;
const SYSTEM_WILL_POWER_ON: u32 = 0xe000_0320;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IORegisterForSystemPower(
        refcon: *mut c_void,
        port: *mut NotificationPort,
        callback: InterestCallback,
        notifier: *mut IoObject,
    ) -> IoObject;
    fn IOAllowPowerChange(kernel_port: IoObject, notification_id: isize) -> i32;
    fn IONotificationPortSetDispatchQueue(port: NotificationPort, queue: *mut c_void);
}

extern "C" {
    // libdispatch, part of libSystem; a null attribute makes a serial queue.
    fn dispatch_queue_create(label: *const c_char, attributes: *mut c_void) -> *mut c_void;
}

struct Context {
    pulse: Arc<Pulse>,
    /// The root power domain connection, set before the first notice arrives.
    root: AtomicU32,
}

extern "C" fn received(
    refcon: *mut c_void,
    _service: IoObject,
    message: u32,
    argument: *mut c_void,
) {
    // SAFETY: `observe` passes a Context it never frees.
    let context = unsafe { &*refcon.cast::<Context>() };
    match message {
        // An unanswered query delays idle sleep by 30 s.
        CAN_SYSTEM_SLEEP => allow(context, argument),
        SYSTEM_WILL_SLEEP => {
            context.pulse.will_sleep(now_ms());
            allow(context, argument);
        }
        SYSTEM_WILL_POWER_ON | SYSTEM_HAS_POWERED_ON => context.pulse.powered_on(now_ms()),
        _ => {}
    }
}

fn allow(context: &Context, argument: *mut c_void) {
    // SAFETY: the connection IORegisterForSystemPower returned; the argument is the notice's ID.
    unsafe { IOAllowPowerChange(context.root.load(Ordering::Acquire), argument as isize) };
}

/// Holds the watchdog's check around sleep and dark wakes for the life of the
/// process. Without the notices a wake may abort the client, as Qt warns.
pub fn observe(pulse: Arc<Pulse>) {
    let context = Box::into_raw(Box::new(Context {
        pulse,
        root: AtomicU32::new(0),
    }));
    let mut port: NotificationPort = std::ptr::null_mut();
    let mut notifier: IoObject = 0;
    // SAFETY: a plain registration; the context outlives every callback, since it is never freed.
    let root =
        unsafe { IORegisterForSystemPower(context.cast(), &mut port, received, &mut notifier) };
    if root == 0 {
        eprintln!("zerus-client: cannot observe system sleep; the main-thread watchdog may abort after a wake");
        // SAFETY: IOKit kept no reference to the context.
        drop(unsafe { Box::from_raw(context) });
        return;
    }
    // SAFETY: the context is live; the queue and the port last as long as the process.
    unsafe {
        (*context).root.store(root, Ordering::Release);
        let queue = dispatch_queue_create(c"hgs-system-power".as_ptr(), std::ptr::null_mut());
        IONotificationPortSetDispatchQueue(port, queue);
    }
}
