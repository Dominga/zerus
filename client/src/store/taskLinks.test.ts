import { describe, expect, it, vi } from "vitest";
import { createNoticeBus } from "./notices";
import { LINK_NOTICE, openLinkedReference, SHORT_REFERENCE, type LinkedReferencePort } from "./taskLinks";

const UUID = "11111111-1111-4111-8111-111111111111";
function port(resolve: LinkedReferencePort["resolve"], unchanged = true) {
  return { resolve: vi.fn(resolve), open: vi.fn(), notify: vi.fn(), unchanged: () => unchanged };
}

describe("a short task reference beyond a windowed list (zerus-cg5h.27)", () => {
  it("opens the task its bounded read confirmed", async () => {
    const p = port(async () => UUID);
    await openLinkedReference("example", "T-ab12cd", p);
    expect(p.resolve).toHaveBeenCalledExactlyOnceWith("example", "T-ab12cd");
    expect(p.open).toHaveBeenCalledExactlyOnceWith("example", UUID);
    expect(p.notify).not.toHaveBeenCalled();
  });
  it("fails closed: nothing opens and the board says the task was not found", async () => {
    const p = port(async () => { throw new Error("no task T-ab12cd in this project"); });
    await openLinkedReference("example", "T-ab12cd", p);
    expect(p.open).not.toHaveBeenCalled();
    expect(p.notify).toHaveBeenCalledExactlyOnceWith("error", "Task T-ab12cd was not found in this project.", false, LINK_NOTICE);
  });
  it("a later link's notice takes the place of the last instead of stacking", async () => {
    const bus = createNoticeBus();
    for (const reference of ["T-ab12cd", "T-zz99zz", "T-zz99zz"]) {
      await openLinkedReference("example", reference, { ...port(async () => { throw new Error("no task"); }), notify: bus.notify });
    }
    expect(bus.notices().map((n) => n.text)).toEqual(["Task T-zz99zz was not found in this project."]);
  });
  it("does not open later by surprise once another task or project was opened", async () => {
    const p = port(async () => UUID, false);
    await openLinkedReference("example", "T-ab12cd", p);
    expect(p.open).not.toHaveBeenCalled();
    expect(p.notify).not.toHaveBeenCalled();
  });
  it("recognizes a short ID alone or as a task's ref, and nothing else", () => {
    for (const ok of ["T-ab12cd", "Sample project/T-ab12cd", "team/sample/T-000000"]) expect(SHORT_REFERENCE.test(ok), ok).toBe(true);
    for (const bad of ["T-AB12CD", "T-ab12c", "T-ab12cde", "ab12cd", UUID, "/T-ab12cd", "a\nb/T-ab12cd", "T-ab12cd/x"]) expect(SHORT_REFERENCE.test(bad), bad).toBe(false);
  });
});
