use super::*;

#[test]
fn retiring_an_original_listener_preserves_a_later_socket_or_file() {
    for replacement in ["socket", "file"] {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: temp.path().canonicalize().unwrap().join("state/desktop"),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let path = location.root.join("service.sock");
        let original = Listener::bind(&location).unwrap();
        if replacement == "socket" {
            let later = Listener::bind(&location).unwrap();
            drop(original);
            assert!(
                path.exists(),
                "retiring an old actor removed its successor's endpoint"
            );
            assert!(connect(&location).unwrap().is_some());
            drop(later);
            assert!(
                !path.exists(),
                "the last original still removes its own endpoint"
            );
        } else {
            std::fs::remove_file(&path).unwrap();
            std::fs::write(&path, b"keep this replacement").unwrap();
            drop(original);
            assert_eq!(std::fs::read(&path).unwrap(), b"keep this replacement");
        }
    }
}
