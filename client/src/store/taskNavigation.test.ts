import { describe, expect, it, vi } from "vitest";

vi.hoisted(() => vi.stubGlobal("location", new URL("http://localhost/")));
// The inspector reads the skin's stylesheet; its panel stays as it is here.
vi.mock("./inspector", () => ({ inspectorOpen: () => true, inspectorTab: () => "tasks", openInspector: () => {} }));
const { linkBoard, setSelectedTask } = await import("./taskNavigation");
const { setDetailTab, setSubagentOpen } = await import("./ui");
const { openLinkedReference } = await import("./taskLinks");

const UUID = "11111111-1111-4111-8111-111111111111";
/** A link whose bounded read is still pending while `meanwhile` navigates; what it opens after. */
async function linkWhile(meanwhile: () => void) {
  let finish!: (uuid: string) => void;
  const open = vi.fn(), notify = vi.fn();
  const pending = openLinkedReference("example", "T-ab12cd", {
    resolve: () => new Promise<string>((resolve) => { finish = resolve; }), open, notify, unchanged: linkBoard(),
  });
  meanwhile();
  finish(UUID);
  await pending;
  return { open, notify };
}

describe("a late short-reference link result (zerus-cg5h.27)", () => {
  it("opens its task while nothing was navigated", async () => {
    const { open, notify } = await linkWhile(() => {});
    expect(open).toHaveBeenCalledExactlyOnceWith("example", UUID);
    expect(notify).not.toHaveBeenCalled();
  });
  it("stays put once the detail tab was switched during the read", async () => {
    setDetailTab("activity");
    const { open } = await linkWhile(() => setDetailTab("terminal"));
    expect(open).not.toHaveBeenCalled();
  });
  it("stays put once a subagent was opened during the read", async () => {
    setSubagentOpen(undefined);
    const { open } = await linkWhile(() => setSubagentOpen("agent-1"));
    expect(open).not.toHaveBeenCalled();
    setSubagentOpen(undefined);
  });
  it("stays put once another task was opened during the read", async () => {
    setSelectedTask(undefined);
    const { open } = await linkWhile(() => setSelectedTask({ project: "example", uuid: "22222222-2222-4222-8222-222222222222" }));
    expect(open).not.toHaveBeenCalled();
    setSelectedTask(undefined);
  });
});
