//! Behavior helpers and legacy names for the sole Teams version registry.
//! Numeric allocations live only in `versions`; timers use a separate mark.
pub(crate) const UNSUPPORTED_PRE_RELEASE: u32 = super::versions::RETIRED_PREVIEW;
pub(crate) const LEGACY_QUEUE: u32 = super::versions::CORE_LEGACY;
pub(crate) const LEGACY_PLANNING: u32 = super::versions::CORE_PLANNING;
pub(crate) const NATIVE_PERSON: u32 = super::versions::CORE_NATIVE_PERSON;
pub(crate) const PUBLICATION: u32 = super::versions::CORE_PUBLICATION;
/// Activated only by the first SendBack fact in slice 4.
pub(crate) const REWORK: u32 = super::versions::CORE_REWORK;
/// Reserved for .31 slice 2; this engine must still refuse it.
pub(crate) const NATIVE_PERSON_CONTROL: u32 = super::versions::CORE_NATIVE_FLOOR;
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
        LEGACY_QUEUE | LEGACY_PLANNING | NATIVE_PERSON | PUBLICATION | REWORK
    )
}
pub(crate) const fn native(version: u32) -> bool {
    matches!(version, NATIVE_PERSON | PUBLICATION | REWORK)
}
