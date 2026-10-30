import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionFileView } from "../../bridge";
import type { Session } from "./types";

const calls = vi.hoisted(() => ({
  dialog: vi.fn(),
  close: vi.fn(),
  download: vi.fn(),
  save: vi.fn(),
  act: vi.fn(),
  external: vi.fn(),
  changed: vi.fn(),
}));
vi.mock("../../bridge", () => ({
  sessionFileDialog: calls.dialog,
  sessionFileClose: calls.close,
  sessionFileDownload: calls.download,
  sessionFileSave: calls.save,
  sessionFileAct: calls.act,
  sessionFileExternal: calls.external,
  onSessionFileChanged: calls.changed,
}));
vi.mock("../../store/ui", () => ({ toast: vi.fn() }));

import { actOnSessionFile, closeSessionFile, downloadSessionFile, openExternal, openSessionFile, sessionFile } from "./sessionFiles";

const view: SessionFileView = {
  id: 7, computer: "peer", key: "run:1", href: "/remote/report.md", path: "/remote/report.md", location: ":4", remote: true,
  title: "File on peer", local_path: "", copy_path: "", checking: false, downloading: false, saved: false, error: "", notice: "", closed: false,
};

describe("session file dialogs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    closeSessionFile();
    calls.dialog.mockResolvedValue(view);
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
    downloadSessionFile(true);
    expect(calls.download).toHaveBeenCalledWith(7, true);
  });

  it("closes the dialog without cancelling a running download", () => {
    closeSessionFile();
    expect(sessionFile()).toBeUndefined();
    expect(calls.close).toHaveBeenCalledWith(7);
    expect(calls.download).not.toHaveBeenCalled();
  });

  it("opens external URLs through the guarded opener", () => {
    openExternal("https://example.com/report");
    expect(calls.external).toHaveBeenCalledWith("https://example.com/report");
  });
});
