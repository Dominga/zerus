/** Shell presentation qualification with an isolated replay, no live shell. */
import { invoke } from "@tauri-apps/api/core";
import { catalog } from "../store/catalog";
import { setSelectedKey } from "../store/ui";
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
export async function shellBadgeSmoke(allowed: boolean) {
  if (!allowed) return;
  const log = (line: string) => invoke("perf_log", { line: "shell-badge-smoke: " + line });
  try {
    const deadline = performance.now() + 15_000;
    while (!Object.values(catalog.rows).some((s) => s.cmd === "sh")) { if (performance.now() > deadline) throw Error("synthetic shell row missing"); await sleep(20); }
    const row = Object.values(catalog.rows).find((s) => s.cmd === "sh")!; setSelectedKey(row.key); await sleep(300);
    const badges = [...document.querySelectorAll<HTMLElement>(".agent-badge[title=Shell]")];
    if (badges.length < 2 || badges.some((b) => b.textContent?.trim() !== "Shell")) throw Error("Shell badge missing in list or header");
    if (document.querySelector(".agent-badge[title=Claude]")) throw Error("shell draws a Claude badge");
    if (!document.querySelector(".agent-badge[title=Codex]")) throw Error("existing provider badge changed");
    if (badges.some((b) => b.querySelector("svg")?.innerHTML.includes("rotate("))) throw Error("shell draws the Claude glyph");
    const codex = document.querySelector<HTMLElement>(".agent-badge[title=Codex]")!;
    if (getComputedStyle(badges[0]).color !== getComputedStyle(codex).color) throw Error("shell tint differs from Qt's default provider tint");
    await log("SHELL"); await sleep(600); await log("PASS"); await invoke("exit_app", { code: 0 });
  } catch (error) { await log("FAIL " + String(error)); await invoke("exit_app", { code: 1 }); }
}
