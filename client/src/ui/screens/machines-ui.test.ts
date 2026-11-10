import { describe, expect, it } from "vitest";
import { LOCAL, localState, machineKey, peerState, placeholders, reconnectLine, reconnectOutcome, reconnectReady, reconnectShown, retryDelaysDraft, sessionHostLine, summary, testEnabled } from "./machines-ui";
import { machineMeta } from "../../store/testing";
import { emptyMachines, type MachineRow } from "../../types";

const row = (extra: Partial<MachineRow> = {}): MachineRow => ({
  alias: "studio", enabled: true, source: "local", inherited: false, hostname: "", user: "", port: null, identity_file: "", origin: "", ...extra,
});
const at = (status: string, attempt = 0) => ({ id: "studio", status, attempt });

describe("machines page", () => {
  it("selects this computer by its LOCAL key and a peer by its alias", () => {
    expect(machineKey("")).toBe(LOCAL);
    expect(machineKey("studio")).toBe("studio");
  });

  it("words a peer's state line as Qt, with the stream's states", () => {
    expect(peerState(row({ enabled: false }), at("live"), 2)).toEqual({ tile: "disabled", line: "Disabled", live: false });
    expect(peerState(row(), at("live"), 1)).toEqual({ tile: "online", line: "Connected, 1 session", live: true });
    expect(peerState(row(), at("live"), 3).line).toBe("Connected, 3 sessions");
    expect(peerState(row(), at("reconnecting", 2), 3)).toEqual({ tile: "connecting", line: "Reconnecting", live: false });
    expect(peerState(row(), at("probing"), 0)).toEqual({ tile: "connecting", line: "Connecting", live: false });
    expect(peerState(row(), at("unsupported"), 0)).toEqual({ tile: "offline", line: "Update hgs", live: false });
    expect(peerState(row(), undefined, 0)).toEqual({ tile: "offline", line: "Not connected", live: false });
  });

  it("counts like Qt: a control console is no machine of the fleet", () => {
    expect(summary({ console: false, live: true }, [{ live: true }, { live: false }])).toBe("2 connected, 3 machines");
    expect(summary({ console: true, live: true }, [{ live: true }])).toBe("1 connected, 1 machine");
    expect(summary({ console: false, live: false }, [])).toBe("0 connected, 1 machine");
  });

  it("describes this computer, a control console and a Windows session host", () => {
    expect(localState(machineMeta("workstation"), "")).toMatchObject({
      name: "workstation", console: false, line: "This machine", formHidden: false, canEnable: false,
      origin: "This device runs Zerus. Add a remote machine to manage its agents over SSH.",
    });
    expect(localState(undefined, "workstation").name).toBe("workstation");
    const host = { state: "running", pid: 4120, protocol: 1, conpty: "bundled", integrity: "Medium", windows_session: 1, error: null };
    const console = localState(machineMeta("win-lab", {}, { platform: "windows", local_sessions: false, local_host: host }), "");
    expect(console).toMatchObject({
      console: true, canEnable: true, formHidden: true, line: "This computer: control console",
      origin: "This computer is a control console: agents run on the remote machines you add here, over SSH. Enable local sessions to run them on this computer too.",
      sessionHost: "Session host: running, PID 4120, bundled ConPTY, Medium integrity, Windows session 1",
    });
    expect(sessionHostLine({ state: "not-installed", pid: null, protocol: null, conpty: null, integrity: null, windows_session: null, error: "access denied" }))
      .toBe("Session host: not installed: access denied");
  });

  it("shows inherited values once the effective connection is known, and tests only an idle saved peer", () => {
    const e = { alias: "studio", pending: false, error: false, text: "", hostname: "studio.example", user: "person", port: "2222", key: "/home/example/.ssh/id_ed25519" };
    expect(placeholders(e, "studio")).toEqual({
      hostname: "Inherited: studio.example", user: "Inherited: person", port: "Inherited: 2222", key: "Inherited: /home/example/.ssh/id_ed25519",
    });
    expect(placeholders({ ...e, pending: true }, "studio").hostname).toBe("Address or existing SSH alias");
    expect(placeholders(e, "build-01").user).toBe("Use SSH configuration");
    expect(placeholders(null, "studio")).toEqual({
      hostname: "Address or existing SSH alias", user: "Use SSH configuration", port: "SSH configuration (usually 22)", key: "Use SSH agent / configuration",
    });
    const view = { ...emptyMachines(), loaded: true, machines: [row()] };
    expect(testEnabled("studio", view)).toBe(true);
    expect(testEnabled(LOCAL, view)).toBe(false);
    expect(testEnabled("studio", { ...view, busy: true })).toBe(false);
    expect(testEnabled("gone", view)).toBe(false);
  });
});

