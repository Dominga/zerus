// Task #9: one update of a 465-task team in the Plan view, applied as the
// store did before (replace) and does now (reconcile), on synthetic data only
// (no CLI, agent, installed app or owner data). Served by Vite outside the app
// entry; a headless browser reads the result from `window.teamUpdate` (also
// shown in the page and the console). Run:
//   npx vite --port <free port> --strictPort, then open
//   http://127.0.0.1:<port>/scripts/team-update-smoke.html[?mode=board&updates=30]
// in a browser (or headless Chrome/Edge with --dump-dom and read #result).
import { measureTeamUpdates, type Measured } from "./teamUpdate";
import type { PlanMode } from "../ui/teams/planModes";
import { applySkin, skins } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/teams.css";
import "../ui/styles/plan.css";

const params = new URLSearchParams(location.search);
const mode = (params.get("mode") ?? "graph") as PlanMode;
const updates = Number(params.get("updates") ?? 30);
applySkin(skins[0]);
const host = document.getElementById("root")!;
host.style.cssText = "width:1400px;height:900px;display:flex;flex-direction:column";
const results: Measured[] = [];
for (const scenario of ["member", "task"] as const) {
  for (const way of ["replace", "reconcile"] as const) {
    measureTeamUpdates(host, way, scenario, mode, 3);   // warm-up
    results.push(measureTeamUpdates(host, way, scenario, mode, updates));
  }
}
(window as unknown as { teamUpdate: Measured[] }).teamUpdate = results;
const pre = document.createElement("pre");
pre.id = "result";
pre.textContent = JSON.stringify(results, null, 1);
document.body.append(pre);
console.log("team-update-smoke", JSON.stringify(results));
