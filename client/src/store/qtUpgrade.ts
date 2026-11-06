import { createSignal } from "solid-js";
import { acknowledgeQtUpgradeNotice, qtUpgradeNotice } from "../bridge";
import { tr } from "../i18n";
import { bus, type NoticeBus } from "./notices";

interface Backend { pending(): Promise<boolean>; acknowledge(): Promise<void> }
/** One plain explanation. Acknowledgment changes only local notice metadata. */
export function createQtUpgradeNotice(backend: Backend, notices: NoticeBus) {
  let checked = false;
  return async () => {
    if (checked) return;
    checked = true;
    try {
      if (!await backend.pending()) return;
      const [saving, setSaving] = createSignal(false);
      const [error, setError] = createSignal("");
      const id = notices.notify("info", tr("Projects and accounts remain available. Other Qt data is not carried over: drafts, unsent organization edits, attention and notification history, pending operations and task deliveries, repository clone settings, and caches. Configure what you need again. Original files are kept for manual recovery. Existing Next data is kept; old Qt operations and notifications will not run again."), true, "qt-upgrade-048", tr("Upgrading from Qt"), {
        dismissible: false,
        note: error,
        actions: [{ label: () => tr("Got it"), style: "primary", disabled: saving, run: async () => {
          if (saving()) return;
          setSaving(true);
          setError("");
          try { await backend.acknowledge(); notices.dismiss(id); }
          catch { setError(tr("The upgrade notice could not be saved. Try again.")); }
          finally { setSaving(false); }
        } }],
      });
    } catch { checked = false; }
  };
}
export const showQtUpgradeNotice = createQtUpgradeNotice({ pending: qtUpgradeNotice, acknowledge: acknowledgeQtUpgradeNotice }, bus);
