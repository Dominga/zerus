/** The Team catalog model: the saved roles, templates and routing, and the
 * designer's draft on top of it. The editor's manual edits stay in the
 * component; this module owns the pure rules Qt's catalog tests cover. */

export interface CatalogRole {
  id: string;
  name: string;
  agent: string;
  model: string;
  prompt: string;
  raw: Record<string, unknown>;
}

export interface CatalogSeat {
  role: string;
  count: number;
  optional: boolean;
}

export interface CatalogTemplate {
  id: string;
  name: string;
  description: string;
  planning: string;
  approval: string;
  maxMembers: number;
  pipeline: string;
  roster: CatalogSeat[];
  raw: Record<string, unknown>;
}

export interface CatalogRule {
  id: string;
  when: string;
  template: string;
  use: string;
  approval: string;
  raw: Record<string, unknown>;
}

export interface CatalogStage {
  kind: string;
  role: string;
  member: string;
  command: string;
  raw: Record<string, unknown>;
}

export interface CatalogPipeline {
  id: string;
  name: string;
  stages: CatalogStage[];
  maxRounds: number;
  folder: string;
  accept: string;
  raw: Record<string, unknown>;
}

export interface CatalogProposal {
  id: string;
  kind: string;
  action: string;
  target: string;
  title: string;
  reason: string;
  state: string;
  value: unknown;
  text: string;
  at: number;
}

export interface Catalog {
  version: number;
  builtin: boolean;
  roles: CatalogRole[];
  templates: CatalogTemplate[];
  routing: CatalogRule[];
  planningRules: string;
  pipelines: CatalogPipeline[];
  draftBase: number;
  proposals: CatalogProposal[];
}

const str = (value: unknown): string => (typeof value === "string" ? value : "");
const num = (value: unknown): number => (typeof value === "number" ? value : 0);
const obj = (value: unknown): Record<string, unknown> =>
  value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

export function parseRole(value: unknown): CatalogRole {
  const raw = obj(value);
  return {
    id: str(raw.id),
    name: str(raw.name),
    agent: str(raw.agent),
    model: str(raw.model),
    prompt: str(raw.prompt),
    raw,
  };
}

export function parseTemplate(value: unknown): CatalogTemplate {
  const raw = obj(value);
  return {
    id: str(raw.id),
    name: str(raw.name),
    description: str(raw.description),
    planning: str(raw.planning),
    approval: str(raw.approval),
    maxMembers: num(raw.max_members),
    pipeline: str(raw.pipeline),
    roster: list(raw.roster).map((seat) => ({
      role: str(obj(seat).role),
      count: num(obj(seat).count),
      optional: obj(seat).optional === true,
    })),
    raw,
  };
}

export function parseRule(value: unknown): CatalogRule {
  const raw = obj(value);
  return {
    id: str(raw.id),
    when: str(raw.when),
    template: str(raw.template),
    use: str(raw.use),
    approval: str(raw.approval),
    raw,
  };
}

export function parsePipeline(value: unknown): CatalogPipeline {
  const raw = obj(value);
  return {
    id: str(raw.id),
    name: str(raw.name),
    stages: list(raw.stages).map((stage) => ({
      kind: str(obj(stage).kind),
      role: str(obj(stage).role),
      member: str(obj(stage).member),
      command: str(obj(obj(stage).command).command),
      raw: obj(stage),
    })),
    maxRounds: num(raw.max_rounds),
    folder: str(raw.folder),
    accept: str(raw.accept),
    raw,
  };
}

export function parseProposal(value: unknown): CatalogProposal {
  const raw = obj(value);
  const change = raw.value;
  return {
    id: str(raw.id),
    kind: str(raw.kind),
    action: str(raw.action),
    target: str(raw.target),
    title: str(raw.title),
    reason: str(raw.reason),
    state: str(raw.state) || "proposed",
    value: change,
    text: typeof change === "string" ? change : "",
    at: num(raw.at),
  };
}

export function parseCatalog(view: Record<string, unknown>): Catalog {
  return {
    version: num(view.version),
    builtin: view.builtin === true,
    roles: list(view.roles).map(parseRole),
    templates: list(view.templates).map(parseTemplate),
    routing: list(view.routing).map(parseRule),
    planningRules: str(view.planning_rules),
    pipelines: list(view.pipelines).map(parsePipeline),
    draftBase: num(view.draft_base),
    proposals: list(view.proposals).map(parseProposal),
  };
}

export function roleById(catalog: Catalog, id: string): CatalogRole | undefined {
  return catalog.roles.find((role) => role.id === id);
}

export function templateById(catalog: Catalog, id: string): CatalogTemplate | undefined {
  return catalog.templates.find((template) => template.id === id);
}

/** Applies a kept proposal (or every proposal not rejected when the draft is
 * shown): update by target, add, or remove. */
