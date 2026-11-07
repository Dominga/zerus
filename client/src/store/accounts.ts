import { createSignal } from "solid-js";
import { accountsView, accountFiltersView } from "../bridge";
import { emptyAccounts, type AccountsView } from "../types";
import { latest } from "./latest";

// The Accounts screen and the Overview's account cards share one view from
// the screens hub. The machine filter and the profile to select are UI state.

export const [accountsPage, setAccountsPage] = createSignal<AccountsView>(emptyAccounts());
const pull = latest(setAccountsPage);
export const loadAccounts = (force = false) => pull(() => accountsView(force));
export const loadAccountFilters = (force = false) => pull(() => accountFiltersView(force));
/** Account marks read the named computers' catalogs only, then follow them until they settle (zerus-j648h). */
export async function loadMarkCatalogs(hosts: string[]) {
  await pull(() => accountFiltersView(true, hosts));
  for (let i = 0; i < 64 && accountsPage().catalog_loading; i++) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    await loadAccountFilters();
  }
}
/** The machine the Accounts list shows: a computer id, or "*" for all machines. */
export const [accountMachine, setAccountMachine] = createSignal("*");
/** The profile key an Overview card asks Accounts to select. */
export const [requestedProfile, setRequestedProfile] = createSignal<string | undefined>();
