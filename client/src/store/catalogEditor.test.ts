import { describe, expect, it } from "vitest";
import { createCatalogEditor, type CatalogPort, type CatalogSnapshot, type CatalogOutcome } from "./catalogEditor";
// Hand-authored transport snapshots/outcomes, not store rows. Kernel/native
// writer fixtures are covered separately by catalog148 and catalog_edit.
const snapshot: CatalogSnapshot = { revision: "r1", person: {}, builtin: { roles: [{ id: "developer", name: "Developer", agent: "claude", prompt: "Keep this prompt" }] }, catalog: {}, schema_fields: { roles: [], pipelines: [], templates: [] } };
function setup() {
  let saves = 0, reads = 0, validates = 0;
  let outcome: CatalogOutcome["outcome"] = "done";
  const port: CatalogPort = {
    async load() { reads++; return structuredClone(snapshot); },
    async validate(value) { validates++; if (value.invalid) throw Error("kernel refused"); return value; },
    async importFile() { return { roles: [] }; }, async import(value) { return value as typeof snapshot.person; }, async exportFile() { return true; },
    async save(edit) { saves++; return { outcome, request_id: edit.request_id, revision: "r2", detail: outcome }; },
    async receipt(request) { return { outcome, request_id: request, revision: "r2" }; },
  };
  const editor = createCatalogEditor(port);
  return { editor, port, counts: () => ({ saves, reads, validates }), outcome: (value: typeof outcome) => outcome = value };
}
describe("catalog editor custody", () => {
  it("validates before saving, keeps the complete prompt and saves exactly once", async () => {
    const s = setup(); await s.editor.load();
    s.editor.setItem("roles", "developer", { ...s.editor.items("roles")[0], name: "Builder" });
    await s.editor.save();
    expect(s.counts()).toEqual({ saves: 1, reads: 1, validates: 1 });
    expect(s.editor.working().roles?.[0].prompt).toBe("Keep this prompt");
    expect(s.editor.dirty()).toBe(false);
  });
  it("kernel refusal leaves the draft and sends nothing", async () => {
    const s = setup(); await s.editor.load(); s.editor.replace({ invalid: true }); await s.editor.save();
    expect(s.counts().saves).toBe(0); expect(s.editor.dirty()).toBe(true);
  });
  it("uncertainty freezes the original UUID and payload; Check only reads", async () => {
    const s = setup(); await s.editor.load(); s.editor.replace({ planning_rules: ["rules"] }); s.outcome("uncertain");
    await s.editor.save(); const intent = s.editor.pending();
    await s.editor.save(); s.editor.replace({}); await s.editor.revert(); await s.editor.importFile(); await s.editor.load();
    expect(s.editor.pending()).toEqual(intent); expect(s.editor.working()).toEqual(intent?.catalog);
    await s.editor.check(); expect(s.counts().saves).toBe(1); expect(s.counts().reads).toBe(1);
    s.outcome("done"); await s.editor.check(); expect(s.editor.pending()).toBeUndefined();
  });
  it("stale refusal preserves edits and revert needs confirmation", async () => {
    const s = setup(); await s.editor.load(); s.editor.replace({ planning_rules: ["mine"] }); s.outcome("failed"); await s.editor.save();
    expect(s.editor.pending()).toBeUndefined(); expect(s.editor.working()).toEqual({ planning_rules: ["mine"] });
    await s.editor.revert(); expect(s.counts().reads).toBe(1); s.editor.cancel(); expect(s.editor.dirty()).toBe(true);
    await s.editor.revert(); s.editor.confirm(); await Promise.resolve(); await Promise.resolve();
    expect(s.editor.dirty()).toBe(false);
  });
  it("a mismatched result cannot settle the pending intent", async () => {
    const s = setup(); s.port.save = async () => ({ outcome: "done", request_id: "other", revision: "r2" });
    await s.editor.load(); s.editor.replace({ planning_rules: ["mine"] }); await s.editor.save();
    expect(s.editor.pending()).toBeDefined(); expect(s.editor.dirty()).toBe(true);
  });
  it("import remains a confirmed unsaved draft and never writes", async () => {
    const s = setup(); await s.editor.load(); await s.editor.importFile(); expect(s.editor.dirty()).toBe(false);
    s.editor.confirm(); expect(s.editor.dirty()).toBe(true); expect(s.counts().saves).toBe(0);
  });
});
it("bounds the working copy and does not replace it with oversized input", async () => {
  const s=setup();await s.editor.load();s.editor.replace({planning_rules:["x".repeat(240001)]});
  expect(s.editor.working()).toEqual({});expect(s.editor.notice()).toContain("size limit");
});
it("Check binds the original request and never reposts a pending save", async () => {
  const s=setup();let read="";s.outcome("uncertain");s.port.receipt=async(id)=>{read=id;return {outcome:"uncertain",request_id:id};};
  await s.editor.load();s.editor.replace({planning_rules:["held"]});await s.editor.save();const id=s.editor.pending()!.request_id;await s.editor.check();
  expect(read).toBe(id);expect(s.counts().saves).toBe(1);expect(s.editor.pending()?.request_id).toBe(id);
});
it("confirmation is required for removing an override; built-ins return after confirmation",async()=>{
  const s=setup();await s.editor.load();s.editor.setItem("roles","developer",{...s.editor.items("roles")[0],name:"Mine"});
  s.editor.remove("roles","developer");expect(s.editor.items("roles")[0].name).toBe("Mine");s.editor.confirm();expect(s.editor.items("roles")[0].name).toBe("Developer");
});
it("discarding an uncertain draft requires confirmation and reads current state without replay", async()=>{
  const s=setup();s.outcome("uncertain");await s.editor.load();s.editor.replace({planning_rules:["mine"]});await s.editor.save();
  s.editor.discardUncertain();expect(s.editor.pending()).toBeDefined();s.editor.cancel();expect(s.editor.pending()).toBeDefined();
  s.editor.discardUncertain();s.editor.confirm();await Promise.resolve();await Promise.resolve();
  expect(s.editor.pending()).toBeUndefined();expect(s.counts().saves).toBe(1);expect(s.counts().reads).toBe(2);
});
it("refreshes the New team catalog only after a confirmed save or receipt", async()=>{
  const s=setup();let changes=0;s.port.changed=()=>{changes++;};await s.editor.load();s.editor.replace({planning_rules:["mine"]});s.outcome("uncertain");await s.editor.save();expect(changes).toBe(0);
  s.outcome("done");await s.editor.check();expect(changes).toBe(1);
});
it("a receipt lookup refusal keeps the original uncertain edit",async()=>{
  const s=setup();s.outcome("uncertain");await s.editor.load();s.editor.replace({planning_rules:["mine"]});await s.editor.save();const held=s.editor.pending();
  s.port.receipt=async(id)=>({outcome:"failed",request_id:id,detail:"Preview off"});await s.editor.check();
  expect(s.editor.pending()).toEqual(held);expect(s.editor.working()).toEqual(held!.catalog);expect(s.counts().saves).toBe(1);
});
it("counts the pretty stored form and newline at the exact writer boundary",async()=>{
  const s=setup();await s.editor.load();
  const roles=Array.from({length:200},(_,i)=>({id:`r${String(i).padStart(3,"0")}`,name:"Role",agent:"codex",prompt:"x".repeat(1102)}));
  const value={roles};expect(new TextEncoder().encode(JSON.stringify(value)).length).toBe(231611);
  expect(new TextEncoder().encode(JSON.stringify(value,null,2)+"\n").length).toBe(240020);
  s.editor.replace(value);expect(s.editor.working()).toEqual({});
  roles[199].prompt=roles[199].prompt.slice(0,-20);
  expect(new TextEncoder().encode(JSON.stringify(value,null,2)+"\n").length).toBe(240000);
  s.editor.replace(value);expect(s.editor.working().roles).toHaveLength(200);
  roles[199].prompt+="x";s.editor.replace(value);
  expect((s.editor.working().roles![199].prompt as string).length).toBe(1082);
});
