// Real Accounts screen with synthetic identities and no management IPC.
// Run client/scripts/account-identity-geometry-smoke.mjs for the isolated Firefox evidence.
import { render } from "solid-js/web";
import { accountsPage, setAccountsPage, setRequestedProfile } from "../store/accounts";
import { emptyAccounts, emptyUsage, type Account, type Identity, type Profile } from "../types";
import type { Text } from "../text";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/screens.css";
import "../ui/styles/sessions.css";
import "../ui/styles/machines.css";

const failures: string[] = [], calls: string[] = [];
const startupCalls = new Set(["pending_read_marks", "attention_marks", "cancel_content_search", "composer_visibility", "settings_get"]);
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const settle = async () => { await frame(); await frame(); };
const EMPTY_IDENTITY: Identity = { email: "", name: "", plan: "", organization: "", account_id: "", auth_method: "" };
const KIMI_IDENTITY: Identity = {
  ...EMPTY_IDENTITY,
  name: "Ада 例 Лоцянская с очень длинным именем",
  email: "ada.lovelace+оченьдлинныйадрес@example.test",
  organization: "Организация 例 для исследования длинных строк интерфейса",
  plan: "Команда плюс",
  auth_method: "OAuth",
};
const CLAUDE_LATE_IDENTITY: Identity = {
  ...EMPTY_IDENTITY,
  name: "克劳德 Example",
  email: "claude-late-metadata-with-a-long-local-value@example.test",
  organization: "Institut für angewandte Forschung 例",
  plan: "Pro",
  auth_method: "passkey",
};
const TITLE_LABELS = {
  short: "Kimi Personal",
  "email-label": "a.person.with.a.long.account.label@example.test",
  "long-label": "A long research account title used for independent interface checks",
  "localized-label": "Очень длинное название учётной записи 例 для проверки стабильности карточек",
};

function identityLines(label: string, identity: Identity): Text[] {
  const seen = new Set([label.trim().toLowerCase()]);
  const values = [identity.name, identity.email, identity.organization].filter((value) => {
    const text = value.trim(), key = text.toLowerCase();
    if (!text || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  return [
    ...values.map((value): Text => ({ source: "%1", args: [{ kind: "verbatim", value: value.trim() }] })),
    ...(identity.plan ? [{ source: "Plan: %1", args: [{ kind: "verbatim" as const, value: identity.plan }] }] : []),
    ...(identity.auth_method && identity.auth_method !== "none" ? [{ source: "Sign-in: %1", args: [{ kind: "verbatim" as const, value: identity.auth_method }] }] : []),
  ];
}

function account(key: string, label: string, provider: string, identity: Identity, status = "ok", note: string | null = null): Account {
  const usage = { ...emptyUsage(provider), status, identity };
  const profile: Profile = {
    key: `workstation\u001f${key}`, host: "workstation", machine: "workstation", id: key, label, provider, native: false,
    installed: true, is_default: true, home: "/fixture/home", auth_revision: "", online: true, state: "ready",
    detail: { source: "Ready on workstation." }, signed_in: true, permission: "auto", permission_mode: "auto",
    permission_label: { source: "Auto" }, permission_hint: { source: "Synthetic account permission" },
    permission_default: true, permission_scope: "account", model_supported: false, model_text: { source: "" }, usage, opencode: null,
  };
  return {
    key, label, provider, installed: true, machines: ["workstation"], members: [profile], usage,
    permission: { source: "Auto" }, identity_lines: identityLines(label, identity), identity_note: note ? { source: note } : null,
  };
}

const kimiLoading = () => account("kimi", "Kimi Personal", "kimi", EMPTY_IDENTITY, "loading", "Reading account details…");
const kimiLoaded = () => account("kimi", "Kimi Personal", "kimi", KIMI_IDENTITY);
const codex = () => account("codex", "Codex Work", "codex", { ...EMPTY_IDENTITY, plan: "Team", auth_method: "ChatGPT" });
const claude = () => account("claude", "Claude Team", "claude", { ...EMPTY_IDENTITY, email: "claude@example.test" });
const claudeUnavailable = () => account("claude", "Claude Team", "claude", EMPTY_IDENTITY, "unavailable", "Account identity is unavailable.");
const claudeLate = () => account("claude", "Claude Team", "claude", CLAUDE_LATE_IDENTITY);
const initialAccounts = () => [kimiLoading(), codex(), claude()];
const initialView = () => ({
  ...emptyAccounts(), snapshot: true, loaded_machines: 1, accounts: initialAccounts(),
  hosts: [{ computer: "workstation", name: "workstation", label: "Workstation", online: true }],
});
const updates = { loading: kimiLoading, kimi: kimiLoaded, unavailable: claudeUnavailable, "claude-late": claudeLate,
  "codex-email": () => account("codex", "codex@example.test", "codex", { ...EMPTY_IDENTITY, email: "codex@example.test", plan: "Team", auth_method: "ChatGPT" }),
};
type Update = keyof typeof updates;

Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string) => {
  calls.push(command);
  if (command === "pending_read_marks") return [];
  if (command === "attention_marks") return { marks: {}, replies: {} };
  if (command === "cancel_content_search") return null;
  if (command === "composer_visibility") return null;
  if (command === "settings_get") return { values: {}, revision: 0 };
  throw Error(`Unexpected synthetic IPC: ${command}`);
} } });

