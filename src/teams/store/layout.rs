//! Private Teams core allocations (zerus-63rru.176). Timer extension layouts
//! and their raw pinned marks live in their separate metadata namespace.
pub(crate) const UNSUPPORTED_PRE_RELEASE: u32 = 1;
pub(crate) const LEGACY_QUEUE: u32 = 2;
pub(crate) const LEGACY_PLANNING: u32 = 3;
pub(crate) const NATIVE_PERSON: u32 = 4;
pub(crate) const PUBLICATION: u32 = 5;
/// Reserved for slice 4; this engine must still refuse it.
pub(crate) const REWORK: u32 = 6;
/// Reserved for .31 slice 2; this engine must still refuse it.
pub(crate) const NATIVE_PERSON_CONTROL: u32 = 7;
pub(crate) const ALLOCATED: [u32; 7] = [
    UNSUPPORTED_PRE_RELEASE,
    LEGACY_QUEUE,
    LEGACY_PLANNING,
    NATIVE_PERSON,
    PUBLICATION,
    REWORK,
    NATIVE_PERSON_CONTROL,
];
pub(crate) const fn supported(version: u32) -> bool {
    matches!(
        version,
        LEGACY_QUEUE | LEGACY_PLANNING | NATIVE_PERSON | PUBLICATION
    )
}
pub(crate) const fn native(version: u32) -> bool {
    matches!(version, NATIVE_PERSON | PUBLICATION)
}
