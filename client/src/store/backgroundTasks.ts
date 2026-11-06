// What the status bar lists as running work. This only reads the native cleanup
// store: it adds no authority, retry or cancel, and no timer ever ends an item.
import { tr, trNoop } from "../i18n";
import type { CleanupView } from "./terminationCleanup";

export interface BackgroundTask { id: string; name: string; path: string; state: string }

const ACTIVITY = new Map(Object.entries({
  checking: trNoop("Checking…"),
  removing: trNoop("Checking again and removing…"),
  stopping: trNoop("Stopping programs…"),
  restoring: trNoop("Restoring session…"),
}));
/** A native phase this build doesn't name still reads as running work. */
export const activityLabel = (activity?: string | null) => tr(ACTIVITY.get(activity ?? "") ?? trNoop("Working…"));

/** A row is running exactly while native marks it busy. */
export const backgroundTasks = (view: CleanupView): BackgroundTask[] => view.rows.filter((row) => row.busy)
  .map((row) => ({ id: row.id, name: row.label, path: row.path, state: activityLabel(row.activity) }));

/**
 * The cleanup chip steps aside only while every row is still running and no
 * result is held, including results past the row cap, so running work never
 * hides a kept or unknown result.
 */
export const showsCleanupChip = (view: CleanupView) => view.kept > 0 || view.unknown > 0 || view.rows.some((row) => !row.busy);
