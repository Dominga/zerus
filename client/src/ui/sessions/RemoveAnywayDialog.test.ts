// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
const native=vi.hoisted(()=>({offer:vi.fn(),confirm:vi.fn(),cancel:vi.fn(),apply:vi.fn()}));
vi.mock("../../bridge",()=>({terminationCleanupManualOffer:native.offer,terminationCleanupManualConfirm:native.confirm,terminationCleanupManualCancel:native.cancel}));
vi.mock("../../store/terminationCleanup",()=>({applyCleanup:native.apply,emptyConsents:()=>({local_files:false,discard_changes:false,unchecked_processes:false,merged_branches:false})}));
vi.mock("../../i18n",()=>({tr:(s:string,...args:unknown[])=>s.replace(/%(\d+)/g,(_,n)=>String(args[Number(n)-1]))}));
import { RemoveAnywayDialog } from "./RemoveAnywayDialog";
const view=()=>({confirmation:"original-token",path:"/srv/example/hand-made",computer:"fixture",branch:"retained-branch",size:"4.00 KiB",changes:2,untracked:3,ignored:["local/report.html"],ignored_count:1,ignored_size:"1.00 KiB",required:{local_files:true,discard_changes:true,unchecked_processes:true,merged_branches:false},ready:true,reason:"Fresh native file review",sessions:[],processes:[]});
let dispose:(()=>void)|undefined;
const flush=async()=>{for(let i=0;i<10;i++)await Promise.resolve();};
beforeEach(()=>{vi.clearAllMocks();native.offer.mockResolvedValue(view());native.cancel.mockResolvedValue(undefined);native.confirm.mockResolvedValue({revision:2,rows:[],busy:true});vi.spyOn(HTMLDialogElement.prototype,"showModal").mockImplementation(function(this:HTMLDialogElement){this.open=true;});vi.spyOn(HTMLDialogElement.prototype,"close").mockImplementation(function(this:HTMLDialogElement){this.open=false;});});
afterEach(()=>{dispose?.();dispose=undefined;document.body.replaceChildren();vi.restoreAllMocks();});
const button=(host:HTMLElement,text:string)=>[...host.querySelectorAll<HTMLButtonElement>("button")].find((b)=>b.textContent===text)!;
function mount(){const host=document.createElement("div");document.body.append(host);const close=vi.fn();dispose=render(()=>createComponent(RemoveAnywayDialog,{id:"original-row",close}),host);return {host,close};}
function checkAll(host:HTMLElement){for(const box of host.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')){box.checked=true;box.dispatchEvent(new Event("change",{bubbles:true}));}}
it("shows native deletion scope and requires every discard consent before one call",async()=>{
 const {host,close}=mount();await flush();expect(native.offer).toHaveBeenCalledExactlyOnceWith("original-row");
 for(const text of ["/srv/example/hand-made","fixture","retained-branch","4.00 KiB","Changed files: 2; untracked files: 3","local/report.html"])expect(host.textContent).toContain(text);
 expect(button(host,"Remove working folder").disabled).toBe(true);expect(native.confirm).not.toHaveBeenCalled();
 checkAll(host);expect(button(host,"Remove working folder").disabled).toBe(false);button(host,"Remove working folder").click();button(host,"Remove working folder").click();await flush();
 expect(native.confirm).toHaveBeenCalledExactlyOnceWith("original-row","original-token",{local_files:true,discard_changes:true,unchecked_processes:true,merged_branches:false});expect(native.apply).toHaveBeenCalledTimes(1);expect(close).toHaveBeenCalledTimes(1);
});
it("a known session or program keeps removal disabled despite all consents",async()=>{
 native.offer.mockResolvedValue({...view(),ready:false,sessions:["codex/example/live"],processes:[{pid:42,label:"report-server"}]});const {host}=mount();await flush();checkAll(host);
 expect(host.textContent).toContain("codex/example/live");expect(host.textContent).toContain("report-server (PID 42)");expect(button(host,"Remove working folder").disabled).toBe(true);button(host,"Remove working folder").click();expect(native.confirm).not.toHaveBeenCalled();
});
it("a lost or refused confirmation cannot be sent again from the mounted dialog",async()=>{
 native.confirm.mockRejectedValue(new Error("outcome unavailable"));const {host}=mount();await flush();checkAll(host);button(host,"Remove working folder").click();await flush();
 expect(host.textContent).toContain("This confirmation will not be replayed.");expect(button(host,"Remove working folder").disabled).toBe(true);button(host,"Remove working folder").click();expect(native.confirm).toHaveBeenCalledTimes(1);
});
it("closing before a slow read returns cancels only its own later token without mutation",async()=>{
 let resolve!:(value:ReturnType<typeof view>)=>void;native.offer.mockReturnValue(new Promise((done)=>resolve=done));mount();dispose?.();dispose=undefined;resolve(view());await flush();
 expect(native.cancel).toHaveBeenCalledExactlyOnceWith("original-token");expect(native.confirm).not.toHaveBeenCalled();
});
it("a native identity refusal offers no destructive action",async()=>{
 native.offer.mockRejectedValue(new Error("not the same linked worktree"));const {host}=mount();await flush();expect(host.textContent).toContain("not the same linked worktree");expect(button(host,"Remove working folder").disabled).toBe(true);expect(native.confirm).not.toHaveBeenCalled();
});
