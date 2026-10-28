import { beforeEach, describe, expect, it, vi } from "vitest";
import * as bridge from "../bridge";
import * as ui from "./ui";
import { startDesk } from "./desk";

vi.mock("../bridge", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../bridge")>()),
  onOpenSession: vi.fn(() => () => {}),
  onDeskNotice: vi.fn(() => () => {}),
  deskTakePending: vi.fn(async () => ({ open: null, notices: [] })),
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
  collapsed: {},
  revealRow: vi.fn(),
  toast: vi.fn(),
}));
const mocked = vi.mocked(bridge);
const shown = vi.mocked(ui);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("desk wiring", () => {
  beforeEach(() => vi.clearAllMocks());

  it("opens the session the menu bar or a notification names", () => {
    let open: ((target: bridge.OpenSession) => void) | undefined;
    mocked.onOpenSession.mockImplementation((listener) => {
      open = listener;
      return () => {};
    });
    const stop = startDesk();
    open!({ computer: "mac", key: "claude/sample-project/review" });
    expect(shown.setPage).toHaveBeenCalledWith("sessions");
    expect(shown.setSelectedKey).toHaveBeenCalledWith("mac\nclaude/sample-project/review");
    expect(shown.revealRow).toHaveBeenCalledWith("mac\nclaude/sample-project/review");
    stop();
  });

  it("clears every filter first, as Qt's showSession does", () => {
    let open: ((target: bridge.OpenSession) => void) | undefined;
    mocked.onOpenSession.mockImplementation((listener) => {
      open = listener;
      return () => {};
    });
    startDesk();
    open!({ computer: "", key: "k1" });
    expect(shown.setStateFilter).toHaveBeenCalledWith("all");
    expect(shown.setMachineFilter).toHaveBeenCalledWith([]);
    expect(shown.setProjectFilter).toHaveBeenCalledWith([]);
    expect(shown.setSearch).toHaveBeenCalledWith("");
    expect(shown.setSubagentOpen).toHaveBeenCalledWith(undefined);
  });

  it("applies what arrived before the page loaded", async () => {
    mocked.deskTakePending.mockResolvedValueOnce({
      open: { computer: "", key: "k1" },
      notices: [{ title: "Zerus", text: "This session is no longer available: claude/sample-project/x" }],
    });
    startDesk();
    await settle();
    expect(shown.setSelectedKey).toHaveBeenCalledWith("\nk1");
    expect(shown.toast).toHaveBeenCalledWith("Zerus", "This session is no longer available: claude/sample-project/x");
  });

  it("shows notices as toasts", () => {
    let notice: ((value: bridge.DeskNotice) => void) | undefined;
    mocked.onDeskNotice.mockImplementation((listener) => {
      notice = listener;
      return () => {};
    });
    startDesk();
    notice!({ title: "Zerus", text: "System notification was not delivered: denied" });
    expect(shown.toast).toHaveBeenCalledWith("Zerus", "System notification was not delivered: denied");
  });
});
