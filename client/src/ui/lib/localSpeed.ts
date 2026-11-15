import { createEffect, onCleanup } from "solid-js";
import type { ModelServer, ModelServerStatus } from "../../bridge";
import { tr } from "../../i18n";
import { modelServers } from "../../store/modelServers";

// Owner, 2026-11-15: an account that runs a local model (GB10) has no limits
// or balance worth showing; its place shows the model's current speed in
// tokens per second, as the account's model server reports it.

/** What ties an account to a model server: the gateway its agent talks to, else a server of the same name. */
export interface LocalAccount { id: string; gateway?: string }
export interface LocalSpeed { server: string; model: string; tok_s: number; kind: string }

const root = (url: string) => url.trim().toLowerCase().replace(/\/+$/, "").replace(/\/v1$/, "");

/** The model server of the first account that runs a local model, by gateway URL and then by name. */
export function localServer(servers: readonly ModelServer[], accounts: readonly (LocalAccount | undefined)[]): string | undefined {
  for (const account of accounts) {
    const gateway = account?.gateway ? root(account.gateway) : "";
    const byGateway = gateway ? servers.find((s) => root(s.base_url) === gateway) : undefined;
    if (byGateway) return byGateway.name;
  }
  for (const account of accounts) {
    const byName = account && servers.find((s) => s.name === account.id);
    if (byName) return byName.name;
  }
  return undefined;
}

const RANK: Record<string, number> = { live: 3, last: 2, mean: 1 };

/** The speed to show: a live reading first, then the latest answer, then an average; the fastest model of a kind. */
export function speedIn(server: string, status: ModelServerStatus | undefined): LocalSpeed | undefined {
  let best: (LocalSpeed & { rank: number }) | undefined;
  for (const model of status?.models ?? []) {
    const tok_s = model.tok_s ?? 0;
    if (!(tok_s > 0)) continue;
    const kind = model.tok_s_kind ?? "";
    const rank = RANK[kind] ?? 0;
    if (!best || rank > best.rank || (rank === best.rank && tok_s > best.tok_s)) best = { server, model: model.name, tok_s, kind, rank };
  }
  return best && { server: best.server, model: best.model, tok_s: best.tok_s, kind: best.kind };
}

const amount = (tok_s: number) => (tok_s >= 100 ? String(Math.round(tok_s)) : tok_s.toFixed(1));

/** "56.6 tok/s"; "local model" while the server reports no speed. */
export const speedText = (speed: LocalSpeed | undefined) => (speed ? tr("%1 tok/s", amount(speed.tok_s)) : tr("local model"));

/** The tooltip and accessible name: which model, on which server, and what the figure measures. */
export function speedTitle(server: string, speed: LocalSpeed | undefined): string {
  if (!speed) return tr("A local model on %1: it reports no speed yet.", server);
  const figure = amount(speed.tok_s);
  switch (speed.kind) {
    case "live": return tr("%1 on %2 generates %3 tokens per second now.", speed.model, speed.server, figure);
    case "last": return tr("%1 on %2 generated its latest answer at %3 tokens per second.", speed.model, speed.server, figure);
    case "mean": return tr("%1 on %2 generates %3 tokens per second on average.", speed.model, speed.server, figure);
    default: return tr("%1 on %2: %3 tokens per second.", speed.model, speed.server, figure);
  }
}

/**
 * The model server and speed of a local model account, while the caller
 * shows it: the server is read every SPEED_INTERVAL until the caller's owner
 * is disposed. `server()` is undefined for a cloud account.
 */
export function useLocalSpeed(accounts: () => readonly (LocalAccount | undefined)[]) {
  const server = () => localServer(modelServers.servers(), accounts());
  createEffect(() => {
    const name = server();
    if (!name) return;
    onCleanup(modelServers.watchSpeed(name));
  });
  const speed = () => { const name = server(); return name ? speedIn(name, modelServers.entry(name).status) : undefined; };
  return { server, speed, text: () => speedText(speed()), title: () => { const name = server(); return name ? speedTitle(name, speed()) : ""; } };
}
