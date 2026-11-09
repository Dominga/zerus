import { Dropdown } from "../lib/Dropdown";
import { TextInput } from "../lib/TextField";
import {For,Show,createSignal,onMount,onCleanup} from "solid-js";
import {Icon,AgentGlyph} from "../lib/icons";
import {agentKind} from "../../store/catalog";
import {providerName} from "../lib/usage";
import type {SetupPrompt} from "../../store/accountSetup";
import type {AccountSetupValues} from "../../bridge";
import { tr } from "../../i18n";

/** Approved SettingsDialogs AddAccountDialog / ApiKeyDialog; Qt saved profiles. */
export default function AccountSetupDialog(props:{prompt:SetupPrompt;busy:boolean;close(handle:string):void;confirm(values:AccountSetupValues,handle:string):void}) {
 const view=props.prompt;
 const first=view.hosts.find(h=>h.key===(view.kind==="key"?view.host:view.preferredHost))??view.hosts.find(h=>h.key==="")??view.hosts[0];
 const [host,setHost]=createSignal(first?.key??""),[provider,setProvider]=createSignal(view.provider),[saved,setSaved]=createSignal(""),[name,setName]=createSignal(""),[api,setApi]=createSignal(view.kind==="key"),[key,setKey]=createSignal("");
 const [ollamaModel,setOllamaModel]=createSignal(""),[ollamaEndpoint,setOllamaEndpoint]=createSignal("http://127.0.0.1:11434"),[ollamaContext,setOllamaContext]=createSignal(0),[ollamaPrepare,setOllamaPrepare]=createSignal("");
 const machine=()=>view.hosts.find(h=>h.key===host());
 const savedFor=(p=provider())=>machine()?.saved.filter(s=>s.provider===p)??[];
 const chooseAgent=(p:string)=>{setProvider(p);setSaved(savedFor(p)[0]?.id??"");};
 const chooseHost=(h:string)=>{setHost(h);const p=machine()?.providers.includes(provider())?provider():machine()?.providers[0]??"codex";chooseAgent(p);};
 chooseHost(host());
 const reuse=()=>!!saved(),ocListed=()=>machine()?.listed.includes("native-opencode"),canAgent=(p:string)=>p!=="opencode"||savedFor(p).length>0;
 const tabAgent=()=>machine()?.providers.find(p=>p===provider()&&canAgent(p))??machine()?.providers.find(canAgent);
 const stepAgent=(event:KeyboardEvent&{currentTarget:HTMLDivElement})=>{
  if(event.altKey||event.ctrlKey||event.metaKey)return;
  const rows=[...event.currentTarget.querySelectorAll<HTMLButtonElement>(".acct-agent:not(:disabled)")];
  const at=rows.findIndex(row=>row===event.target);
  if(at<0)return;
  const next=event.key==="ArrowDown"||event.key==="ArrowRight"?at+1:event.key==="ArrowUp"||event.key==="ArrowLeft"?at-1:event.key==="Home"?0:event.key==="End"?rows.length-1:undefined;
  if(next===undefined)return;
  event.preventDefault();
  const row=rows[Math.max(0,Math.min(rows.length-1,next))];
  // Moving against an edge must not reset an already chosen saved profile.
  if(row.getAttribute("aria-selected")!=="true")row.click();
  row.focus();
 };
 const detail=(p:string)=>p==="ollama"?tr("Claude Code on a model Ollama serves on this machine"):p==="opencode"?(savedFor(p).length?tr("Add the native configuration again"):ocListed()?tr("Native configuration, already added"):tr("Not installed on %1", machine()?.machine ?? "")):p==="dsh"?(savedFor(p).length?tr("Saved API key or a new one"):tr("DeepSeek API key")):savedFor(p).length?tr("Saved account or a new profile"):tr("New profile with its own sign-in");
 const hint=()=>!machine()?.online?`${machine()?.machine??"Machine"} is offline.`:reuse()&&provider()==="opencode"?tr("Use OpenCode's native configuration on %1 for new sessions again.", machine()?.machine ?? ""):reuse()?tr("Use the saved sign-in on %1. The account will be available for new sessions again.", machine()?.machine ?? ""):provider()==="dsh"?tr("Configure a DeepSeek API key on the selected machine."):provider()==="ollama"?tr("Run Claude Code on a model that Ollama serves on the selected machine. No Anthropic sign-in is needed."):tr("Create a separate account profile on the selected machine, then sign in.");
 const can=()=>!!machine()?.online&&(api()?!!key().trim():provider()==="ollama"?!!ollamaModel().trim()&&!!ollamaEndpoint().trim()&&!!name().trim():canAgent(provider())&&(reuse()||!!name().trim()));
 let dialog!:HTMLDialogElement;let keyInput:HTMLInputElement|undefined;
 onMount(()=>{dialog.showModal();queueMicrotask(()=>dialog.querySelector<HTMLElement>(api()?"input":'[role="combobox"]')?.focus());});
 onCleanup(()=>{setKey("");dialog.close();if(view.origin.isConnected)view.origin.focus();});
 const submit=(e:Event)=>{e.preventDefault();if(!can()||props.busy)return;if(!api()&&!reuse()&&provider()==="dsh"&&machine()?.deepseek_installed){setApi(true);queueMicrotask(()=>keyInput?.focus());return;}props.confirm({host:host(),provider:provider(),saved:saved(),label:view.kind==="key"?view.label:name().trim(),api_key:key().trim(),ollama_model:ollamaModel().trim(),ollama_endpoint:ollamaEndpoint().trim(),ollama_context:ollamaContext(),ollama_prepare:ollamaPrepare().trim()},view.handle);setKey("");};
 const titleId=()=>api()?"accounts-api-key-title":"accounts-add-title";
 return <dialog ref={dialog} class="dlg" style={{width:api()?"460px":"520px"}} aria-labelledby={titleId()} onCancel={e=>{e.preventDefault();if(!props.busy)props.close(view.handle);}} onClose={()=>props.close(view.handle)}><form onSubmit={submit}>
  <div class="dlg-head"><Icon name={api()?"key-round":"user-round-cog"} size={18} class="accent"/><h2 id={titleId()}>{api()?`DeepSeek API key on ${machine()?.machine}`:tr("Add agent account")}</h2><span class="grow"/><button type="button" class="icon-btn" aria-label={tr("Close")} disabled={props.busy} onClick={()=>props.close(view.handle)}><Icon name="x" size={16}/></button></div>
  <div class="dlg-body"><Show when={api()} fallback={<>
   <div class="form-row"><label for="aa-host">{tr("Machine")}</label><Dropdown id="aa-host" class="field" value={host()} onChange={(value) =>chooseHost(value)} options={[...((view.hosts) ?? []).map((h) => ({ value: h.key, label: `${h.label}${h.online ? "" : " (offline)"}` }))]} /></div>
   <div class="form-row acct-agents-row"><label>{tr("Agent")}</label><div class="acct-agents" role="listbox" aria-label={tr("Agent")} onKeyDown={stepAgent}><For each={machine()?.providers}>{p=><button type="button" role="option" class="acct-agent" classList={{on:provider()===p}} aria-selected={provider()===p} tabIndex={p===tabAgent()?0:-1} disabled={!canAgent(p)} title={p==="opencode"?(ocListed()?tr("Configure providers, models and permissions with OpenCode on %1. Zerus uses that native configuration.", machine()?.machine ?? ""):tr("Install OpenCode on %1, then refresh Accounts.", machine()?.machine ?? "")):undefined} style={{"--agent":`var(--z-agent-${agentKind(p)})`}} onClick={()=>chooseAgent(p)}><AgentGlyph agent={agentKind(p)} size={16}/><span class="col"><b>{providerName(p)}</b><span class="muted">{detail(p)}</span></span></button>}</For></div></div>
   <div class="form-row"><label for="aa-saved">{tr("Account")}</label><Dropdown id="aa-saved" class="field" value={saved()} disabled={savedFor().length+(provider()==="opencode"?0:1)<2} onChange={(value) =>setSaved(value)} options={[...((savedFor()) ?? []).map((s) => ({ value: s.id, label: s.label })), ...(provider()!=="opencode" ? [{ value: "", label: tr("New account") }] : [])]} /></div>
   <div class="form-row"><label for="aa-name">{tr("Account name")}</label><TextInput id="aa-name" class="field" maxLength={160} placeholder={tr("Work, Personal, Team…")} disabled={reuse()} value={reuse()?"":name()} onInput={e=>setName(e.currentTarget.value)}/></div><Show when={provider()==="ollama"}><div class="form-row"><label for="aa-ollama-model">{tr("Ollama model")}</label><TextInput id="aa-ollama-model" class="field" maxLength={256} placeholder="qwen3-coder:30b" value={ollamaModel()} onInput={e=>setOllamaModel(e.currentTarget.value)}/></div><div class="form-row"><label for="aa-ollama-endpoint">{tr("Ollama address")}</label><TextInput id="aa-ollama-endpoint" class="field" maxLength={256} value={ollamaEndpoint()} onInput={e=>setOllamaEndpoint(e.currentTarget.value)}/></div><div class="form-row"><label for="aa-ollama-context">{tr("Context window (0: default)")}</label><TextInput id="aa-ollama-context" class="field" type="number" min="0" max="4194304" value={ollamaContext()} onInput={e=>setOllamaContext(Number(e.currentTarget.value)||0)}/></div><div class="form-row"><label for="aa-ollama-prepare">{tr("Prepare command (optional)")}</label><TextInput id="aa-ollama-prepare" class="field" maxLength={1024} value={ollamaPrepare()} onInput={e=>setOllamaPrepare(e.currentTarget.value)}/></div></Show>
  </>}><p>Save an API key on {machine()?.machine}. DeepSeek on that machine will use this key.</p><div class="form-row"><label for="ak-key">{tr("API key")}</label><TextInput ref={keyInput} disabled={props.busy} id="ak-key" class="field" type="password" maxLength={4096} autocomplete="off" value={key()} onInput={e=>setKey(e.currentTarget.value)}/></div></Show></div>
  <div class="dlg-foot"><Show when={!api()}><span class="hint">{hint()}</span></Show><span class="spacer"/><button type="button" class="btn" disabled={props.busy} onClick={()=>props.close(view.handle)}>{tr("Cancel")}</button><button type="submit" class="btn primary" classList={{"acct-add-btn":!api()}} disabled={!can()||props.busy}>{api()?(props.busy?tr("Saving on %1…", machine()?.machine ?? ""):tr("Save on %1", machine()?.machine ?? "")):reuse()?tr("Add account"):provider()==="dsh"?tr("Enter API key…"):provider()==="ollama"?tr("Create Ollama account"):tr("Create and sign in")}</button></div>
 </form></dialog>;
}
