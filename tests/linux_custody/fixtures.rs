//! Private root/descendant images; never an installed CLI entry.
use super::*;

pub(super) fn root(case: &str, dir: &Path) {
    if case == "cleanup-late" {
        std::thread::sleep(Duration::from_millis(100));
        std::process::exit(17);
    }
    if case == "external" {
        let mut byte = [0];
        assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
        std::process::exit(17);
    }
    let mut stream = UnixStream::connect(dir.join("root.sock")).unwrap();
    frame(&mut stream, "ready");
    if case == "owner-death" {
        let owner = unsafe { libc::getppid() };
        let mut byte = [0];
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
        let end = deadline();
        while unsafe { libc::getppid() } == owner {
            assert!(Instant::now() < end);
            tick();
        }
        let mut survivor = UnixStream::connect(dir.join("after-owner.sock")).unwrap();
        frame(
            &mut survivor,
            &format!("{} {}", std::process::id(), unsafe { libc::getppid() }),
        );
        assert_eq!(read_frame(&mut survivor), "exit");
        std::process::exit(17);
    }
    let mut request = read_frame(&mut stream);
    if request == "alive" {
        frame(&mut stream, "alive");
        request = read_frame(&mut stream);
    }
    if request == "fork" {
        if matches!(case, "clone" | "policy-gap") {
            // fork-like clone with no SIGCHLD exit signal; ordinary wait omits it.
            let child =
                unsafe { libc::syscall(libc::SYS_clone, 0usize, 0usize, 0usize, 0usize, 0usize) };
            assert!(child >= 0);
            if child == 0 {
                descendant(dir, None);
            }
        } else {
            // Double fork plus a new session; no process-group membership proof.
            let first = unsafe { libc::fork() };
            assert!(first >= 0);
            if first == 0 {
                assert!(unsafe { libc::setsid() } > 0);
                let mut ready = [0; 2];
                assert_eq!(unsafe { libc::pipe(ready.as_mut_ptr()) }, 0);
                let second = unsafe { libc::fork() };
                assert!(second >= 0);
                if second == 0 {
                    descendant(dir, Some(ready[1]));
                }
                let mut byte = 0u8;
                assert_eq!(
                    unsafe { libc::read(ready[0], (&mut byte as *mut u8).cast(), 1) },
                    1
                );
                unsafe {
                    libc::_exit(0);
                }
            }
            let mut status = 0;
            assert_eq!(unsafe { libc::waitpid(first, &mut status, 0) }, first);
        }
        frame(&mut stream, "forked");
        assert_eq!(read_frame(&mut stream), "root-exit");
    } else {
        assert_eq!(request, "exit");
    }
    unsafe {
        libc::_exit(23);
    }
}

fn descendant(dir: &Path, ready: Option<i32>) -> ! {
    let end = deadline();
    let original = unsafe { libc::getppid() };
    let mut stream = UnixStream::connect(dir.join("child.sock")).unwrap();
    if let Some(fd) = ready {
        assert_eq!(unsafe { libc::write(fd, b"!".as_ptr().cast(), 1) }, 1);
    }
    frame(&mut stream, &std::process::id().to_string());
    loop {
        let parent = unsafe { libc::getppid() };
        if parent != original {
            frame(&mut stream, &format!("adopted {parent}"));
            break;
        }
        assert!(Instant::now() < end);
        tick();
    }
    assert_eq!(read_frame(&mut stream), "child-exit");
    unsafe {
        libc::_exit(17);
    }
}
