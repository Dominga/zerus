import { createSignal } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import { machineCheck, machineResolve, machinesView } from "../bridge";
import { emptyMachines, type Batch, type ComputerId, type MachineMeta, type MachinesView } from "../types";
import { latest } from "./latest";

// Machine cards: each computer's latest catalog.meta projection
// (Batch.machines). The catalog applies them right after the computer
// statuses (the core's apply order); an ended computer drops its card.
// Also the Machines page's view from the screens hub.

export function createMachineCards() {
  const [cards, setCards] = createStore<Record<ComputerId, MachineMeta>>({});
  function applyMachines(b: Batch) {
    const ended = b.computers.filter((c) => c.status === "ended" && cards[c.computer]).map((c) => c.computer);
    if (ended.length) setCards(produce((all) => { for (const id of ended) delete all[id]; }));
    for (const m of b.machines ?? []) setCards(m.computer, reconcile(m.machine));
  }
  return { cards, applyMachines };
}

export const { cards: machineCards, applyMachines } = createMachineCards();

/** A machine another screen asks Machines to select (an alias, "" for this computer); Machines takes it once. */
export const [requestedMachine, setRequestedMachine] = createSignal<string | undefined>();
export const requestMachine = (alias: string) => setRequestedMachine(alias);

export const [machinesPage, setMachinesPage] = createSignal<MachinesView>(emptyMachines());
const pull = latest(setMachinesPage);
export const loadMachines = (force = false) => pull(() => machinesView(force));
export const resolveMachine = (alias: string) => pull(() => machineResolve(alias));
export const testConnection = (alias: string) => pull(() => machineCheck(alias));
