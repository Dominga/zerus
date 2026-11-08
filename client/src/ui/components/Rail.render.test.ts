// @vitest-environment happy-dom
// The rendered Rail (review-win-c on #1820, zerus-nfmh1): the capped badge, its exact name and tooltip.
import { afterEach,expect,it,vi } from "vitest";
import { createComponent,createSignal } from "solid-js";
import { render } from "solid-js/web";
const f=vi.hoisted(()=>({count:(()=>0) as ()=>number,layout:(()=>"icons") as ()=>string}));
vi.mock("../../store/ui",()=>({page:()=>"sessions",setPage:vi.fn(),counts:()=>({attention:f.count()}),setMenu:vi.fn(),logoMotion:()=>"spin"}));
vi.mock("../../skin/runtime",()=>({skin:()=>({layout:{rail:f.layout()}})}));
vi.mock("../../store/windowLayer",()=>({windowLayer:()=>({on:false,supported:true,hint:""}),setWindowLayer:vi.fn()}));
vi.mock("../sessions/mark-entries",()=>({markAllEntry:()=>({label:"Mark all read"})}));
vi.mock("../sessions/background-menu",()=>({expandAllCollapsedEntry:()=>({label:"Expand all"})}));
vi.mock("../sessions/newsession-view",()=>({newSessionFromRail:vi.fn()}));
vi.mock("../../store/desk",()=>({openAbout:vi.fn()}));
vi.mock("../../store/buildInfo",()=>({appBuildInfo:()=>undefined,refreshAppBuildInfo:vi.fn(async()=>{})}));
vi.mock("./ZerusLogo",()=>({ZerusLogo:()=>null}));
vi.mock("../lib/icons",()=>({Icon:()=>null}));
import { Rail } from "./Rail";
import { setSetting } from "../../store/settings";
let dispose:(()=>void)|undefined;
afterEach(async()=>{dispose?.();dispose=undefined;document.body.replaceChildren();await setSetting("shortcuts/go.sessions",null);});
const setup=(count:number,layout="icons")=>{
 const [n,setN]=createSignal(count);const [l,setL]=createSignal(layout);f.count=n;f.layout=l;
 const host=document.createElement("div");document.body.append(host);dispose=render(()=>createComponent(Rail,{}),host);
 return {host,setN,setL,button:()=>host.querySelector<HTMLButtonElement>(".rail-btn")!};
};
it("the actual icon rail keeps an exact name and shortcut while its visual count caps",()=>{
 const t=setup(0);expect(t.button().getAttribute("aria-label")).toBe("Sessions");expect(t.host.querySelector(".rail-badge")).toBeNull();
 for(const n of [1,99,100,12345]){
  t.setN(n);expect(t.host.querySelector(".rail-badge")?.textContent).toBe(n>99?"99+":String(n));expect(t.host.querySelector(".rail-badge")?.getAttribute("aria-hidden")).toBe("true");
  expect(t.button().getAttribute("aria-label")).toBe(`Sessions — ${n} need attention`);expect(t.button().getAttribute("data-tip")).toBe(`Sessions — ${n} need attention (Ctrl+2)`);
 }
 t.setN(0);expect(t.host.querySelector(".rail-badge")).toBeNull();expect(t.button().getAttribute("aria-label")).toBe("Sessions");
});
it("an overridden shortcut remains in the actual count tooltip",async()=>{
 const t=setup(101);await setSetting("shortcuts/go.sessions",["Ctrl+Shift+S"]);expect(t.button().getAttribute("data-tip")).toBe("Sessions — 101 need attention (Ctrl+Shift+S)");
});
it("the labeled rail also exposes the exact count in its hover tooltip",()=>{
 const t=setup(12345,"labeled");expect(t.host.querySelector(".rail-badge")?.textContent).toBe("99+");expect(t.button().getAttribute("aria-label")).toBe("Sessions — 12345 need attention");
 expect(t.button().getAttribute("data-tip")).toBe("Sessions — 12345 need attention (Ctrl+2)");
});