export function withProposals(catalog: Catalog, includeUndecided: boolean): Catalog {
  const next: Catalog = {
    ...catalog,
    roles: [...catalog.roles],
    templates: [...catalog.templates],
    routing: [...catalog.routing],
    pipelines: [...catalog.pipelines],
  };
  for (const proposal of catalog.proposals) {
    if (proposal.state === "rejected" || (!includeUndecided && proposal.state !== "kept")) continue;
    const remove = proposal.action === "remove";
    const apply = <T extends { id: string }>(items: T[], parsed: T) => {
      const at = items.findIndex((item) => item.id === proposal.target);
      if (remove) {
        if (at >= 0) items.splice(at, 1);
      } else if (at >= 0) {
        items[at] = parsed;
      } else {
        items.push(parsed);
      }
    };
    if (proposal.kind === "role") apply(next.roles, parseRole(proposal.value));
    else if (proposal.kind === "template") apply(next.templates, parseTemplate(proposal.value));
    else if (proposal.kind === "rule") apply(next.routing, parseRule(proposal.value));
    else if (proposal.kind === "pipeline") apply(next.pipelines, parsePipeline(proposal.value));
    else if (proposal.kind === "planning_rules" && !remove) next.planningRules = proposal.text;
  }
  return next;
}

export function proposalCounts(catalog: Catalog): { kept: number; undecided: number; rejected: number } {
  return {
    kept: catalog.proposals.filter((proposal) => proposal.state === "kept").length,
    undecided: catalog.proposals.filter((proposal) => proposal.state !== "kept" && proposal.state !== "rejected").length,
    rejected: catalog.proposals.filter((proposal) => proposal.state === "rejected").length,
  };
}

/** Apply's confirmation text: what is written and what is dropped. */
export function applyConfirmation(catalog: Catalog): string {
  const { kept, undecided } = proposalCounts(catalog);
  const written = `${kept} kept ${kept === 1 ? "change" : "changes"}`;
  const dropped = `${undecided} undecided ${undecided === 1 ? "change is" : "changes are"}`;
  return `Write ${written} as the next catalog version? The ${dropped} dropped with the draft.`;
}

export interface TemplateFilter {
  query?: string;
  favouritesOnly?: boolean;
  favourites?: string[];
}

/** The template list's rows: search by name, id, description or seat role,
 * favourites first, then by name, with "No template" last. */
export function visibleTemplates(catalog: Catalog, filter: TemplateFilter = {}): string[] {
  const query = (filter.query ?? "").trim().toLowerCase();
  const favourites = new Set(filter.favourites ?? []);
  const roleName = (id: string) => roleById(catalog, id)?.name ?? id;
  const shown = catalog.templates.filter((template) => {
    if (filter.favouritesOnly && !favourites.has(template.id)) return false;
    if (!query) return true;
    return (
      template.name.toLowerCase().includes(query) ||
      template.id.toLowerCase().includes(query) ||
      template.description.toLowerCase().includes(query) ||
      template.roster.some(
        (seat) => seat.role.toLowerCase().includes(query) || roleName(seat.role).toLowerCase().includes(query),
      )
    );
  });
  shown.sort((a, b) => {
    const fa = favourites.has(a.id);
    const fb = favourites.has(b.id);
    if (fa !== fb) return fa ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  return [...shown.map((template) => template.id), "-"];
}

const STOP_WORDS = new Set(["label", "files", "file", "the", "and", "with", "when", "services", "src"]);

export function routeMatch(when: string, goal: string): boolean {
  const word = when.toLowerCase();
  if (word.includes("everything") || word.trim() === "*") return true;
  for (const match of word.matchAll(/[a-z0-9]{3,}/g)) {
    const token = match[0];
    if (STOP_WORDS.has(token)) continue;
    if (goal.includes(token) || (token.length > 4 && token.endsWith("s") && goal.includes(token.slice(0, -1)))) return true;
  }
  return false;
}

export interface CatalogRoute {
  rule: number;
  templateId: string;
  added: string[];
  approval: string;
}

/** The template a goal would use, as Try it shows it. */
export function route(catalog: Catalog, goal: string): CatalogRoute {
  const lowered = goal.toLowerCase();
  const found: CatalogRoute = { rule: -1, templateId: "", added: [], approval: "" };
  for (let index = 0; index < catalog.routing.length; index += 1) {
    const rule = catalog.routing[index];
    if (!routeMatch(rule.when, lowered)) continue;
    const use = rule.use || rule.template;
    if (use.startsWith("+")) {
      if (!found.added.includes(use.slice(1))) found.added.push(use.slice(1));
      continue;
    }
    const template = templateById(catalog, use);
    if (template) {
      found.rule = index;
      found.templateId = template.id;
      found.approval = rule.approval || template.approval;
      return found;
    }
  }
  if (catalog.templates.length > 0) {
    found.templateId = catalog.templates[0].id;
    found.approval = catalog.templates[0].approval;
  }
  return found;
}

export function seatCount(template: CatalogTemplate): number {
  return template.roster.reduce((total, seat) => total + seat.count, 0);
}
