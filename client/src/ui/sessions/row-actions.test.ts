import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  processes: false, offline: false, hasTerminal: true,
  select: vi.fn(), subagent: vi.fn(), page: vi.fn(), tab: vi.fn(), opened: vi.fn(), notify: vi.fn(), native: vi.fn(), inspector: vi.fn(),
}));
vi.mock("../../store/settings", () => ({ flag: (name: string) => name === "processes/enabled" && mocks.processes }));
vi.mock("../../store/catalog", () => ({ computerOffline: () => mocks.offline }));
vi.mock("../../store/actions", () => ({ notify: mocks.notify }));
vi.mock("../../store/terminal", () => ({ terminal: { opened: mocks.opened } }));
vi.mock("../../store/ui", () => ({ setSelectedKey: mocks.select, setSubagentOpen: mocks.subagent, setPage: mocks.page, setDetailTab: mocks.tab }));
vi.mock("../../store/inspector", () => ({ openInspector: mocks.inspector }));
vi.mock("./actions-ui", () => ({ terminal: () => mocks.hasTerminal }));
vi.mock("./native-entry", () => ({ nativeRoute: () => mocks.native }));

import { openChip, openTerminal, terminalAvailable } from "./row-actions";
import type { Session } from "./types";

const session = (extra: Partial<Session> = {}) => ({ key: "k1", computer: "", agent: "claude", ...extra }) as Session;

// Where a click on a card's chip goes (SessionsWindow chipActivated), and Open terminal.
describe("a card's chips and Open terminal", () => {
  beforeEach(() => {
    Object.values(mocks).forEach((m) => typeof m === "function" && m.mockReset());
    mocks.processes = false; mocks.offline = false; mocks.hasTerminal = true;
  });

  it("opens services and background commands in Processes when it is on", () => {
    mocks.processes = true;
    for (const kind of ["services", "background"] as const) {
      mocks.tab.mockClear();
      openChip(session(), kind);
      expect(mocks.select).toHaveBeenCalledWith("k1");
      expect(mocks.tab).toHaveBeenCalledWith("processes");
    }
    expect(mocks.opened).not.toHaveBeenCalled();
  });

  it("goes to the terminal when Processes is off, and to the native DeepSeek UI for DeepSeek", async () => {
    openChip(session(), "services");
    await vi.waitFor(() => expect(mocks.tab).toHaveBeenCalledWith("terminal"));
    expect(mocks.opened).toHaveBeenCalled();
    mocks.tab.mockClear();
    openChip(session({ agent: "deepseek" }), "background");
    expect(mocks.native).toHaveBeenCalled();
    expect(mocks.tab).not.toHaveBeenCalled();
  });

  it("says so when the session has no terminal, instead of opening nothing", () => {
    mocks.hasTerminal = false;
    openChip(session(), "services");
    expect(mocks.notify).toHaveBeenCalledWith("error", "This session has no terminal to attach to.", true);
    expect(mocks.tab).not.toHaveBeenCalled();
  });

  it("does not use Processes of a machine that does not answer", async () => {
    mocks.processes = true; mocks.offline = true;
    openChip(session(), "background");
    await vi.waitFor(() => expect(mocks.tab).toHaveBeenCalledWith("terminal"));
  });

  it("opens the side panel's checkouts for a git chip, once it loads", async () => {
    openChip(session(), "git");
    expect(mocks.select).toHaveBeenCalledWith("k1");
    await vi.waitFor(() => expect(mocks.inspector).toHaveBeenCalledWith("worktrees"));
  });

  it("opens the terminal of the row from the menu and only offers it when it can work", async () => {
    openTerminal(session());
    await vi.waitFor(() => expect(mocks.tab).toHaveBeenCalledWith("terminal"));
    expect(mocks.select).toHaveBeenCalledWith("k1");
    expect(mocks.subagent).toHaveBeenCalledWith(undefined);
    expect(mocks.page).toHaveBeenCalledWith("sessions");
    expect(mocks.opened).toHaveBeenCalled();
    expect(mocks.tab).toHaveBeenCalledWith("terminal");
    expect(terminalAvailable(session())).toBe(true);
    mocks.hasTerminal = false;
    expect(terminalAvailable(session())).toBe(false);
    mocks.hasTerminal = true; mocks.offline = true;
    expect(terminalAvailable(session())).toBe(false);
  });
});
