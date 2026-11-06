import { describe, expect, it, vi } from "vitest";

vi.mock("../../bridge", () => ({ readTaskDetails: vi.fn() }));
import { readSelectedTask } from "./task-view-read";

const detail = (uuid: string) => ({ uuid, id: "T-abcdef", stage: "done", text: "Finished long ago" });

describe("a selected task beyond the loaded rows", () => {
  it("is read on its own, waiting out the inspector's one-read permit", async () => {
    const read = vi.fn()
      .mockRejectedValueOnce(new Error("Task details are already loading."))
      .mockResolvedValueOnce(detail("u-1"));
    const task = await readSelectedTask("p1\nu-1", read);
    expect(task.uuid).toBe("u-1"); expect(task.stage).toBe("done");
    expect(read).toHaveBeenCalledTimes(2);
    expect(read).toHaveBeenLastCalledWith("p1", "u-1", null);
  });
  it("refuses another task's reply and does not retry other failures", async () => {
    await expect(readSelectedTask("p1\nu-1", vi.fn().mockResolvedValue(detail("u-2")))).rejects.toThrow();
    const failing = vi.fn().mockRejectedValue(new Error("no task u-1 in this project"));
    await expect(readSelectedTask("p1\nu-1", failing)).rejects.toThrow("no task");
    expect(failing).toHaveBeenCalledTimes(1);
  });
});
