import { acknowledgeExitFlush, onExitFlush } from "../bridge";

export interface ExitFlushPort {
  listen(request: (nonce: string) => Promise<void>): Promise<() => void>;
  ack(nonce: string): Promise<void>;
}

/** One quit per page; future composers register in the same bounded handshake. */
export function createExitFlush(port: ExitFlushPort) {
  const composers = new Set<() => Promise<void>>();
  let started: Promise<() => void> | undefined;
  let nonce: string | undefined;
  async function request(next: string) {
    if (nonce !== undefined) return;
    nonce = next;
    await Promise.allSettled([...composers].map((flush) => Promise.resolve().then(flush)));
    await port.ack(next).catch(() => {});
  }
  return {
    register(flush: () => Promise<void>) { composers.add(flush); return () => { composers.delete(flush); }; },
    start: () => started ??= port.listen(request),
  };
}

export const exitFlush = createExitFlush({ listen: onExitFlush, ack: (nonce) => acknowledgeExitFlush(nonce) });
