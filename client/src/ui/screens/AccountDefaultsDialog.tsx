import { Dropdown } from "../lib/Dropdown";
import {Show,createSignal,onMount,onCleanup} from "solid-js";
import {Icon} from "../lib/icons";
import type {AccountPrompt} from "../../store/accountManagement";
import type {AccountEditValues} from "../../bridge";
import {accountModelChoices,accountModelView,permissionExplanation,savesOn} from "./account-defaults";
import { normalizeModelCatalog, selectModel } from "../provider-selection";
import { tr } from "../../i18n";

/** Mockup SettingsDialogs PermissionsDialog / ModelDialog; Qt account options. */
export default function AccountDefaultsDialog(props:{prompt:AccountPrompt;close(handle:string):void;confirm(values:AccountEditValues,handle:string):void}) {
  const view=props.prompt,permissions=view.kind==="permissions",scope=view.kind==="scope",catalog=view.models;
  const titleId=scope ? "accounts-scope-title" : permissions ? "accounts-permissions-title" : "accounts-model-title";
  const [where,setWhere]=createSignal(view.per_computer ? "computer" : "account");
  const many=(view.machines?.length??1)>1;
  const [mode,setMode]=createSignal(view.permission_mode??"provider"),[model,setModel]=createSignal(catalog?.defaults.model??""),[effort,setEffort]=createSignal(catalog && accountModelView(catalog,catalog.defaults.model).efforts.some(option=>option.id===catalog.defaults.effort) ? catalog.defaults.effort : "");
  let dialog!:HTMLDialogElement;
  onMount(()=>{dialog.showModal();queueMicrotask(()=>dialog.querySelector<HTMLElement>('[role="combobox"]')?.focus());});
  onCleanup(()=>{dialog.close();if(view.origin.isConnected)view.origin.focus();});
  const state=()=>catalog ? accountModelView(catalog,model(),view.provider) : undefined;
  const choose=(value:string)=>{
    setModel(value);
    if(!catalog){setEffort("");return;}
    const next=selectModel(normalizeModelCatalog(catalog),value,value===catalog.defaults.model?catalog.defaults.effort:effort());
    setEffort(catalog.effort_supported?next.effortId:"");
  };
  const inherited=()=>catalog ? `${catalog.provider_defaults.model||tr("Provider model")}${catalog.provider_defaults.effort ? `, ${catalog.provider_defaults.effort} effort` : ""}` : "";
  const submit=(event:Event)=>{event.preventDefault();if(scope)props.confirm({mode:where()},view.handle);else if(permissions)props.confirm({mode:mode()},view.handle);else if(state()?.canSave)props.confirm({model:model(),effort:state()?.effortEnabled?effort():""},view.handle);};
  return <dialog ref={dialog} class="dlg" style={{width:"480px"}} aria-labelledby={titleId} onCancel={event=>{event.preventDefault();props.close(view.handle);}} onClose={()=>props.close(view.handle)}>
    <form onSubmit={submit}>
      <div class="dlg-head"><Icon name={permissions||scope?"shield-check":"sparkles"} size={18} class="accent"/><h2 id={titleId}>{scope?tr("Permissions across computers"):permissions?tr("Account permissions"):tr("Default model and effort")}</h2><span class="grow"/><button type="button" class="icon-btn" aria-label={tr("Close")} onClick={()=>props.close(view.handle)}><Icon name="x" size={16}/></button></div>
      <div class="dlg-body"><p>{savesOn(view)}</p>
        <Show when={!scope} fallback={<>
          <Dropdown class="field acct-perm-select" aria-label={tr("Permissions across computers")} value={where()} onChange={setWhere} options={[{value:"account",label:tr("Shared by every computer")},{value:"computer",label:tr("Per computer")}]} />
          <p>{where()==="computer"
            ? tr("Each computer keeps its own permission mode. Changing it on one computer leaves the others as they are.")
            : tr("One permission mode for every computer with this account. The next change applies everywhere, and a computer that differs is marked.")}</p>
        </>}>
        <Show when={permissions} fallback={<Show when={catalog}>{data=><>
          <div class="form-row"><label for="md-model">{tr("Model")}</label><Dropdown id="md-model" class="field" value={model()} onChange={choose} options={[{value:"",label:tr("Provider settings")},...accountModelChoices(data()).map(option=>({value:option.id,label:option.label}))]} /></div>
          <div class="form-row"><label for="md-effort">{tr("Effort")}</label><Dropdown id="md-effort" class="field" value={effort()} disabled={!state()?.effortEnabled} onChange={setEffort} options={(state()?.efforts??[]).map(option=>({value:option.id,label:option.label}))} /></div>
          <Show when={data().provider_defaults.model||data().provider_defaults.effort}><p class="muted">Account settings: {inherited()}</p></Show>
          <p class="hint">{state()?.hint}</p><p class="hint">{many ? tr("New conversations with this account on these computers. Existing conversations keep their model and effort. Explicit launch choices take precedence.") : tr("New conversations with this account on this machine. Existing conversations keep their model and effort. Explicit launch choices take precedence.")}</p>
        </>}</Show>}>
          <Dropdown class="field acct-perm-select" aria-label={tr("Permission mode")} value={mode()} onChange={setMode} options={[{value:"bypass",label:tr("Bypass permissions")},{value:"provider",label:tr("Provider settings")}]} />
          <p classList={{danger:mode()==="bypass"}}>{permissionExplanation(view,mode())}</p><p class="hint">{tr("New and resumed sessions only. Running sessions keep their mode.")}</p><p class="hint">{tr("Without a choice, Claude, Codex and Kimi accounts bypass permissions.")}</p>
        </Show>
        </Show>
      </div>
      <div class="dlg-foot"><span class="spacer"/><button type="button" class="btn" onClick={()=>props.close(view.handle)}>{tr("Cancel")}</button><button type="submit" class="btn primary" disabled={!permissions&&!scope&&!state()?.canSave}>{tr("Save")}</button></div>
    </form>
  </dialog>;
}
