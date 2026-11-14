import { tr, trNoop } from "../../i18n";

export const NO_COLOR_SEND = trNoop("This agent was started with NO_COLOR, so Zerus cannot tell a queued message from a draft. Restart the session to fix sending.");
export const REFUSED_SEND = trNoop("Message was not sent. hgs refused before typing: %1");
const PREFIX = REFUSED_SEND.replace("%1", "");
const NATIVE_REFUSALS = [
  trNoop("Codex is waiting for your sandbox setup choice. Choose an option in Activity or Terminal before sending a message."),
  NO_COLOR_SEND,
  trNoop("terminal has a draft, dialog, or unrecognized prompt; open Terminal and return to an empty agent prompt"),
  trNoop("agent busy: terminal queue prompt is unrecognized or contains a draft; wait for the agent or open Terminal; nothing was submitted"),
  trNoop("Claude is waiting for tool or safety approval. Review the request in Activity or Terminal before sending a message."),
  trNoop("Claude is waiting for a conversation resume choice. Choose an option in Activity or Terminal before sending a message."),
  trNoop("Claude is waiting for a usage-limit choice. Choose an option in Activity or Terminal before sending a message."),
  trNoop("Claude is waiting for folder trust approval. Choose an option in Activity or Terminal before sending a message."),
  trNoop("Codex is waiting for folder trust approval. Choose an option in Activity or Terminal before sending a message."),
  trNoop("Codex is waiting for hook trust review. Choose an option in Activity or Terminal before sending a message."),
  trNoop("Codex is waiting for an update choice. Choose an option in Activity or Terminal before sending a message."),
  trNoop("Codex is waiting for your rate-limit model choice. Choose an option in Activity or open Terminal before sending a message."),
  trNoop("Codex is waiting for your paused goal choice. Choose an option in Activity or Terminal before sending a message."),
  trNoop("Kimi is waiting for folder trust approval. Choose an option in Activity or Terminal before sending a message."),
];

/** Translate the known refusal inside the direct or deferred CLI diagnostic. */
export function sendErrorText(text: string): string {
  if (text.startsWith(PREFIX)) {
    const reason = text.slice(PREFIX.length);
    const known = NATIVE_REFUSALS.find((source) => reason === source);
    return tr(REFUSED_SEND, known ? tr(known) : reason);
  }
  return text.replace(NO_COLOR_SEND, tr(NO_COLOR_SEND));
}
