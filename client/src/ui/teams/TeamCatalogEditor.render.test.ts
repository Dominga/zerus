// @vitest-environment happy-dom
import { afterEach, expect, it } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import generatedSchema from "../../../core/src/teams/fixtures/catalog-editor-schema.json";
import builtinCatalog from "../../../../crates/team-plan/fixtures/catalog-builtin.json";
import { TeamCatalogEditor } from "./TeamCatalogEditor";
import { createCatalogEditor, type CatalogSnapshot } from "../../store/catalogEditor";
// Hand-authored schema/transport fixture for rendering; production kernel
// schema and native writer controls live in the Rust suites.
const fixture: CatalogSnapshot = { revision: "r1", person: {}, catalog: {}, builtin: { roles: [{ id: "developer", name: "Developer", prompt: "Instructions", agent: "claude" }], pipelines: [{ id: "reviewed", name: "Reviewed" }], templates: [{ id: "bugfix", name: "Bugfix" }] }, schema_fields: {
  roles: [{ name: "name", label: "Name", type: "text" }, { name: "prompt", label: "Prompt", type: "multiline" }, { name: "agent", label: "Agent", type: "choice", options: ["claude","kimi"] }],
  pipelines: [{ name: "name", label: "Name", type: "text" }], templates: [{ name: "name", label: "Name", type: "text" }],
} };
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
const tick = async () => { await Promise.resolve(); await Promise.resolve(); };
it("renders supported tabs, prompt, edit and save with no unsupported controls", async () => {
  const editor=createCatalogEditor({ async load(){return fixture;},async validate(v){return v;},async importFile(){return null;},async import(v){return v as typeof fixture.person;},async exportFile(){return true;},async save(v){return {outcome:"done",request_id:v.request_id,revision:"r2"};},async receipt(id){return {outcome:"uncertain",request_id:id};} });
  const host=document.createElement("div"); document.body.append(host);
  dispose=render(() => createComponent(TeamCatalogEditor,{editor}),host); await tick();
  expect([...host.querySelectorAll('[role="tab"]')].map(x=>x.textContent)).toEqual(["Roles","Pipelines","Templates"]);
  expect(host.querySelector("textarea")?.value).toBe("Instructions");
  expect(host.textContent).not.toMatch(/Designer|Folder policy|Skills|Messaging/);
  const name=host.querySelector('input:not([type])') as HTMLInputElement;
  // The first input is search; role Name is the next one.
  const field=host.querySelector('.cat-field input') as HTMLInputElement;
  field.value="Builder";field.dispatchEvent(new Event("change",{bubbles:true}));await tick();
  expect(editor.dirty()).toBe(true); expect(name).toBeDefined();
  const save=[...host.querySelectorAll("button")].find(b=>b.textContent==="Save catalog")!;
  expect(save.disabled).toBe(false);save.click();await tick();await tick();expect(editor.dirty()).toBe(false);
});
it("uncertain save shows Check result and disables editing", async () => {
  const editor=createCatalogEditor({async load(){return fixture;},async validate(v){return v;},async importFile(){return null;},async import(v){return v as typeof fixture.person;},async exportFile(){return true;},async save(v){return {outcome:"uncertain",request_id:v.request_id};},async receipt(id){return {outcome:"uncertain",request_id:id};}});
  await editor.load();editor.replace({planning_rules:["mine"]});await editor.save();
  const host=document.createElement("div");document.body.append(host);dispose=render(()=>createComponent(TeamCatalogEditor,{editor}),host);await tick();
  expect(host.textContent).toContain("Check result");expect((host.querySelector("fieldset.cat-form") as HTMLFieldSetElement).disabled).toBe(true);
});

it("renders the kernel schema's pipeline, lead, roster and settings controls", async () => {
  const full: CatalogSnapshot={...fixture,builtin:builtinCatalog,catalog:builtinCatalog,schema_fields:generatedSchema};
  const editor=createCatalogEditor({async load(){return full;},async validate(v){return v;},async importFile(){return null;},async import(v){return v as typeof fixture.person;},async exportFile(){return true;},async save(v){return {outcome:"done",request_id:v.request_id,revision:"r2"};},async receipt(id){return {outcome:"uncertain",request_id:id};}});
  const host=document.createElement("div");document.body.append(host);dispose=render(()=>createComponent(TeamCatalogEditor,{editor}),host);await tick();
  const tabs=[...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')];tabs[1].click();await tick();
  expect(host.textContent).toContain("Implementer");expect(host.textContent).toContain("Maximum rounds");expect(host.querySelectorAll("select").length).toBeGreaterThan(0);
  tabs[2].click();await tick();expect(host.textContent).toContain("Lead rules");expect(host.textContent).toContain("Roster");expect(host.textContent).toContain("Maximum open questions");
  expect([...host.querySelectorAll("label")].some(x=>x.textContent==="About")).toBe(false);
});
it("reveals a newly added role even when the catalog list was filtered", async()=>{
  const editor=createCatalogEditor({async load(){return fixture;},async validate(v){return v;},async importFile(){return null;},async import(v){return v as typeof fixture.person;},async exportFile(){return true;},async save(v){return {outcome:"done",request_id:v.request_id,revision:"r2"};},async receipt(id){return {outcome:"uncertain",request_id:id};}});
  const host=document.createElement("div");document.body.append(host);dispose=render(()=>createComponent(TeamCatalogEditor,{editor}),host);await tick();
  const search=host.querySelector('.cat-list input') as HTMLInputElement;search.value="Developer";search.dispatchEvent(new Event("input",{bubbles:true}));
  (host.querySelector('.cat-add') as HTMLButtonElement).click();await tick();
  expect(host.querySelector('.cat-item-head b')?.textContent).toBe("New item");expect(search.value).toBe("");
});
