// Settings > Worktrees as WorktreeSettings.h drives it. The computer, the
// request in flight and the policy read live in the app (worktree_policy):
// they outlast the section, and the app runs one request at a time. The
// page keeps only the switches as edited, taken again from each answer.

export interface Rules { enabled: boolean; after_session: boolean; unused_integrated: boolean; missing: boolean; grace_hours: number }
export interface View {
  peers: string[];
  route: string;
  busy: boolean;
  /** The rules read; null when nothing can be applied. */
  rules: Rules | null;
  /** Counts the answers that set `rules`. */
  loaded: number;
  status: string;
  failed: boolean;
  /** Grows with every change in the app. */
  version: number;
}

export interface WorktreesIo {
  view(): Promise<View>;
  read(): Promise<View>;
  choose(route: string): Promise<View>;
  apply(rules: Rules): Promise<View>;
}

export interface WorktreesState {
  view?: View;
  /** The switches as shown and edited. */
  rules: Rules;
  /** A call to the app has not answered yet. */
  sending: boolean;
  /** A call that failed outright. */
  error?: string;
}

/** WorktreeSettings::controls(): which controls can be used. */
export function controls(view: View | undefined, rules: Rules, sending = false) {
  const idle = !!view && !view.busy && !sending;
  const ready = idle && !!view?.rules;
  return {
    computer: idle,
    refresh: idle,
    apply: ready,
    enabled: ready,
    others: ready && rules.enabled,
    grace: ready && rules.enabled && rules.unused_integrated,
  };
}

export const DEFAULT_RULES: Rules = { enabled: false, after_session: false, unused_integrated: false, missing: false, grace_hours: 24 };

export function createWorktrees(io: WorktreesIo, show: (state: WorktreesState) => void) {
  let state: WorktreesState = { rules: DEFAULT_RULES, sending: false };
  // The answer whose rules the switches took; none yet.
  let loaded = -1;
  const update = (change: Partial<WorktreesState>) => { state = { ...state, ...change }; show(state); };
  const take = (view: View) => {
    // A reply can arrive after a newer view (an answer's event was quicker).
    if (state.view && view.version < state.view.version) { update({ sending: false }); return; }
    const fresh = view.rules && view.loaded !== loaded;
    if (fresh) loaded = view.loaded;
    update({ view, sending: false, error: undefined, ...(fresh ? { rules: view.rules! } : {}) });
  };
  const fail = (error: unknown) => update({ sending: false, error: String(error) });
  const send = (call: () => Promise<View>) => {
    update({ sending: true });
    call().then(take, fail);
  };

  return {
    state: () => state,
    /** Opening the section: reads again unless a request still runs, whose outcome then shows. */
    open() { send(() => io.read()); },
    /** The app says an answer came. */
    changed() { io.view().then(take, fail); },
    refresh() { if (controls(state.view, state.rules, state.sending).refresh) send(() => io.read()); },
    choose(route: string) {
      if (!controls(state.view, state.rules, state.sending).computer || route === state.view?.route) return;
      send(() => io.choose(route));
    },
    edit(change: Partial<Rules>) { update({ rules: { ...state.rules, ...change } }); },
    apply() {
      if (!controls(state.view, state.rules, state.sending).apply) return;
      const rules = state.rules;
      send(() => io.apply(rules));
    },
  };
}
