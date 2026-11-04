/** Read-only following for a dialog reattached to an already active clone step.
 * One read at a time; closing or starting an explicit step ignores late replies. */
export function watchBusyClone<T extends { busy: boolean }>(read: () => Promise<T>, update: (view: T) => void): () => void {
  let active = true, pending = false;
  const stop = () => { active = false; clearInterval(timer); };
  const timer = setInterval(() => {
    if (!active || pending) return;
    pending = true;
    void read().then((view) => {
      if (!active) return;
      update(view);
      if (!view.busy) stop();
    }).catch(() => {}).finally(() => { pending = false; });
  }, 150);
  return stop;
}
