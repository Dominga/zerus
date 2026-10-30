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
/** The machine the Accounts list shows: a computer id, or "*" for all machines. */
export const [accountMachine, setAccountMachine] = createSignal("*");
/** The profile key an Overview card asks Accounts to select. */
export const [requestedProfile, setRequestedProfile] = createSignal<string | undefined>();