const rounded = (value: number) => Number(value.toFixed(3));
const box = (element: Element) => {
  const rect = element.getBoundingClientRect();
  return { top: rounded(rect.top), bottom: rounded(rect.bottom), left: rounded(rect.left), right: rounded(rect.right), width: rounded(rect.width), height: rounded(rect.height) };
};

function geometry() {
  const detail = document.querySelector<HTMLElement>(".acct-detail")!;
  const title = detail.querySelector<HTMLElement>(".acct-title")!;
  const titleMain = title.querySelector<HTMLElement>(".acct-title-main")!;
  const actionArea = title.querySelector<HTMLElement>(".acct-title-actions")!;
  const heading = titleMain.querySelector<HTMLElement>("h2")!;
  const hidden = title.querySelector<HTMLElement>(".am-hidden");
  const provider = titleMain.querySelector<HTMLElement>(".agent-badge")!;
  const identity = detail.querySelector<HTMLElement>(".acct-identity")!;
  const main = document.querySelector<HTMLElement>(".acct-main-scroll")!;
  const metadata = [...detail.querySelectorAll<HTMLElement>("[data-meta]")].map((element) => ({
    kind: element.dataset.meta, text: element.textContent, title: element.title, aria: element.getAttribute("aria-label"),
    ...box(element), clientWidth: element.clientWidth, scrollWidth: element.scrollWidth,
  }));
  check(getComputedStyle(identity).userSelect === "text", "metadata group remains selectable");
  check(heading.textContent === heading.title && heading.getAttribute("aria-label") === heading.title, "full account label remains in DOM, tooltip and accessibility");
  check(getComputedStyle(heading).whiteSpace === "nowrap" && getComputedStyle(titleMain).flexWrap === "nowrap", "account label and title row never wrap");
  if (hidden) {
    const shown = hidden.classList.contains("shown");
    check(hidden.textContent === hidden.title && hidden.getAttribute("aria-label") === hidden.title, "hidden status keeps full tooltip and accessible text");
    check(getComputedStyle(hidden).visibility === (shown ? "visible" : "hidden") && hidden.getAttribute("aria-hidden") === String(!shown), "hidden status is exposed only when true");
  }
  for (const field of detail.querySelectorAll<HTMLElement>(".acct-meta-field")) {
    const label = field.querySelector<HTMLElement>(".acct-meta-label")!;
    const value = field.querySelector<HTMLElement>(".acct-meta-value")!;
    check(label.getBoundingClientRect().height <= 20.02 && getComputedStyle(label).whiteSpace === "nowrap", "metadata labels never wrap into another row");
    check(value.getBoundingClientRect().left >= label.getBoundingClientRect().right, "metadata label and value do not overlap");
  }
  for (const value of metadata) {
    if (value.text !== "—") {
      check(value.title === value.text, `${value.kind}: full-value tooltip`);
      check(value.aria?.endsWith(value.text ?? ""), `${value.kind}: full accessible value`);
    }
  }
  const actions = [...actionArea.querySelectorAll<HTMLElement>(".btn")].map((element) => ({ text: element.textContent, ...box(element) }));
  for (const action of actions) check(action.left >= 0 && action.right <= window.innerWidth, `${action.text}: action stays in the viewport`);
  check(main.scrollWidth <= main.clientWidth + 1, "detail has no horizontal overflow");
  return {
    selected: heading.textContent,
    language: document.documentElement.lang,
    viewport: { width: window.innerWidth, height: window.innerHeight },
    detail: box(detail), title: box(title), titleMain: box(titleMain), actionArea: box(actionArea), identity: box(identity),
    accountLabel: { text: heading.textContent, title: heading.title, aria: heading.getAttribute("aria-label"), ...box(heading), clientWidth: heading.clientWidth, scrollWidth: heading.scrollWidth },
    hidden: hidden ? { shown: hidden.classList.contains("shown"), visibility: getComputedStyle(hidden).visibility, ariaHidden: hidden.getAttribute("aria-hidden"), text: hidden.textContent, title: hidden.title, aria: hidden.getAttribute("aria-label"), ...box(hidden) } : null,
    provider: { text: provider.textContent, title: provider.title, ...box(provider) }, mark: box(titleMain.querySelector(".acct-mark")!),
    titleDisplay: getComputedStyle(title).display, titleColumns: getComputedStyle(title).gridTemplateColumns,
    machines: box(detail.querySelector(".acct-card")!), usage: box(detail.querySelector(".acct-usage")!),
    actions, metadata, mainClientWidth: main.clientWidth, mainScrollWidth: main.scrollWidth,
    accountCount: document.querySelectorAll(".acct-row").length,
    machineCount: document.querySelectorAll(".acct-mrow-pick").length,
    ipcExpected: calls.every((command) => startupCalls.has(command)), ipcCalls: [...calls], fixtureFailures: [...failures],
  };
}

