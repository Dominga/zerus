import { beforeEach, expect, it, vi } from "vitest";
import catalog from "../../i18n.ru.json";
const language = vi.hoisted(() => ({ value: "ru" as "en" | "ru" }));
vi.mock("../../i18n", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../i18n")>();
  return { ...actual, tr: (text: string, ...args: (string | number)[]) =>
    actual.translateWithCatalog(catalog, language.value, text, ...args) };
});
import { NO_COLOR_SEND, REFUSED_SEND, sendErrorText } from "./send-error";
beforeEach(() => { language.value = "ru"; });

it("localizes the verified refusal inside a failed row while retaining its receipt diagnostic", () => {
  const caution = "Delivery not confirmed. Check Terminal before retrying.";
  expect(sendErrorText(`${caution}\nhgs send (1): hgs: ${NO_COLOR_SEND}`))
    .toBe(`${caution}\nhgs send (1): hgs: ${catalog[NO_COLOR_SEND as keyof typeof catalog]}`);
  expect(sendErrorText("hgs: generic draft refusal")).toBe("hgs: generic draft refusal");
});

it("shows the durable pre-input refusal in Russian instead of a missing-delivery warning", () => {
  expect(sendErrorText(`Message was not sent. hgs refused before typing: ${NO_COLOR_SEND}`))
    .toBe(`Сообщение не отправлено. hgs отказал до ввода текста: ${catalog[NO_COLOR_SEND as keyof typeof catalog]}`);
});

it("translates the retained draft, dialog and unrecognized-queue reasons with the actual catalog", () => {
  for (const reason of [
    "Codex is waiting for your sandbox setup choice. Choose an option in Activity or Terminal before sending a message.",
    "terminal has a draft, dialog, or unrecognized prompt; open Terminal and return to an empty agent prompt",
    "agent busy: terminal queue prompt is unrecognized or contains a draft; wait for the agent or open Terminal; nothing was submitted",
    "Claude is waiting for tool or safety approval. Review the request in Activity or Terminal before sending a message.",
  ]) {
    const translated = catalog[reason as keyof typeof catalog];
    expect(translated).toBeTruthy();
    expect(sendErrorText(REFUSED_SEND.replace("%1", reason)))
      .toBe(`Сообщение не отправлено. hgs отказал до ввода текста: ${translated}`);
  }
});

it("keeps English in English mode and never translates an unknown native diagnostic as source text", () => {
  const message = REFUSED_SEND.replace("%1", NO_COLOR_SEND);
  language.value = "en";
  expect(sendErrorText(message)).toBe(message);
  language.value = "ru";
  expect(sendErrorText(REFUSED_SEND.replace("%1", "native I/O unavailable")))
    .toBe("Сообщение не отправлено. hgs отказал до ввода текста: native I/O unavailable");
});
