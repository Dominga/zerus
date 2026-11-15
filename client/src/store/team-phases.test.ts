// @vitest-environment happy-dom
import { expect, it, vi } from "vitest";
import { phaseFollower, removeDraftNode, type DraftNode, type PhaseFollow } from "./team-phases";
import fixture from "../ui/teams/fixtures/phase-pane.json";

it("coalesces selection changes and closes only its last acknowledged epoch", async () => {
  const waiting: ((epoch: number) => void)[] = [];
  const port = vi.fn<PhaseFollow>().mockImplementation(() => new Promise((r) => waiting.push(r)));
  const follow = phaseFollower("example-team", port);
  follow.select(null); follow.select(1); follow.select(2);
  expect(port.mock.calls).toEqual([["example-team", null, true, null]]);
  waiting.shift()!(10);
  await vi.waitFor(() => expect(port).toHaveBeenCalledTimes(2));
  expect(port.mock.calls[1]).toEqual(["example-team", 2, true, null]);
  expect(follow.epoch()).toBeNull();
  follow.close(); waiting.shift()!(11);
  await vi.waitFor(() => expect(port).toHaveBeenCalledTimes(3));
  expect(port.mock.calls[2]).toEqual(["example-team", null, false, 11]);
  waiting.shift()!(12);
  expect(follow.epoch()).toBeNull();
});

it("a failed first open never sends cleanup with another pane's epoch", async () => {
  const port = vi.fn<PhaseFollow>().mockRejectedValue(new Error("unavailable"));
  const follow = phaseFollower("example-team", port); follow.select(null);
  await vi.waitFor(() => expect(follow.error()).toContain("unavailable"));
  follow.close();
  await Promise.resolve();
  expect(port).toHaveBeenCalledTimes(1);
});

it("keeps persisted IDs and compacts only unsaved IDs and their dependency links", () => {
  const saved = structuredClone(fixture.plan.draft.nodes) as DraftNode[];
  const fresh = [3, 4, 5].map((id) => ({ ...structuredClone(saved[0]), id, deps: [{ node: id - 1 }] }));
  const result = removeDraftNode([...saved, ...fresh], 3, 3);
  expect(result.map((n) => n.id)).toEqual([1, 2, 3, 4]);
  expect(result[2].deps).toEqual([]);
  expect(result[3].deps).toEqual([{ node: 3 }]);
  expect(removeDraftNode(result, 1, 5).map((n) => n.id)).toEqual([2, 3, 4]);
  expect(removeDraftNode(result, 1, 5)[0].deps).toEqual([]);
  expect(saved[1].deps).toEqual([{ node: 1 }]);
});
