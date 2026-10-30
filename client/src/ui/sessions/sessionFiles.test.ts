import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionFileView } from "../../bridge";
import type { Session } from "./types";

const calls = vi.hoisted(() => ({
  dialog: vi.fn(),
  check: vi.fn(),
  notice: vi.fn(),
  close: vi.fn(),
  download: vi.fn(),
  save: vi.fn(),
  act: vi.fn(),
  external: vi.fn(),
  changed: vi.fn(),
}));
vi.mock("../../bridge", () => ({
  sessionFileDialog: calls.dialog,
  sessionFileCheck: calls.check,
  onSessionFileNotice: calls.notice,
  sessionFileClose: calls.close,
  sessionFileDownload: calls.download,
  sessionFileSave: calls.save,
  sessionFileAct: calls.act,
  sessionFileExternal: calls.external,
  onSessionFileChanged: calls.changed,
}));
vi.mock("../../store/ui", () => ({ toast: vi.fn() }));

import { actOnSessionFile, closeSessionFile, downloadSessionFile, saveSessionFile, openExternal, openSessionFile, sessionFile } from "./sessionFiles";

const changed = calls.changed.mock.calls[0][0] as (view: SessionFileView) => void;

const view: SessionFileView = {
  id: 7, revision: 0, computer: "peer", key: "run:1", href: "/remote/report.md", path: "/remote/report.md", location: ":4", remote: true,
  title: "File on peer", local_available: false, can_download: true, checking: false, downloading: false, picking: false, error: "", hint: "Download", closed: false,
};

describe("session file dialogs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    closeSessionFile();
    calls.dialog.mockResolvedValue(view);
    calls.check.mockResolvedValue(view);
    calls.save.mockResolvedValue(view);
    calls.close.mockResolvedValue(undefined);
    calls.download.mockResolvedValue(view);
    calls.act.mockResolvedValue(view);
    calls.external.mockResolvedValue(undefined);
  });

  it("uses the session computer, catalog key and cwd, never a local replacement", async () => {
    openSessionFile({ computer: "peer", catalogKey: "run:1", cwd: "work", cwdPath: "/remote/work" } as Session, "/remote/report.md");
    await vi.waitFor(() => expect(sessionFile()).toEqual(view));
    expect(calls.dialog).toHaveBeenCalledWith("peer", "run:1", "/remote/report.md", "/remote/work");
    actOnSessionFile("open_file");
    expect(calls.act).toHaveBeenCalledWith(7, "open_file");
    actOnSessionFile("open_ssh");
    expect(calls.act).toHaveBeenCalledWith(7, "open_ssh");
    downloadSessionFile(true);
    expect(calls.download).toHaveBeenCalledWith(7, true);
  });

  it("closes the dialog without cancelling a running download", async () => {
    openSessionFile({ computer: "peer", catalogKey: "run:1", cwdPath: "/remote/work" } as Session, "/remote/report.md");
    await vi.waitFor(() => expect(sessionFile()).toEqual(view));
    closeSessionFile();
    expect(sessionFile()).toBeUndefined();
    expect(calls.close).toHaveBeenCalledWith(7);
    expect(calls.download).not.toHaveBeenCalled();
  });

  it("opens external URLs through the guarded opener", () => {
    openExternal("https://example.com/report");
    expect(calls.external).toHaveBeenCalledWith("https://example.com/report");
  });
  it("does not reopen a closed dialog or replace a newer dialog when an action finishes late", async () => {
    openSessionFile({ computer: "peer", catalogKey: "run:1" } as Session, "/remote/report.md");
    await vi.waitFor(() => expect(sessionFile()?.id).toBe(7));
    let finish!: (view: SessionFileView) => void;
    calls.download.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    downloadSessionFile(true); closeSessionFile();
    calls.dialog.mockResolvedValueOnce({ ...view, id: 8 });
    openSessionFile({ computer: "peer", catalogKey: "run:2" } as Session, "/remote/other.md");
    await vi.waitFor(() => expect(sessionFile()?.id).toBe(8));
    finish({ ...view, revision: 4, downloading: true });
    await Promise.resolve();
    expect(sessionFile()?.id).toBe(8);
  });
  it("closes an obsolete open reply and ignores an older revision of a cache check", async () => {
    let finish!: (view: SessionFileView) => void;
    calls.dialog.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    openSessionFile({ computer: "peer", catalogKey: "run:1" } as Session, "/remote/report.md");
    closeSessionFile(); finish(view); await Promise.resolve();
    expect(sessionFile()).toBeUndefined(); expect(calls.close).toHaveBeenCalledWith(7);
    openSessionFile({ computer: "peer", catalogKey: "run:1" } as Session, "/remote/report.md");
    await vi.waitFor(() => expect(sessionFile()?.id).toBe(7));
    changed({ ...view, revision: 3, local_available: true });
    changed({ ...view, revision: 1, checking: true });
    expect(sessionFile()?.local_available).toBe(true); expect(sessionFile()?.checking).toBe(false);
  });
  it("uses only the dialog id for native Save As and keeps a cancelled save open", async () => {
    openSessionFile({ computer: "peer", catalogKey: "run:1" } as Session, "/remote/report.md");
    await vi.waitFor(() => expect(sessionFile()?.id).toBe(7));
    calls.save.mockResolvedValueOnce(null); saveSessionFile(); await Promise.resolve();
    expect(calls.save).toHaveBeenCalledWith(7); expect(sessionFile()?.id).toBe(7);
  });

});
