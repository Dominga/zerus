// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
const fixture = vi.hoisted(() => ({ activated: [] as string[] }));
vi.mock("@tanstack/solid-virtual", () => ({ createVirtualizer: (options: {count: number; getItemKey(n: number): string}) => ({ measure() {}, measureElement() {}, _willUpdate() {}, scrollToIndex() {}, getTotalSize: () => options.count * 90, getVirtualItems: () => Array.from({length: options.count}, (_, index) => ({key: options.getItemKey(index),start:index*90,index})) }) }));
vi.mock("../../store/contentSearch", async () => {
 const {createSignal} = await import("solid-js");
 const {searchRows} = await import("../../store/content-search-model");
 const [cursor,setCursor] = createSignal("");
 const session = {key:"one",name:"codex/demo/one",title:"One",agent:"codex",machine:"example",computer:"",runId:"run-one",conversationId:"conversation-one"};
 const entries = [1,2].map(n => ({id:`one\nm${n}`,session,run:"run-one",conversation:"conversation-one",found:{computer:"",machine:"example",hit:{message_id:`m${n}`,at:n,role:"assistant",snippet:"needle",event:{type:"AgentMessage",detail:"needle"}}}}));
 return {contentSearch:{errors:()=>[],busy:()=>false,refresh(){}},searchItems:()=>searchRows(entries as never).rows,searchStatus:()=>"2 messages, 1 session",activateSearchItem:(item:{id:string})=>fixture.activated.push(item.id),searchCursor:cursor,setSearchCursor:setCursor};
});
vi.mock("../../store/creations",()=>({searchCards:()=>[],selectCreation(){},selectedCreation:()=>undefined}));
vi.mock("../../store/ui",()=>({search:()=>"needle",selectedKey:()=>"one",stateFilter:()=>"all"}));
// SearchResults reads the archive's partial-scan state (zerus-080n8); the real store pulls in the catalog.
vi.mock("../../store/archive",()=>({archive:{partial:()=>false,partialNewer:()=>false,searched:()=>0}}));
vi.mock("../../store/catalog",()=>({agentKind:(cmd:string)=>cmd}));
vi.mock("./SessionCard",()=>({AgentBadge:()=>null,MachineBadge:()=>null}));
import {SearchResults} from "./SearchResults";
let dispose:(()=>void)|undefined;
afterEach(()=>{dispose?.();document.body.replaceChildren();});
// zerus-ge5xc (review-win-d on #1849): a single-select listbox marks one option.
it("a single-select search list has one current result before a cursor is chosen",()=>{
 const host=document.createElement("div");document.body.append(host);dispose=render(()=>createComponent(SearchResults,{}),host);
 const rows=[...host.querySelectorAll('[role="option"]')];expect(rows).toHaveLength(2);
 const selected=host.querySelectorAll('[role="option"][aria-selected="true"]');
 expect(selected).toHaveLength(1);
 expect(selected[0]).toBe(rows[0]);
});
