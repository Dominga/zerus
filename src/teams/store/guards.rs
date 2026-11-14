//! Rule guards are definite refusals, separate from failed storage IO.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Guard {
    MissingDirectory,
    DirectoryPrivacy,
    FilePrivacy,
    Dependencies,
    LiveNodes,
    ObservedTeam,
    RetargetedTeam,
    CatalogReply,
    LaunchDirectory,
    LaunchPrivacy,
    LaunchSchema,
    LaunchHealth,
    LaunchKey,
    LaunchRevision,
    LaunchOriginal,
    LaunchRecord,
}

impl Guard {
    pub(crate) fn problem(self) -> Problem {
        let (code, message) = match self {
            Self::MissingDirectory => (code::INVALID_STATE, "Teams store path has no directory."),
            Self::DirectoryPrivacy => {
                (code::INVALID_STATE, "Teams store directory is not private.")
            }
            Self::FilePrivacy => (code::INVALID_STATE, "Teams store file is not private."),
            Self::Dependencies => (code::OVER_CAP, "Too many node dependencies."),
            Self::LiveNodes => (code::OVER_CAP, "Live node count exceeds its bound."),
            Self::ObservedTeam => (code::INVALID_STATE, "Observed team disappeared."),
            Self::RetargetedTeam => (code::INVALID_STATE, "Retargeted team disappeared."),
            Self::CatalogReply => (code::INVALID_STATE, "A catalog reply must be an object."),
            Self::LaunchDirectory => (code::INVALID_STATE, "Teams directory is not private"),
            Self::LaunchPrivacy => (code::INVALID_STATE, "Member launch ledger is not private"),
            Self::LaunchSchema => (
                code::INVALID_STATE,
                "Member launch schema is newer than this Zerus",
            ),
            Self::LaunchHealth => (code::INVALID_STATE, "Launch health singleton is missing"),
            Self::LaunchKey => (code::INVALID_STATE, "Member launch key changed"),
            Self::LaunchRevision => (code::OVER_CAP, "Launch revision overflow"),
            Self::LaunchOriginal => (code::INVALID_STATE, "Member launch original changed"),
            Self::LaunchRecord => (code::OVER_CAP, "Member launch record exceeds its bound"),
        };
        Problem::new(code, message)
    }
}