async function select(label: string) {
  const row = [...document.querySelectorAll<HTMLButtonElement>(".acct-row")].find((candidate) => candidate.querySelector("b")?.textContent === label);
  check(row, `${label}: account row exists`);
  row!.focus(); row!.click(); await settle();
  check(document.activeElement === row, `${label}: account selection retains row focus`);
  return geometry();
}

async function update(kind: Update) {
  const replacement = updates[kind]();
  const before = document.querySelector(".acct-detail");
  setAccountsPage({ ...accountsPage(), accounts: accountsPage().accounts.map((item) => item.key === replacement.key ? replacement : item) });
  await settle();
  check(document.querySelector(".acct-detail") === before, `${kind}: same-key update preserves the Detail node`);
  return geometry();
}

function selectedFixtureAccount() {
  const label = document.querySelector(".acct-title h2")?.textContent;
  const selected = accountsPage().accounts.find((item) => item.label === label);
  if (!selected) throw Error(`Selected account is missing: ${label}`);
  return selected;
}

async function setLabel(label: string) {
  const current = selectedFixtureAccount();
  const before = document.querySelector(".acct-detail"), focus = document.activeElement;
  setAccountsPage({ ...accountsPage(), accounts: accountsPage().accounts.map((item) => item.key === current.key ? {
    ...item, label, members: item.members.map((member) => ({ ...member, label })), identity_lines: identityLines(label, item.usage.identity),
  } : item) });
  await settle();
  check(document.querySelector(".acct-detail") === before && document.activeElement === focus, `${label}: late label preserves Detail and focus`);
  return geometry();
}

