//! Whether a local pipe server may stand for the Zerus frontend. Windows reads
//! the facts from the server's own process; the decision is platform-neutral so
//! every refusal is covered without a live adversarial process.

/// What is known about one process: its user, integrity level, logon session
/// and the file its image was loaded from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessFacts {
    /// The token user's SID bytes.
    pub user: Vec<u8>,
    /// The mandatory integrity RID (low 0x1000, medium 0x2000, high 0x3000).
    pub integrity: u32,
    pub session: u32,
    /// Volume serial number and full 128-bit file ID of the image, when readable.
    pub image: Option<(u64, u128)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Trusted,
    OtherUser,
    LowerIntegrity,
    OtherSession,
    OtherImage,
}

/// A server stands for the frontend only with this process's user and logon
/// session, at least this process's integrity, and exactly the maintained
/// image. Anything else is a refusal: never absence and never a live GUI.
pub fn verdict(server: &ProcessFacts, own: &ProcessFacts, image: (u64, u128)) -> Verdict {
    if server.user != own.user {
        Verdict::OtherUser
    } else if server.integrity < own.integrity {
        Verdict::LowerIntegrity
    } else if server.session != own.session {
        Verdict::OtherSession
    } else if server.image != Some(image) {
        Verdict::OtherImage
    } else {
        Verdict::Trusted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own() -> ProcessFacts {
        ProcessFacts {
            user: vec![1, 5, 0, 0, 0, 0, 0, 5, 21],
            integrity: 0x2000,
            session: 1,
            image: Some((7, 42)),
        }
    }

    #[test]
    fn only_the_same_user_session_and_image_at_no_lower_integrity_is_trusted() {
        let image = (7, 42);
        assert_eq!(verdict(&own(), &own(), image), Verdict::Trusted);
        let higher = ProcessFacts {
            integrity: 0x3000,
            ..own()
        };
        assert_eq!(verdict(&higher, &own(), image), Verdict::Trusted);
        let cases = [
            (
                ProcessFacts {
                    user: vec![1, 5, 0, 0, 0, 0, 0, 5, 22],
                    ..own()
                },
                Verdict::OtherUser,
            ),
            (
                ProcessFacts {
                    integrity: 0x1000,
                    ..own()
                },
                Verdict::LowerIntegrity,
            ),
            (
                ProcessFacts {
                    integrity: 0,
                    ..own()
                },
                Verdict::LowerIntegrity,
            ),
            (
                ProcessFacts {
                    session: 2,
                    ..own()
                },
                Verdict::OtherSession,
            ),
            (
                ProcessFacts {
                    image: Some((7, 43)),
                    ..own()
                },
                Verdict::OtherImage,
            ),
            (
                ProcessFacts {
                    image: Some((8, 42)),
                    ..own()
                },
                Verdict::OtherImage,
            ),
            (
                ProcessFacts {
                    image: None,
                    ..own()
                },
                Verdict::OtherImage,
            ),
        ];
        for (server, expected) in cases {
            assert_eq!(verdict(&server, &own(), image), expected, "{server:?}");
        }
        // The user check comes first: another user is never reported as anything else.
        let foreign = ProcessFacts {
            user: vec![9],
            integrity: 0,
            session: 9,
            image: None,
        };
        assert_eq!(verdict(&foreign, &own(), image), Verdict::OtherUser);
    }
}
