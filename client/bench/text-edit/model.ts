import { createComposer, type ComposerPort } from "../../src/store/composer";
import type { ComposerView } from "../../src/types";
export const view = (extra: Partial<ComposerView> = {}): ComposerView => ({
  handle: 1, key: "fixture\nclaude/project/test", loaded: true, rev: 0, text_epoch: 1, text: "", position: 0, anchor: 0, question: null, input_visible: true, queue: null,
  attachments: [], removed: null, attaching: false, sending: false, waiting: null, uncertain: false, status: "", status_error: false,
  can_send: true, send_label: { source: "Send" }, available: true, unavailable_reason: "", block_reason: null, attachments_available: true,
  attachments_reason: "", session_status: "", session_status_error: false, session_blocks_input: false, inspect_error: false,
  cache: { visible: false, tone: "quiet", label: "", short_label: "" }, cache_warning: "", cache_details: "",
  context: { visible: false, text: "", tooltip: "", warning: false, tone: "quiet", percent: null, accessible_name: "" }, local: [],
  cancel_send: false, compacting: false,
  clear: { enabled: true, dismiss: false, title: "Clear session?", text: "", confirm: "Clear session", run: "run-1", conversation: "conv-1",
    cache_enabled: true, running: false, reason: "", cache_reason: "" },
  ...extra,
});

export const edits: unknown[][]=[];
export const sends: string[]=[];
let state=view();
const port={open:async()=>state,
 edit:async(...args: Parameters<ComposerPort["edit"]>)=>{ edits.push(args); const [handle,rev,text_epoch,text,position,anchor]=args;
   // Delayed same-epoch views must not rewrite newer local input.
   setTimeout(()=>model.apply({...state,handle,rev,text_epoch,text,position,anchor}),30);
 },
 send:async(_h:number,_r:number,_e:number,text:string)=>{sends.push(text); return {kind:"sending"} as const;},
 push:async()=>({kind:"sending"} as const),interrupted:async()=>{},interruptFailed:async()=>{},
} as ComposerPort;
export const model=createComposer(port);
export const open=async()=>{await model.open({computer:"fixture",key:"run:fixture",name:"claude/project/test",archive_id:null});};
export const handBack=(text:string)=>{state=view({rev:state.rev+100,text_epoch:state.text_epoch+1,text,anchor:text.length,position:text.length});model.apply(state);};