async function setHidden(hidden: boolean) {
  const [{ ACCOUNT_HIDE, profileToken }, { setSetting }] = await Promise.all([import("../store/accountMarks"), import("../store/settings")]);
  const current = selectedFixtureAccount();
  const before = document.querySelector(".acct-detail"), focus = document.activeElement;
  await Promise.all(current.members.map((member) => setSetting(ACCOUNT_HIDE + profileToken(member.host, member.id), hidden, { persist: false })));
  await settle();
  check(!!document.querySelector(".am-hidden.shown") === hidden, `${hidden}: hidden setting takes effect`);
  check(document.querySelector(".acct-detail") === before && document.activeElement === focus, `${hidden}: hidden setting preserves Detail and focus`);
  return geometry();
}

async function clearHidden() {
  const [{ ACCOUNT_HIDE, profileToken }, { setSetting }] = await Promise.all([import("../store/accountMarks"), import("../store/settings")]);
  for (const item of initialAccounts()) for (const member of item.members) {
    await setSetting(ACCOUNT_HIDE + profileToken(member.host, member.id), false, { persist: false });
  }
}

async function machineKeyboard() {
  const radio = document.querySelector<HTMLButtonElement>(".acct-mrow-pick")!;
  radio.focus();
  radio.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
  await settle();
  check(radio.getAttribute("aria-checked") === "true" && document.activeElement === radio, "machine keyboard navigation retains selection and focus");
  return geometry();
}

async function run() {
  const { settingsReady, setSetting } = await import("../store/settings");
  await settingsReady;
  if (new URLSearchParams(location.search).get("lang") === "ru") {
    await setSetting("workspace/language", "ru", { persist: false });
    const { installUiLanguage, uiLanguage } = await import("../i18n");
    installUiLanguage();
    for (let i = 0; i < 50 && uiLanguage() !== "ru"; i++) await frame();
    check(uiLanguage() === "ru", "Russian catalog loaded");
    document.documentElement.lang = "ru";
  }
  const { applySkin, skins } = await import("../skin/runtime");
  applySkin(skins[0]);
  const { ACCOUNT_HIDE, profileToken } = await import("../store/accountMarks");
  setAccountsPage(initialView());
  const { AccountsScreen } = await import("../ui/screens/Accounts");
  render(() => <AccountsScreen />, document.getElementById("root")!);
  await settle();
  const api = {
    geometry,
    select,
    update,
    setLabel,
    setHidden,
    machineKeyboard,
    echoMetadata: async () => {
      const current = selectedFixtureAccount(), before = document.querySelector(".acct-meta-field");
      setAccountsPage({ ...accountsPage(), accounts: accountsPage().accounts.map((item) => item.key === current.key ? {
        ...item, usage: { ...item.usage, identity: { ...item.usage.identity } },
      } : item) });
      await settle(); check(document.querySelector(".acct-meta-field") === before, "unchanged metadata refresh preserves field nodes"); return geometry();
    },
    requestProfile: async (provider: string) => {
      const account = accountsPage().accounts.find((item) => item.provider === provider)!;
      setRequestedProfile(account.members[0].key); await settle(); return geometry();
    },
    reset: async () => { await clearHidden(); setAccountsPage(initialView()); await settle(); return select(TITLE_LABELS.short); },
    inputs: () => ({
      initial: initialView(), updates: { kimi: kimiLoaded(), unavailable: claudeUnavailable(), "claude-late": claudeLate() },
      titleLabels: TITLE_LABELS, hiddenKeys: initialAccounts().flatMap((item) => item.members.map((member) => ACCOUNT_HIDE + profileToken(member.host, member.id))), hiddenValues: [true, false],
    }),
    failures: () => failures,
    calls: () => calls,
  };
  Object.assign(window, { accountIdentityFixture: api });
  document.documentElement.dataset.fixtureReady = "true";
}

void run().catch((error) => {
  failures.push(String(error));
  Object.assign(window, { accountIdentityFixture: { failures: () => failures, calls: () => calls } });
  document.documentElement.dataset.fixtureReady = "error";
});
