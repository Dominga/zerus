import { ErrorBoundary, Suspense, type JSX } from "solid-js";
import { tr } from "../i18n";

/**
 * Where a lazyView renders (zerus-ycycu): `loading` shows while its module
 * loads, and a failed load shows `failed` with Retry, which loads it again.
 * `floating` places that notice over the window, for views that render as
 * dialogs and have no place of their own in the layout.
 */
export function LazyBoundary(props: { loading?: JSX.Element; failed?: string; floating?: boolean; children: JSX.Element }) {
  return (
    <ErrorBoundary fallback={(_error, retry) => (
      <div class="lazy-failed" classList={{ floating: !!props.floating }} role="alert">
        <span>{props.failed ?? tr("This part of the window did not load.")}</span>
        <button type="button" class="btn small" onClick={() => retry()}>{tr("Retry")}</button>
      </div>
    )}>
      <Suspense fallback={props.loading}>{props.children}</Suspense>
    </ErrorBoundary>
  );
}
