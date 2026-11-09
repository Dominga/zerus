import { describe, expect, it } from "vitest";
import ts from "typescript";
import { readdirSync, readFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ui=dirname(dirname(fileURLToPath(import.meta.url)));
function files(dir:string):string[]{return readdirSync(dir,{withFileTypes:true}).flatMap(e=>e.isDirectory()?files(join(dir,e.name)):e.name.endsWith(".tsx")?[join(dir,e.name)]:[]);}
describe("editable controlled fields",()=>{
  it("uses native-history fields across screens, tasks and dialogs",()=>{
    const missed:string[]=[];
    for(const file of files(ui)){
      if(basename(file)==="TextField.tsx")continue;
      const tree=ts.createSourceFile(file,readFileSync(file,"utf8"),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
      const visit=(node:ts.Node)=>{
        if(ts.isJsxSelfClosingElement(node)||ts.isJsxOpeningElement(node)){
          const name=node.tagName.getText(tree),attributes=node.attributes.properties.filter(ts.isJsxAttribute);
          const attr=(key:string)=>attributes.find(a=>a.name.getText(tree).toLowerCase()===key.toLowerCase());
          const type=attr("type")?.initializer, value=type && ts.isStringLiteral(type)?(type as ts.StringLiteral).text:"text";
          const readonly=attr("readonly"), uneditable=readonly && (!readonly.initializer || readonly.initializer.getText(tree)==="{true}");
          if((name==="textarea" || name==="input" && ["text","password","number","email","url","tel","search"].includes(value)) && attr("value") && !uneditable)
            missed.push(file.slice(ui.length+1)+":"+tree.getLineAndCharacterOfPosition(node.getStart()).line);
        }
        ts.forEachChild(node,visit);
      };visit(tree);
    }
    expect(missed).toEqual([]);
  });
});
