import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createServer, type ViteDevServer } from "vite";
import solid from "vite-plugin-solid";

// Task #8 (team ede724c4): New team's plan mode (both modes, the lead's offer
// and its reason, the marked state) and the page's how-to, rendered as the
// Tauri client draws them.
let server: ViteDevServer | undefined;
let planMode: (props: Record<string, unknown>) => string;
let howTo: (props: Record<string, unknown>) => string;
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };

beforeAll(async () => {
  const { fileURLToPath } = await import(/* @vite-ignore */ ["node", "url"].join(":")) as { fileURLToPath(url: URL): string };
  server = await createServer({
    configFile: false, root: fileURLToPath(new URL("../../../", import.meta.url)),
    server: { middlewareMode: true, watch: null }, logLevel: "silent",
    plugins: [solid({ ssr: true })],
  });
  const { renderToString } = await server.ssrLoadModule("solid-js/web");
  const { createComponent } = await server.ssrLoadModule("solid-js");
  const { PlanModeField, NewTeamHowTo } = await server.ssrLoadModule("/src/ui/teams/NewTeamGuide.tsx");
  planMode = (props) => renderToString(() => createComponent(PlanModeField, { onChoose: () => {}, ...props }));
  howTo = (props) => renderToString(() => createComponent(NewTeamHowTo, { onToggle: () => {}, ...props }));
});

afterAll(async () => { await server?.close(); });

const checked = (html: string, mode: string) => new RegExp(`<input[^>]*value="${mode}"[^>]*checked`).test(html);

describe("New team plan mode (task #8)", () => {
  it("shows standard chosen, both modes with their sentence, and no offer", () => {
    const html = planMode({ mode: "standard" });
    expect(html).toContain('data-plan-mode="standard"');
    expect(html).toContain("Standard planning");
    expect(html).toContain("You approve the phases one after another; the team ends with a release.");
    expect(html).toContain("Free planning");
    expect(html).toContain("Tasks come as a stream and are decided one by one; you close the phase when it is done.");
    expect(checked(html, "standard")).toBe(true);
    expect(checked(html, "free")).toBe(false);
    expect(html).not.toContain("data-plan-offer");
    expect(html).not.toContain("data-unseen-mark");
  });

  it("shows free chosen", () => {
    const html = planMode({ mode: "free" });
    expect(html).toContain('data-plan-mode="free"');
    expect(checked(html, "free")).toBe(true);
    expect(checked(html, "standard")).toBe(false);
  });

  it("shows the lead's offer with its reason and a way to take it", () => {
    const html = planMode({ mode: "standard", offer: "free", reason: "a stream of small requests" });
    expect(html).toContain('data-plan-offer="free"');
    expect(html).toContain("The lead offers Free planning: a stream of small requests");
    expect(html).toContain("Use the lead's offer");
  });

  it("shows an offer the owner already follows without the button, and one without a reason", () => {
    const html = planMode({ mode: "free", offer: "free" });
    expect(html).toContain("The lead offers Free planning");
    expect(html).not.toContain("data-use-offer");
  });

  it("marks the field the lead changed until it is seen", () => {
    const html = planMode({ mode: "standard", offer: "free", unseen: true });
    expect(html).toContain('data-unseen="true"');
    expect(html).toContain("nt-changed");
    expect(html).toContain('data-unseen-mark="plan_mode"');
    expect(html).toContain("Changed by the lead, not yet seen");
  });
});

describe("New team how-to (task #8)", () => {
  it("lists every step when open", () => {
    const html = howTo({ open: true });
    expect(html).toContain('data-howto="open"');
    expect(html).toContain('aria-expanded="true"');
    for (const step of ["Fill in the folder and the goal.", "Start planning starts only the lead, in the team folder.",
      "Talk to the lead in its chat to shape the plan.", "Fields the lead changed are marked ● until you look at them.",
      "Create team starts the members and their worktrees; nothing runs before that."]) expect(html).toContain(step);
  });

  it("keeps only its title when collapsed", () => {
    const html = howTo({ open: false });
    expect(html).toContain('data-howto="closed"');
    expect(html).toContain('aria-expanded="false"');
    expect(html).toContain("How it works");
    expect(html).not.toContain("Fill in the folder");
  });

  it("sits first in the form, its collapse kept in a remembered setting, and the plan mode is always editable", () => {
    const form = fs.readFileSync(new URL("./NewTeam.tsx", import.meta.url), "utf8");
    const body = form.indexOf('<div class="nt-body">');
    expect(form.indexOf("<NewTeamHowTo", body)).toBeGreaterThan(body);
    expect(form.indexOf("<NewTeamHowTo", body)).toBeLessThan(form.indexOf('tr("Template")', body));
    expect(form).toContain('const HOWTO_CLOSED = "client/newTeamHowToClosed"');
    expect(form).toContain("setSetting(HOWTO_CLOSED, !open)");
    // The plan mode is outside every draft-only branch: before and after Start planning.
    expect(form).toMatch(/<PlanModeField mode=\{fields\(\)\.plan_mode\}/);
    const defaults = JSON.parse(fs.readFileSync(new URL("../../store/settings-defaults.json", import.meta.url), "utf8")) as Record<string, unknown>;
    expect(defaults["client/newTeamHowToClosed"]).toBe(false);
  });
});
