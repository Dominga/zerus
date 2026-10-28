# A Dev base names the next planned stable release; CI supplies its sequence.
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/../DEV_VERSION" ZERUS_DEV_BASE_VERSION)
string(STRIP "${ZERUS_DEV_BASE_VERSION}" ZERUS_DEV_BASE_VERSION)
if(NOT ZERUS_DEV_BASE_VERSION MATCHES "^(0|[1-9][0-9]*)[.](0|[1-9][0-9]*)[.](0|[1-9][0-9]*)$"
   OR NOT ZERUS_DEV_BASE_VERSION VERSION_GREATER PROJECT_VERSION)
    message(FATAL_ERROR "DEV_VERSION must name a release newer than the stable product version")
endif()
set(ZERUS_DEV_BUILD_NUMBER "$ENV{ZERUS_DEV_BUILD_NUMBER}" CACHE STRING "Verified main CI build sequence")
set(ZERUS_LOCAL_PREVIEW_LABEL "" CACHE STRING "Label for an explicitly requested local branch preview")
if(NOT ZERUS_LOCAL_PREVIEW_LABEL STREQUAL "")
    string(LENGTH "${ZERUS_LOCAL_PREVIEW_LABEL}" ZERUS_PREVIEW_LABEL_LENGTH)
    if(NOT ZERUS_LOCAL_PREVIEW_LABEL MATCHES "^[a-z0-9]+(-[a-z0-9]+)*$" OR ZERUS_PREVIEW_LABEL_LENGTH GREATER 32)
        message(FATAL_ERROR "A local preview label must contain at most 32 lowercase letters, digits or separated hyphens")
    endif()
    if(NOT ZERUS_DEV_BUILD_NUMBER STREQUAL "" OR NOT ZERUS_CLEAN_SOURCE)
        message(FATAL_ERROR "A local branch preview requires clean Git source and no Dev build number")
    endif()
    target_compile_definitions(hgs-updates PRIVATE
        ZERUS_DEV_BASE_VERSION="${ZERUS_DEV_BASE_VERSION}"
        ZERUS_LOCAL_PREVIEW_LABEL="${ZERUS_LOCAL_PREVIEW_LABEL}")
endif()
if(NOT ZERUS_DEV_BUILD_NUMBER STREQUAL "")
    string(LENGTH "${ZERUS_DEV_BUILD_NUMBER}" ZERUS_DEV_NUMBER_LENGTH)
    if(NOT ZERUS_DEV_BUILD_NUMBER MATCHES "^[1-9][0-9]*$" OR ZERUS_DEV_NUMBER_LENGTH GREATER 15)
        message(FATAL_ERROR "ZERUS_DEV_BUILD_NUMBER must be a positive integer of at most 15 digits")
    endif()
    if(NOT ZERUS_CLEAN_SOURCE)
        message(FATAL_ERROR "A numbered Dev build requires a clean Git source tree")
    endif()
    target_compile_definitions(hgs-updates PRIVATE
        ZERUS_DEV_BASE_VERSION="${ZERUS_DEV_BASE_VERSION}"
        ZERUS_DEV_BUILD_NUMBER=${ZERUS_DEV_BUILD_NUMBER})
endif()

# Fork candidates have a separate identity; upstream semantic versions and
# saved application/session identifiers remain unchanged. No publisher is enabled.
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/../FORK_VERSION")
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/../FORK_VERSION" ZERUS_FORK_VERSION)
string(STRIP "${ZERUS_FORK_VERSION}" ZERUS_FORK_VERSION)
if(NOT ZERUS_FORK_VERSION MATCHES "^[0-9][0-9][0-9][0-9][.][0-9]+[.][0-9]+-[1-9][0-9]*$")
    message(FATAL_ERROR "FORK_VERSION must be YYYY.M.D-N")
endif()
target_compile_definitions(hgs-updates PRIVATE ZERUS_FORK_VERSION="${ZERUS_FORK_VERSION}")
