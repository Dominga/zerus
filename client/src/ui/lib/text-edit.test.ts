import { afterEach, describe, expect, it, vi } from "vitest";
import { Window } from "happy-dom";
import { installTextEditingKeys, inboundEdit, replaceText, setComposing, textEditor } from "./text-edit";

// Only API/dispatch guards are modeled here. Native history is tested with the
// rendered Composer/store fixture in WebKitGTK (scripts/text-edit-smoke.py).
const dom = new Window();
vi.stubGlobal("window", dom);
vi.stubGlobal("document", dom.document);
vi.stubGlobal("HTMLInputElement", dom.HTMLInputElement);
vi.stubGlobal("HTMLTextAreaElement", dom.HTMLTextAreaElement);
const browser = dom as unknown as globalThis.Window;
const Keys = dom.KeyboardEvent as unknown as typeof KeyboardEvent;
const doc = dom.document as unknown as Document;
const command = vi.fn((..._args: unknown[]) => true);
const enabled = vi.fn(() => true);
Object.assign(doc, { execCommand: command, queryCommandEnabled: enabled });
const field = () => { const e = doc.createElement("textarea"); doc.body.append(e); e.focus(); return e; };
afterEach(() => { doc.body.replaceChildren(); command.mockReset().mockReturnValue(true); enabled.mockReset().mockReturnValue(true); });

describe("native text edits", () => {
  it("leaves same-value echoes and unfocused/locked fields untouched", () => {
    const e=field();e.value="draft";
    expect(replaceText(e,"draft")).toBe(true);expect(command).not.toHaveBeenCalled();
    e.blur();expect(replaceText(e,"other")).toBe(false);
    e.focus();e.readOnly=true;expect(replaceText(e,"other")).toBe(false);
    e.readOnly=false;e.disabled=true;expect(replaceText(e,"other")).toBe(false);
    expect(command).not.toHaveBeenCalled();expect(e.value).toBe("draft");
  });
  it("uses the native transaction and marks only a silent inbound edit", () => {
    const e=field();e.value="before";const seen:boolean[]=[];
    command.mockImplementation((_c?: unknown, _ui?: unknown, value?: unknown) => {
      expect(e.selectionStart).toBe(0);expect(e.selectionEnd).toBe(6);
      seen.push(inboundEdit(e));e.value=String(value);return true;
    });
    expect(replaceText(e,"after",false)).toBe(true);expect(seen).toEqual([true]);expect(inboundEdit(e)).toBe(false);
    e.value="before";expect(replaceText(e,"user edit")).toBe(true);expect(seen).toEqual([true,false]);
  });
  it("never edits during composition, and clears inbound marking on a thrown command", () => {
    const e=field();setComposing(e,true);expect(replaceText(e,"other")).toBe(false);expect(command).not.toHaveBeenCalled();
    setComposing(e,false);command.mockImplementation(()=>{throw Error("native refusal");});
    expect(()=>replaceText(e,"other",false)).toThrow("native refusal");expect(inboundEdit(e)).toBe(false);
  });
  it("excludes nontext controls and terminal input", () => {
    for(const type of ["checkbox","range","file","date","color"]){const e=doc.createElement("input");e.type=type;expect(textEditor(e)).toBeUndefined();}
    const e=field(), terminal=doc.createElement("div");terminal.className="xterm";doc.body.append(terminal);terminal.append(e);expect(textEditor(e)).toBeUndefined();
  });
});

describe("editing shortcuts", () => {
  it.each([false,true])("keeps the native manager and platform modifiers (Mac=%s)",mac=>{
    const e=field(),off=installTextEditingKeys(browser,mac);
    const send=(key:string,extra:KeyboardEventInit={})=>{const event=new Keys("keydown",{key,bubbles:true,cancelable:true,...(mac?{metaKey:true}:{ctrlKey:true}),...extra});e.dispatchEvent(event);return event;};
    try {
      expect(send("z").defaultPrevented).toBe(true);expect(command).toHaveBeenLastCalledWith("undo");
      send("z",{shiftKey:true});expect(command).toHaveBeenLastCalledWith("redo");
      send("y");expect(command).toHaveBeenLastCalledWith("redo");
      send("я",{code:"KeyZ"});expect(command).toHaveBeenLastCalledWith("undo");
      command.mockClear();send("z",{altKey:true});send("z",{isComposing:true});
      setComposing(e,true);send("z");setComposing(e,false);
      enabled.mockReturnValue(false);expect(send("z").defaultPrevented).toBe(false);
      expect(command).not.toHaveBeenCalled();
    }finally{off();}
  });
  it("lets the capture router handle an active shortcut sequence first",()=>{
    const e=field(), off=installTextEditingKeys(dom as unknown as globalThis.Window,false);
    const capture=(e:Event)=>e.preventDefault();browser.addEventListener("keydown",capture,true);
    try{e.dispatchEvent(new Keys("keydown",{key:"z",ctrlKey:true,bubbles:true,cancelable:true}));expect(command).not.toHaveBeenCalled();}
    finally{browser.removeEventListener("keydown",capture,true);off();}
  });
});
