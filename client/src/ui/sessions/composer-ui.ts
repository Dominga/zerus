import type { AttachmentView } from "../../types";

// Pure pieces of the composer view (Composer.tsx, Attachments.tsx) that tests
// reach without a DOM.

/**
 * A keyed attachment row's item: the attachment with its reference in the
 * current list, else the last one the row showed. A row whose reference went
 * may still read once before Solid disposes it; it then shows its own last
 * item, never another row's and never nothing.
 */
export function keptItem(items: () => readonly AttachmentView[], reference: string): () => AttachmentView {
  let last: AttachmentView = items().find((a) => a.reference === reference)
    ?? { name: "", mime: "", reference, bytes: 0, path: "", excerpt: null };
  return () => {
    const found = items().find((a) => a.reference === reference);
    if (found) last = found;
    return last;
  };
}

/** What `moveFocusOff` focuses as a fallback: the message field. */
export interface FocusTarget { focus(): void; disabled?: boolean; isConnected?: boolean }

/**
 * Before a focused toolbar chip hides (Mark as read, Latest): the transcript
 * takes the focus, as Qt's browser does (SessionsWindow.cpp:1038,
 * ActivityView.cpp:1611). Without a transcript to focus, the message field
 * takes it while it is shown and enabled: the toolbar's focus fallback
 * (MessageComposer.cpp:280; ComposerToolbar.cpp:225-227, 266-267).
 */
export function moveFocusOff(transcript: () => boolean, field: FocusTarget | undefined): void {
  if (transcript()) return;
  if (field && field.isConnected !== false && !field.disabled) field.focus();
}
