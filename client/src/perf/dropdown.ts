/** Exercise the actual themed dropdown, retaining callers' stable trigger selectors. */
export function chooseDropdown(trigger: HTMLButtonElement, value: string): void {
  if (trigger.disabled) throw Error("Dropdown is disabled");
  if (trigger.getAttribute("aria-expanded") !== "true") trigger.click();
  const list = document.getElementById(trigger.getAttribute("aria-controls") ?? "");
  const option = [...(list?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [])].find((row) => row.dataset.value === value);
  if (!option || option.getAttribute("aria-disabled") === "true") throw Error("Dropdown option is unavailable");
  option.click();
}
