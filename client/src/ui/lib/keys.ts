/**
 * A key an input method takes while it composes, such as the Enter that
 * commits Chinese or Japanese input. WebKit reports that Enter with keyCode
 * 229 once `isComposing` is already false. In Qt the input method consumes
 * it, so no dialog acts on it (the shortcut router skips it the same way).
 */
export const composing = (e: Partial<Pick<KeyboardEvent, "isComposing" | "keyCode" | "key">>) =>
  e.isComposing === true || e.keyCode === 229 || e.key === "Process";
