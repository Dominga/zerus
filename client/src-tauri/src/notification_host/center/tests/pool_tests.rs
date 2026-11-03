//! Real Objective-C autorelease and Block_copy destruction, without a call to
//! Notification Center, permission UI, a signed host, or a notification post.
use super::*;
use objc2::{
    define_class, msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{NSObject, NSObjectProtocol},
    AnyThread, DefinedClass,
};

struct BlockIvars {
    _copy: RcBlock<dyn Fn()>,
}
define_class!(
    // SAFETY: NSObject has no additional subclass invariants. Native dealloc
    // drops the ivars and therefore the actual copied Objective-C block.
    #[unsafe(super(NSObject))]
    #[name = "ZerusNotificationAutoreleaseBlockFixture"]
    #[ivars = BlockIvars]
    struct BlockOwner;
    unsafe impl NSObjectProtocol for BlockOwner {}
);

fn autorelease_copy(block: RcBlock<dyn Fn()>) {
    let object = BlockOwner::alloc().set_ivars(BlockIvars {
        _copy: block.copy(),
    });
    // SAFETY: NSObject's designated initializer; its only Rust ivar is live.
    let object: Retained<BlockOwner> = unsafe { msg_send![super(object), init] };
    // Registers with the innermost REAL native pool. Do not immediately drop
    // this holder: its delayed deallocation is the production retention seam.
    let _ = Retained::autorelease_ptr(object);
}

fn copied_completion(post: bool) {
    autoreleasepool(|_| {
        let command = command();
        let mut host = Host::new(command.binding.session.clone()).unwrap();
        drop(host.next_output());
        let fake = Fake::default();
        let now = Instant::now();
        host.receive(wire::ToHost::Command(command), &fake, now)
            .unwrap();
        if post {
            fake.0.lock().unwrap().take().unwrap().complete(true);
            host.poll(&fake, now);
        }
        let callback = fake.0.lock().unwrap().take().unwrap();
        let root = std::env::temp_dir().join(format!("zerus-pool-block-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let owner = Arc::new(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join("owner"))
                .unwrap(),
        );
        let weak = Arc::downgrade(&owner);
        let block: RcBlock<dyn Fn()> = if post {
            let block = post_reply(callback, owner);
            RcBlock::new(move || block.call((std::ptr::null_mut(),)))
        } else {
            let settings = settings_reply(callback.clone(), owner.clone());
            let block = authorization_reply(callback, owner);
            RcBlock::new(move || {
                let _earlier_settings_copy = &settings;
                block.call((Bool::NO, std::ptr::null_mut()));
            })
        };
        let native_copy = block.copy();
        native_turn(|| {
            autorelease_copy(block.copy());
            block.call(());
            host.poll(&fake, now);
            // Completion may publish its ORIGINAL receipt while both actual
            // native copies still own the job. Drain is a separate condition.
            assert!(matches!(
                host.next_output().unwrap().frame,
                wire::FromHost::Receipt(_)
            ));
            host.poll(&fake, now);
            assert!(host.next_output().is_none());
        });
        drop(block);
        host.poll(&fake, now);
        assert!(host.next_output().is_none());
        assert!(
            weak.upgrade().is_some(),
            "explicit native copy still owns it"
        );
        drop(native_copy);
        host.poll(&fake, now);
        assert!(matches!(
            host.next_output().unwrap().frame,
            wire::FromHost::Released { .. }
        ));
        assert!(
            weak.upgrade().is_none(),
            "the turn's autoreleased copy was destroyed"
        );
        host.receive(
            wire::ToHost::Stop {
                session: host_session(),
            },
            &fake,
            now,
        )
        .unwrap();
        host.poll(&fake, now);
        assert!(matches!(
            host.next_output().unwrap().frame,
            wire::FromHost::Stopped { .. }
        ));
        std::fs::remove_dir_all(root).unwrap();
    });
}

fn host_session() -> wire::Session {
    command().binding.session
}

#[test]
fn authorization_autorelease_destruction_and_last_native_copy_gate_release() {
    copied_completion(false);
}

#[test]
fn post_autorelease_destruction_and_last_native_copy_gate_release() {
    copied_completion(true);
}

#[test]
fn native_pool_runs_on_the_actual_callback_thread() {
    // No SDK dispatch is assumed: run the real copy/pool/destructor control on
    // a separate thread, where a later pool on the main thread cannot help.
    std::thread::spawn(|| {
        copied_completion(false);
        copied_completion(true);
    })
    .join()
    .unwrap();
}

#[test]
fn a_later_wait_pool_does_not_drain_a_previous_sdk_turn() {
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    struct ObserveDrop(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for ObserveDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    autoreleasepool(|_| {
        let guard = ObserveDrop(dropped.clone());
        autorelease_copy(RcBlock::new(move || {
            let _retained = &guard;
        }));
        native_turn(|| {}); // The former run-loop-only pool placement.
        assert!(!dropped.load(Ordering::Acquire));
    });
    assert!(dropped.load(Ordering::Acquire));
}
