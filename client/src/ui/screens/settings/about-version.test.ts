import { describe, expect, it } from "vitest";
import { installedVersion } from "./about-version";

const commit = "abcdef0123456789abcdef0123456789abcdef01";

describe("the installed version", () => {
  it("reads as UpdateController::installedVersion", () => {
    // A numbered Dev build is its version name.
    expect(installedVersion({ versionName: "0.47.0-dev.1520", devBuildNumber: 1520, sourceCommit: commit, sourceDirty: false })).toBe("0.47.0-dev.1520");
    // A local preview names itself.
    expect(installedVersion({ versionName: "0.47.0-preview.mac.abcdef0", devBuildNumber: 0, sourceCommit: commit, previewLabel: "mac" })).toBe("0.47.0-preview.mac.abcdef0");
    // An unnumbered build names its source, and local changes to it.
    expect(installedVersion({ versionName: "0.46.2", devBuildNumber: 0, sourceCommit: commit, sourceDirty: false })).toBe("0.46.2 (abcdef0)");
    expect(installedVersion({ versionName: "0.46.2", devBuildNumber: 0, sourceCommit: commit, sourceDirty: true })).toBe("0.46.2 (local changes abcdef0)");
    // Without its source, the version alone.
    expect(installedVersion({ versionName: "0.46.2", devBuildNumber: 0, sourceCommit: null, sourceDirty: null })).toBe("0.46.2");
  });
});
