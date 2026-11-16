import { expect, it, vi } from "vitest";
import { createTeams, type TeamsTransport, type TeamView } from "./teams";
import frames from "../ui/teams/fixtures/phase-estimates-view.json";
const transport = () => ({ follow: vi.fn().mockResolvedValue(undefined), hidden: vi.fn().mockResolvedValue(undefined) } as unknown as TeamsTransport);
const product = () => structuredClone(frames.published) as unknown as TeamView;
it("a product batch retirement removes derived estimates instead of merging old optional data", async () => {
  const store = createTeams(transport()), value = product(); await store.openTeam(value.bar.id);
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "team", payload: value }]);
  expect(store.team()?.plan?.estimates?.rows[0].estimate_minutes).toBe(30);
  const retired = { ...value, plan: { ...value.plan!, observed: null, estimates: null } };
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "team", payload: retired }]);
  expect(store.team()?.plan?.estimates).toBeNull();
});
it("canonical read failure clears both observed and derived output", async () => {
  const store = createTeams(transport()), value = product(); await store.openTeam(value.bar.id);
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "team", payload: value }]);
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "failed", payload: { code: "storage_error", message: "Source unavailable" } }]);
  expect(store.team()?.plan?.observed).toBeNull(); expect(store.team()?.plan?.estimates).toBeNull();
});
it("a different ComputerId cannot carry the local derived page into this store", async () => {
  const store = createTeams(transport()), value = product(); await store.openTeam(value.bar.id);
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "team", payload: value }]);
  store.apply([{ computer: "other-computer-id", key: `team:${value.bar.id}`, kind: "team", payload: value }]);
  expect(store.team()?.plan?.estimates).toBeNull(); expect(store.team()?.plan?.observed).toBeNull();
});
it("changing the team drops the old estimates and rejects its late product frame", async () => {
  const store = createTeams(transport()), value = product(); await store.openTeam(value.bar.id);
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "team", payload: value }]);
  await store.openTeam("00000000-0000-4000-8000-00000000abcd");
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "team", payload: value }]);
  expect(store.team()?.plan?.estimates).toBeNull(); expect(store.team()?.plan?.observed).toBeNull();
});
it("hide immediately drops derived and observed data before the native response", async () => {
  const store = createTeams(transport()), value = product(); await store.openTeam(value.bar.id);
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "team", payload: value }]);
  await store.hidden(true);
  store.apply([{ computer: "", key: `team:${value.bar.id}`, kind: "team", payload: value }]);
  expect(store.team()?.plan?.estimates).toBeNull(); expect(store.team()?.plan?.observed).toBeNull();
  await store.hidden(false); expect(store.team()?.plan?.estimates).toBeNull();
});
