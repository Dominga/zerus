import { settings, setSetting } from "./settings";

// Task 74: Qt's SessionLeads (`workspace/sessionLeads`): a session identity
// reports to one lead of the same project, one level deep. The value is the
// same JSON object Qt stores; Tauri keeps it under the same workspace key.

export type LeadLinks = Record<string, string>;

const KEY = "workspace/sessionLeads";

export function leadLinks(): LeadLinks {
  const raw = settings[KEY];
  if (typeof raw !== "string" || !raw.trim()) return {};
  try {
    const value = JSON.parse(raw) as unknown;
    if (!value || typeof value !== "object" || Array.isArray(value)) return {};
    const links: LeadLinks = {};
    for (const [identity, lead] of Object.entries(value as Record<string, unknown>)) {
      if (typeof lead === "string" && lead) links[identity] = lead;
    }
    return links;
  } catch {
    return {};
  }
}

export const leadOf = (identity: string): string => (identity ? leadLinks()[identity] ?? "" : "");
export const subordinatesOf = (identity: string): string[] =>
  identity ? Object.entries(leadLinks()).filter(([, lead]) => lead === identity).map(([child]) => child) : [];

/** Whether `identity` may be linked to `lead`; Qt refuses a lead that reports
 * and a session that already leads. Returns the reason for a refusal. */
export function leadRefusal(identity: string, lead: string, links: LeadLinks = leadLinks()): string {
  if (!identity) return "Choose a session.";
  if (identity === lead) return "A session cannot report to itself.";
  if (!lead) return "";
  if (links[lead]) return "That session already reports to a lead, so it cannot lead another.";
  if (Object.values(links).includes(identity)) return "That session already leads another session.";
  return "";
}

export function setLead(identity: string, lead: string): Promise<boolean> {
  const links = leadLinks();
  const refusal = leadRefusal(identity, lead, links);
  if (refusal) return Promise.resolve(false);
  const next = { ...links };
  if (lead) next[identity] = lead;
  else delete next[identity];
  return setSetting(KEY, JSON.stringify(next));
}
