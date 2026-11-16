// @vitest-environment happy-dom
import { afterEach, expect, it } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { PhaseEstimates } from "./PhaseEstimates";
import type { PhaseEstimatesView } from "../../store/phase-estimates";
import produced from "./fixtures/phase-estimates.json";

let stop: (() => void) | undefined;
afterEach(() => { stop?.(); document.body.replaceChildren(); });

function mount(value?: PhaseEstimatesView, readFailed = false) {
  const host = document.createElement("div"); document.body.append(host);
  stop = render(() => createComponent(PhaseEstimates, { value, readFailed }), host);
  return host;
}

// Synthetic stale/corrupt DTOs test suppression, not positive product rows.
const stale = (): PhaseEstimatesView => ({
  scope: { team: "00000000-0000-4000-8000-000000000001", home: "00000000-0000-4000-8000-000000000002",
    revision: 3, basis: "unapproved_preview", version: { phase: 1, draft_generation: 2, publication: null } },
  cohort_complete: true, unknown: null,
  rows: [{ node: 1, title: "Stale estimate control", kind: "task", task: null, estimate_minutes: 30, unknown: null }],
});

it("renders product-written publication estimates per unit without summing an elapsed time", () => {
  const host = mount(produced.published as PhaseEstimatesView);
  expect([...host.querySelectorAll("[data-estimate-minutes]")].map(node => node.textContent)).toEqual(["30 min", "30 min"]);
  expect(host.textContent).toContain("Approved publication estimates");
  expect(host.textContent).not.toContain("60 min");
  expect(host.querySelector("button,input,textarea")).toBeNull();
});

it("labels the product-written draft as an unapproved preview without an actual comparison", () => {
  const host = mount(produced.draft as PhaseEstimatesView);
  expect(host.textContent).toContain("Unapproved draft preview");
  expect(host.textContent).toContain("This preview is not approved and has no actual comparison.");
});

it("keeps the product-written partial publication numeric values unavailable", () => {
  const host = mount(produced.partial as PhaseEstimatesView);
  expect(host.querySelector("[data-estimate-minutes]")).toBeNull();
  expect(host.textContent).toContain("Unknown");
});

it("an older core with no projection cannot fabricate a zero estimate", () => {
  const host = mount();
  expect(host.textContent).toContain("Estimates are unavailable for the selected scope.");
  expect(host.querySelector("[data-estimate-minutes]")).toBeNull();
  expect(host.querySelector("table")).toBeNull();
});

it("a failed selected read suppresses a synthetic retained projection", () => {
  const host = mount(stale(), true);
  expect(host.querySelector("[data-estimate-minutes]")).toBeNull();
  expect(host.textContent).not.toContain("Stale estimate control");
  expect(host.querySelector("button,input,textarea")).toBeNull();
});

it("unavailable history never substitutes current rows", () => {
  const host = mount({ scope: null, cohort_complete: false, unknown: "history_unavailable", rows: [] });
  expect(host.textContent).toContain("That publication or generation was not returned by this read.");
  expect(host.querySelector("[data-estimate-minutes]")).toBeNull();
});

it("a corrupt partial DTO cannot promote its retained number to a known estimate", () => {
  const value = stale(); value.cohort_complete = false; value.unknown = "incomplete_live";
  const host = mount(value);
  expect(host.textContent).toContain("The complete published phase snapshot has not been read.");
  expect(host.textContent).toContain("Unknown");
  expect(host.querySelector("[data-estimate-minutes]")).toBeNull();
});
