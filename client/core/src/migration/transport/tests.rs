use super::*;
use crate::migration::SourceStream;
use zerus_desktop_migration::SourceRecord;

#[test]
fn writer_failure_stops_decoding_before_the_next_source_item() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let source = SourceStream::new(
        SourceRecord {
            family: "synthetic".into(),
            schema: 1,
            format: "json".into(),
            sha256: sha256(b"synthetic"),
            build: None,
        },
        std::iter::from_fn(|| -> Option<Result<serde_json::Value, SourceError>> {
            panic!("no source read after broken start header")
        }),
    );
    let bundle = Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        "11111111-1111-4111-8111-111111111111".into(),
        1,
        vec![source],
        1 << 20,
    )
    .unwrap();
    assert_eq!(
        encode(bundle, &mut Broken).unwrap_err(),
        PublishError::Unconfirmed
    );
}
