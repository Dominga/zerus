import { describe, expect, it } from "vitest";
import { createBuildInfo, type BuildInfo } from "./buildInfo";

describe("the shared native app identity", () => {
  it("coalesces rail/About reads and keeps the actual native result", async () => {
    let finish!: (value: BuildInfo) => void;
    let reads = 0;
    const build = createBuildInfo(() => {
      reads++;
      return new Promise((resolve) => { finish = resolve; });
    });
    expect(build.info()).toBeUndefined();
    const rail = build.refresh();
    const about = build.refresh();
    expect(rail).toBe(about);
    expect(reads).toBe(1);
    const stable = { versionName: "0.48.0", devBuildNumber: 0 };
    finish(stable);
    await rail;
    expect(build.info()).toEqual(stable);
    await build.refresh();
    expect(reads).toBe(1);
  });

  it("keeps failed reads unknown and allows a later open to retry", async () => {
    let reads = 0;
    const build = createBuildInfo(async () => {
      if (++reads === 1) throw Error("unavailable");
      return { versionName: "0.48.0-dev.123", devBuildNumber: 123 };
    });
    await build.refresh();
    expect(build.info()).toBeUndefined();
    await build.refresh();
    expect(build.info()?.versionName).toBe("0.48.0-dev.123");
    expect(reads).toBe(2);
  });

  it("does not invent a browser/replay identity or invoke native commands there", async () => {
    const build = createBuildInfo(async () => { throw Error("unexpected native read"); }, () => false);
    await build.refresh();
    expect(build.info()).toBeUndefined();
  });
});
