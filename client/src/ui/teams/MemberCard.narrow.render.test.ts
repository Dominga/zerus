// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { MemberCard } from "./MemberCard";
import fixture from "./fixtures/lanes-view.json";
import type { TeamView } from "../../store/teams";
const pages = fixture.pages as unknown as TeamView[];
const page = pages.find((v) => v.groups.some((g) => g.cards.some((c) => c.node === 1 && c.held)))!;
const cards = page.groups.flatMap((g) => g.cards);
const member = page.members.find((m) => m.id === cards[0].holder)!;
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
function mount(navigate = true, title?: string) {
  const host = document.createElement("div"); document.body.append(host);
  const select = vi.fn(), action = vi.fn();
  // Only the synthetic title stress test changes metadata; every other fact is product-written.
  const current = title ? cards.map((c) => ({ ...c, title })) : cards;
  dispose = render(() => createComponent(MemberCard, { member, teamStatus: page.bar.status, cards: current, busy: false, onAction: action, onSelect: navigate ? select : undefined }), host);
  return { host, select, action };
}
it("keeps the product-held assignment and task title separate and opens that exact task", () => {
  const { host, select, action } = mount();
  expect(host.querySelector(".team-member-assignment")?.textContent).toBe("Task #1 is held");
  expect(host.querySelector(".team-member-task-title")?.textContent).toBe(cards[0].title);
  host.querySelector<HTMLButtonElement>(".team-member-activity > button")!.click();
  expect(select).toHaveBeenCalledExactlyOnceWith(1); expect(action).not.toHaveBeenCalled();
});
it("keeps both product facts visible when task navigation is unavailable", () => {
  const { host } = mount(false);
  expect(host.querySelector(".team-member-activity > button")).toBeNull();
  expect(host.querySelector(".team-member-assignment")?.textContent).toBe("Task #1 is held");
  expect(host.querySelector(".team-member-task-title")?.textContent).toBe(cards[0].title);
});
it("preserves the full synthetic long title while giving it its own truncation line", () => {
  const title = "A deliberately long task title for the narrow member assignment layout control";
  const { host } = mount(true, title);
  expect(host.querySelector(".team-member-assignment")?.textContent).toBe("Task #1 is held");
  const label = host.querySelector(".team-member-task-title");
  expect(label?.textContent).toBe(title); expect(label?.getAttribute("title")).toBe(title);
});