describe("machine reconnect", () => {
  const conn = (status: string, retry?: import("../../store/retry").RetryState) => ({ id: "studio", status, attempt: 1, retry, via: "service" as const });
  const wait = { phase: "waiting" as const, source: "7", serial: 3, attempt: 1, delayMs: 5000, dueAt: 5000 };

  it("shows the actual wait, a running attempt or a wait an older service does not report", () => {
    expect(reconnectLine(conn("live"), 0)).toEqual({ state: "Connected", busy: false });
    expect(reconnectLine(conn("reconnecting", wait), 4)).toEqual({ state: "Not connected", detail: "Next attempt in 4 s", busy: false });
    expect(reconnectLine(conn("reconnecting", wait), 0)).toEqual({ state: "Not connected", detail: "Connecting…", busy: true });
    expect(reconnectLine(conn("reconnecting", { phase: "attempting", source: "7", serial: 3, attempt: 1 }), 9).detail).toBe("Connecting…");
    expect(reconnectLine(conn("reconnecting"), 0).detail).toBe("Waiting to reconnect");
    expect(reconnectLine(conn("probing"), 0).busy).toBe(true);
    expect(reconnectLine(undefined, 0)).toEqual({ state: "Not connected", busy: false });
  });

  it("offers Reconnect now only while a supervisor waits to reconnect", () => {
    expect(reconnectShown(conn("live"))).toBe(false);
    expect(reconnectShown(conn("ended"))).toBe(false);
    expect(reconnectShown(conn("unsupported"))).toBe(false);
    expect(reconnectShown(conn("probing"))).toBe(true);
    expect(reconnectReady(conn("probing"))).toBe(false);
    expect(reconnectReady(conn("reconnecting", wait))).toBe(true);
    expect(reconnectReady(conn("reconnecting"))).toBe(true);
    expect(reconnectReady(conn("reconnecting", { phase: "attempting", source: "7", serial: 3, attempt: 1 }))).toBe(false);
  });

  it("explains every reply that the state itself does not show", () => {
    expect(reconnectOutcome("requested")).toBeNull();
    expect(reconnectOutcome("connected")).toBeNull();
    for (const code of ["pending", "source_changed", "not_configured", "not_admitted", "stopping", "fleet_unavailable", "fleet_request_unsupported"]) {
      expect(reconnectOutcome(code)).toBeTruthy();
    }
    expect(reconnectOutcome("odd")).toBe("Could not reconnect: odd");
  });

  it("accepts four whole non-decreasing seconds from 1 to 3600", () => {
    expect(retryDelaysDraft(["1", "2", "5", "10"])).toEqual({ delays: [1, 2, 5, 10] });
    expect(retryDelaysDraft([" 3", "3", "3", "3600"]).delays).toEqual([3, 3, 3, 3600]);
    for (const bad of [["0", "2", "5", "10"], ["1", "2", "5", "3601"], ["1", "2.5", "5", "10"], ["1", "", "5", "10"], ["1", "-2", "5", "10"], ["1", "2", "5"]]) {
      expect(retryDelaysDraft(bad).error).toBe("Use whole seconds from 1 to 3600.");
    }
    expect(retryDelaysDraft(["5", "2", "5", "10"]).error).toBe("Each delay must be at least the one before.");
  });
});
