import { describe, expect, it } from "vitest";
import fixture from "../../../core/tests/fixtures/folder-paths.json";
import type { LaunchProject, PathStyle } from "../../types";
import * as paths from "./paths";

// The shared golden fixture (core/tests/fixtures/folder-paths.json): the core
// test core/tests/folder_paths.rs checks the Rust answers, this the port's.

const STYLES: PathStyle[] = ["posix", "windows"];

describe("folder paths (paths.rs)", () => {
  it("answers each path as the core does", () => {
    expect(fixture.paths.length).toBeGreaterThan(40);
    for (const entry of fixture.paths) {
      for (const style of STYLES) {
        const want = entry[style];
        const label = `${style} ${JSON.stringify(entry.path)}`;
        expect(paths.absolute(entry.path, style), `absolute ${label}`).toBe(want.absolute);
        expect(paths.clean(entry.path, style), `clean ${label}`).toBe(want.clean);
        expect(paths.leaf(entry.path, style), `leaf ${label}`).toBe(want.leaf);
        expect(paths.native(entry.path, style), `native ${label}`).toBe(want.native);
      }
    }
  });

  it("compares and nests paths as the core does", () => {
    for (const entry of fixture.pairs) {
      for (const style of STYLES) {
        const label = `${style} ${JSON.stringify(entry.path)} ${JSON.stringify(entry.folder)}`;
        expect(paths.same(entry.path, entry.folder, style), `same ${label}`).toBe(entry[style].same);
        expect(paths.within(entry.path, entry.folder, style), `within ${label}`).toBe(entry[style].within);
      }
    }
  });

  it("takes this computer's style from its platform, else from the build", () => {
    for (const entry of fixture.styles) {
      for (const build of STYLES) {
        const want = entry.style === "build" ? build : entry.style;
        expect(paths.of(entry.platform, entry.local, build), JSON.stringify(entry)).toBe(want);
      }
    }
  });

});

describe("New session's folder helpers (launch.rs)", () => {
  it("finds the deepest project folder holding a path", () => {
    for (const entry of fixture.matching_project_folder) {
      const answer = paths.matchingProjectFolder(entry.path, entry.canonical, entry.project as LaunchProject | null, entry.host, entry.style as PathStyle);
      expect(answer, JSON.stringify(entry)).toBe(entry.answer);
    }
  });

  it("opens a linked worktree's session on its main checkout", () => {
    for (const entry of fixture.prefill_target) {
      const answer = paths.prefillTarget(entry.catalog, entry.folder, entry.canonical, entry.project as LaunchProject | null, entry.host, entry.style as PathStyle);
      expect(answer, JSON.stringify(entry)).toBe(entry.answer);
    }
  });

  it("reopens a failed creation on its folder", () => {
    for (const entry of fixture.restored_folder) {
      expect(paths.restoredFolder(entry.plan as Record<string, unknown>), JSON.stringify(entry.plan)).toEqual(entry.answer);
    }
  });
});
