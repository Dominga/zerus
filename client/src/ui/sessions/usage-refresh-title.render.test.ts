// @vitest-environment happy-dom
// zerus-f5gta (review-win-d's control on #1868): the usage refresh button's
// tooltip names clock times and holds still under the pointer, and the
// account tooltip beside it holds too.
import {afterEach,expect,it,vi} from "vitest";
import {createComponent} from "solid-js";
import {render} from "solid-js/web";
const fixture=vi.hoisted(()=>({tick:(_:number)=>{}}));
vi.hoisted(()=>{const storage=new Map<string,string>();vi.stubGlobal("location",new URL("http://localhost/"));vi.stubGlobal("localStorage",{getItem:(k:string)=>storage.get(k)??null,setItem:(k:string,v:string)=>storage.set(k,v),removeItem:(k:string)=>storage.delete(k)});});
vi.mock("../../store/latest",async original=>{const {createSignal}=await import("solid-js");const [now,setNow]=createSignal(10000);fixture.tick=setNow;return{...await original<object>(),useNow:()=>now};});
vi.mock("../../store/sessionUsage",async original=>{const {usageOf}=await import("../../store/testing");return{...await original<object>(),sessionUsage:()=>usageOf({status:"rate_limited",checked_at:9880,retry_at:10120}),followSessionUsage(){},startSessionUsage(){},stopSessionUsage(){},refreshSessionUsage:async()=>{}};});
vi.mock("../../store/catalog",async original=>({...await original<object>(),computerOffline:()=>false}));
import {AccountUsageChip} from "./UsageChips";
let dispose:(()=>void)|undefined;
afterEach(()=>{dispose?.();document.body.replaceChildren();});
it("a hovered usage refresh title stays still when its sample age crosses a minute",()=>{
 expect(document.body).toBeTruthy();
 const host=document.createElement("div");document.body.append(host);dispose=render(()=>createComponent(AccountUsageChip,{session:{key:"one",name:"codex/demo/one",computer:"",agent:"codex",tracked:true,cmd:"codex"} as never}),host);
 const refresh=host.querySelector<HTMLButtonElement>(".usage-refresh")!;const account=host.querySelector<HTMLButtonElement>(".account-usage")!;expect(refresh).toBeTruthy();
 const before=refresh.title;const accountBefore=account.title;refresh.dispatchEvent(new Event("pointerenter"));account.dispatchEvent(new Event("pointerenter"));fixture.tick(10060);
expect(account.title).toBe(accountBefore);expect(refresh.title).toBe(before);
});
it("names the retry and the sample by their clock times, so it reads the same a minute later", () => {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(AccountUsageChip, { session: { key: "two", name: "codex/demo/two", computer: "", agent: "codex", tracked: true, cmd: "codex" } as never }), host);
  fixture.tick(10000);
  const refresh = host.querySelector<HTMLButtonElement>(".usage-refresh")!;
  const before = refresh.title;
  expect(before).toMatch(/^Limits from .+, rate limited\. Retry at .+$/);
  expect(before).not.toMatch(/min ago|Retry in/);
  fixture.tick(10060);
  expect(refresh.title, "unhovered, a minute later").toBe(before);
});
