// @vitest-environment happy-dom
// The member card (the fork's MemberCard, adapted): its role and colour, its
// state word, the account and the session it runs on, and only the M1 member
// operations.
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { MenuLayer } from "../components/Overlays";
import { setMenu } from "../../store/ui";
import { MemberCard } from "./MemberCard";
import { resolveIcon } from "../lib/icons";
import type { MemberView } from "./types";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); setMenu(undefined); });

const member = (over: Partial<MemberView> = {}): MemberView => ({
  id: "m1", role: "developer", agent: "claude", account: "account-example", state: "active", lead: false,
  session: { machine: "00000000-0000-4000-8000-0000000000aa", session_id: "launch_id:00000000-0000-4000-8000-000000000031" },
  ...over,
});

function setup(value: MemberView, onAction = vi.fn()) {
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => [createComponent(MemberCard, { teamStatus: "running", member: value, busy: false, onAction }), createComponent(MenuLayer, {})], host);
  const openDetails = () => {
    (host.querySelector('[aria-label="Member actions"]') as HTMLButtonElement).click();
    ([...host.querySelectorAll<HTMLButtonElement>(".menu-item")].find((button) => button.textContent === "Member details"))!.click();
  };
  return { host, onAction, openDetails };
}

it("gives Member details an icon like its neighbours, and says Hide once the details are open", () => {
  const { host, openDetails } = setup(member());
  (host.querySelector('[aria-label="Member actions"]') as HTMLButtonElement).click();
  const items = [...host.querySelectorAll<HTMLButtonElement>(".menu-item")];
  const details = items.find((button) => button.textContent === "Member details")!;
  expect(details.getAttribute("role")).toBe("menuitem");
  const info = document.createElement("div");
  info.innerHTML = `<svg>${resolveIcon("info").html}</svg>`;
  expect(details.querySelector("svg")!.innerHTML, "the info glyph, not an empty check slot").toBe(info.querySelector("svg")!.innerHTML);
  for (const item of items) expect(item.querySelector("svg")!.innerHTML, item.textContent!).not.toBe("");
  (host.querySelector('[aria-label="Member actions"]') as HTMLButtonElement).click();
  openDetails();
  (host.querySelector('[aria-label="Member actions"]') as HTMLButtonElement).click();
  expect([...host.querySelectorAll(".menu-item")].map((button) => button.textContent)).toContain("Hide member details");
});

it("keeps a linked session as a plain localized fact without a technical keyboard stop (review 13)", () => {
  const { host, openDetails } = setup(member());
  openDetails();
  expect(host.querySelector(".team-member-name")?.textContent).toBe("m1");
  expect(host.querySelector(".role-badge")?.textContent).toBe("developer");
  expect(host.querySelector(".tm-state-badge")).toBeNull();
  expect(host.textContent).toContain("account-example");
  const session = host.querySelector(".team-member-session")!;
  expect(session.textContent).toBe("Session linked");
  expect(session.hasAttribute("tabindex")).toBe(false);
  expect(session.hasAttribute("aria-label")).toBe(false);
  expect(host.innerHTML).not.toContain("launch_id:");
});

it("marks the lead and a state's tone", () => {
  const { host } = setup(member({ id: "lead", role: "lead", lead: true }));
  expect(host.querySelector(".team-member-card")?.classList.contains("team-lead")).toBe(true);
  expect(host.querySelector(".tm-state-badge")).toBeNull();
  expect(host.querySelector(".team-member-card")?.getAttribute("data-state-color")).toBe("info");
  expect(host.querySelector(".team-member-lead")?.textContent).toBe("lead");
});

it("shows a not-created member without a session", () => {
  const { host, openDetails } = setup(member({ state: "not_created", account: "", session: null }));
  openDetails();
  expect(host.querySelector(".tm-state-badge")?.getAttribute("data-tone")).toBe("dim");
  expect(host.textContent).toContain("not created");
  expect(host.textContent).toContain("the default account");
  expect(host.querySelector(".team-member-session")?.textContent).toBe("No linked session");
});

it("offers member operations and a disabled swap, and sends by member id", () => {
  const { host, onAction } = setup(member());
  (host.querySelector('[aria-label="Member actions"]') as HTMLButtonElement).click();
  const buttons = [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].filter((button) => button.textContent !== "Member details");
  expect(buttons.map((button) => button.textContent)).toEqual(["Pause", "Stop", "Swap…", "Remove"]);
  expect(buttons.filter((button) => button.classList.contains("danger"))).toHaveLength(2);
  expect(buttons.find((button) => button.textContent === "Swap…")?.getAttribute("aria-disabled")).toBe("true");
  buttons[0].click();
  expect(onAction).toHaveBeenCalledWith("pause_member", "m1");
});

it("offers resume only to a paused member", () => {
  const { host } = setup(member({ state: "paused" }));
  (host.querySelector('[aria-label="Member actions"]') as HTMLButtonElement).click();
  const buttons = [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].filter((button) => button.textContent !== "Member details");
  expect(buttons.map((button) => button.textContent)).toEqual(["Resume", "Stop", "Swap…", "Remove"]);
});

it("shows Stop disabled with its reason for a member the person stopped, and Resume open", () => {
  const { host } = setup(member({ state: "paused", person_stopped: true }));
  (host.querySelector('[aria-label="Member actions"]') as HTMLButtonElement).click();
  const items = [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
  const stop = items.find((item) => item.textContent === "Stop")!;
  expect(stop.getAttribute("aria-disabled")).toBe("true");
  expect(stop.title).toBe("You stopped this member. Resume it to let it work again.");
  expect(items.find((item) => item.textContent === "Resume")?.getAttribute("aria-disabled")).not.toBe("true");
});

it("keeps lead Pause/Stop and offers no Remove, which the store refuses for the lead", () => {
  const { host, onAction } = setup(member({ lead: true }));
  (host.querySelector('[aria-label="Member actions"]') as HTMLButtonElement).click();
  const buttons = [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].filter((button) => button.textContent !== "Member details");
  expect(buttons.map((button) => button.textContent)).toEqual(["Pause", "Stop", "Swap…"]);
  buttons[1].click(); expect(onAction).toHaveBeenCalledWith("stop_member", "m1");
});

// Second confirmation: a member who is not active shows its state chip alone;
// no line under the name repeats it.
it.each(["paused", "not_created", "removed"])("a %s member shows its state chip alone", (state) => {
  const { host } = setup(member({ state }));
  const chip = host.querySelector(".tm-state-badge")!;
  expect(chip).not.toBeNull();
  expect(host.querySelector(".team-member-idle")).toBeNull();
  expect(host.textContent!.split(chip.textContent!).length - 1, "the state word appears once").toBe(1);
});

it("an active member without work on the page still says so", () => {
  const { host } = setup(member());
  expect(host.querySelector(".team-member-idle")?.textContent).toBe("No current assignment on this page.");
});
