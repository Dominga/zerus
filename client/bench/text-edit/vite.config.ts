import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { fileURLToPath } from "node:url";

// Only the surrounding services/chips are stand-ins. Composer, its optimistic
// draft store, suggestions, TextField and the native editing commands are real.
const empty = "export const EMPTY = () => null;";
const mocks: Record<string, string> = {
  "../../store/composer": 'export { model as composer } from "/bench/text-edit/model.ts";',
  "../../store/ui": 'export const selected=()=>({computer:"fixture",key:"run:fixture",catalogKey:"fixture",state:"running",phase:"idle",tracked:true}); export const contentScale=()=>1,subagentOpen=()=>false;',
  "../../store/catalog": 'export const computerFresh=()=>true, computerOffline=()=>false;',
  "../../store/actions": 'export const detailsFor=()=>undefined, followedFor=()=>({value:{phase:"idle",runtime_state:"live",prompt_suggestion:{text:"hello completion",at:1}}});',
  "../../store/voice": 'export const voicePort={};export const createVoice=()=>({follow(){},close:async()=>{},shown:()=>undefined});',
  "../../store/settings": 'export const flag=()=>false;',
  "./ActionDialogs": 'export const actionEntries=()=>({stop:{shown:false,disabled:true}});',
  "./mark-entries": 'export const markReadChip=()=>({active:false}),runMarkRead=()=>{};',
  "./actions-ui": 'export const snapshotOf=()=>({});',
  "./Activity": 'export const following=()=>true, unseen=()=>0, jumpToLatest=()=>{}, focusTranscript=()=>{};',
  "./VoiceControls": empty+'export {EMPTY as VoiceToolbar,EMPTY as VoicePreview};',
  "./ModelSettings": empty+'export {EMPTY as ModelSettingsButton};',
  "./Attachments": empty+'export {EMPTY as AttachmentsChip};',
  "./RecoveryChip": empty+'export {EMPTY as RecoveryChip};',
  "./UsageChips": empty+'export {EMPTY as SessionUsageChip,EMPTY as UsageLimitChip};',
};
export default defineConfig({
  root: fileURLToPath(new URL("../..", import.meta.url)), base: "./",
  plugins: [{ name: "text-edit-fixture-services", enforce: "pre", resolveId(id, importer) {
    if (importer?.endsWith("/ui/sessions/Composer.tsx") && id in mocks) return "\0text-edit:"+id;
  }, load(id) { if(id.startsWith("\0text-edit:")) return mocks[id.slice(11)]; } }, solid()],
  build: { target: "safari16", rollupOptions: { input: fileURLToPath(new URL("index.html",import.meta.url)) } },
});
