import { beforeEach, describe, expect, it, vi } from "vitest";
import * as bridge from "../bridge";
import * as ui from "./ui";
import { startDesk } from "./desk";

vi.mock("../bridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../bridge")>()),
  onOpenSession: vi.fn(async () => () => {}),
  onDeskNotice: vi.fn(async () => () => {}),
  deskPage: vi.fn(async () => 3),
  deskTakePending: vi.fn(async () => ({ open: null, notices: [] })),
}));
// One catalog row, in a project the list shows collapsed.
vi.mock("./catalog", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./catalog")>()),
  catalog: { rows: { "mac\nrun:folded": { project: "sample-project" } }, order: [] },
}));
// store/ui reads the page URL when it loads; the wiring needs only its setters.
vi.mock("./ui", () => ({
  setPage: vi.fn(),
  setSelectedKey: vi.fn(),
  setSubagentOpen: vi.fn(),
  setStateFilter: vi.fn(),
  setMachineFilter: vi.fn(),
  setProjectFilter: vi.fn(),
  setSearch: vi.fn(),
  setCollapsed: vi.fn(),
  collapsed: { "sample-project": true } as Record<string, boolean>,
  revealRow: vi.fn(),
  toast: vi.fn(),
}));
const mocked = vi.mocked(bridge);
const shown = vi.mocked(ui);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Starts the wiring and returns the open-session listener it registered. */
async function opener(): Promise<(target: bridge.OpenSession) => void> {
  let open: ((target: bridge.OpenSession) => void) | undefined;
  mocked.onOpenSession.mockImplementation(async (listener) => {
    open = listener;
    return () => {};
  });
  await startDesk();
  return open!;
}

describe("desk wiring", () => {
  beforeEach(() => vi.clearAllMocks());

  it("opens the session the menu bar or a notification names", async () => {
    const stop = vi.fn();
    let open: ((target: bridge.OpenSession) => void) | undefined;
    mocked.onOpenSession.mockImplementation(async (listener) => {
      open = listener;
      return stop;
    });
    const stopDesk = await startDesk();
    open!({ computer: "mac", key: "claude/sample-project/review" });
    expect(shown.setPage).toHaveBeenCalledWith("sessions");
    expect(shown.setSelectedKey).toHaveBeenCalledWith("mac\nclaude/sample-project/review");
    expect(shown.revealRow).toHaveBeenCalledWith("mac\nclaude/sample-project/review");
    stopDesk();
    expect(stop).toHaveBeenCalled();
  });

  it("clears every filter first, as Qt's showSession does", async () => {
    const open = await opener();
    open({ computer: "", key: "k1" });
    expect(shown.setStateFilter).toHaveBeenCalledWith("all");
    expect(shown.setMachineFilter).toHaveBeenCalledWith([]);
    expect(shown.setProjectFilter).toHaveBeenCalledWith([]);
    expect(shown.setSearch).toHaveBeenCalledWith("");
    expect(shown.setSubagentOpen).toHaveBeenCalledWith(undefined);
  });

  it("expands the collapsed project of the session it opens", async () => {
    const open = await opener();
    open({ computer: "mac", key: "run:folded" });
    expect(shown.setCollapsed).toHaveBeenCalledWith("sample-project", false);
    expect(shown.setSelectedKey).toHaveBeenCalledWith("mac\nrun:folded");
    vi.clearAllMocks();
    open({ computer: "mac", key: "run:elsewhere" });
    expect(shown.setCollapsed).not.toHaveBeenCalled();
  });

  it("takes what waited only once both listeners are registered", async () => {
    let listening!: (stop: () => void) => void;
    mocked.onDeskNotice.mockImplementationOnce(() => new Promise((resolve) => { listening = resolve; }));
    const started = startDesk();
    await settle();
    expect(mocked.deskPage).not.toHaveBeenCalled();
    expect(mocked.deskTakePending).not.toHaveBeenCalled();
    listening(() => {});
    await started;
    await settle();
    expect(mocked.deskTakePending).toHaveBeenCalledWith(3);
  });

  it("applies what arrived before the page loaded", async () => {
    mocked.deskTakePending.mockResolvedValueOnce({
      open: { computer: "", key: "k1" },
      notices: [{ title: "Zerus", text: "This session is no longer available: claude/sample-project/x" }],
    });
    await startDesk();
    await settle();
    expect(mocked.deskTakePending).toHaveBeenCalledWith(3);
    expect(shown.setSelectedKey).toHaveBeenCalledWith("\nk1");
    expect(shown.toast).toHaveBeenCalledWith("Zerus", "This session is no longer available: claude/sample-project/x");
  });

  it("shows notices as toasts", async () => {
    let notice: ((value: bridge.DeskNotice) => void) | undefined;
    mocked.onDeskNotice.mockImplementation(async (listener) => {
      notice = listener;
      return () => {};
    });
    await startDesk();
    notice!({ title: "Zerus", text: "System notification was not delivered: denied" });
    expect(shown.toast).toHaveBeenCalledWith("Zerus", "System notification was not delivered: denied");
  });
});
