import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { mountEntityLenses } from "../src/memory-entity-lenses.js";
import type { Invoke } from "../src/reader.js";
const claim = { claim:{id:"claim1",statement:"Project <img src=x>",asserted_by:"author",epistemic_origin:"user",status:"active",revision:1,recorded_at_ms:1,evidence:[]},disposition:"unverified",score:1,related_claim_ids:[] };
function fixture(invoke: Invoke) {
 const dom = new JSDOM("<!doctype html><html><body><main></main></body></html>");
 const view = dom.window.document.querySelector("main")!;
 const handle = mountEntityLenses(view,invoke);
 function search(q:string) {
   view.querySelector<HTMLInputElement>("#entity-lens-name")!.value=q;
   view.querySelector("form")!.dispatchEvent(new dom.window.Event("submit",{bubbles:true,cancelable:true}));
 }
 return {view,handle,search};
}
test("named entity lens retains hypothesis semantics and renders timeline/source pointers as inert text",async()=>{
 const invoke:Invoke=async <T>(command,args)=>{
  if(command==="list_memory_inspection") { assert.deepEqual(args,{query:"P"}); return [claim] as T; }
  if(command==="get_memory_inspection") { assert.deepEqual(args,{claimId:"claim1"});return {ranked:claim,history:[{revision:1,recorded_at_ms:1000,event_type:"created",status:"active",reason:null,superseded_by:null,evidence:[{source_ref:"raw-1",evidence_kind:"message"}]}],relations:[]} as T; }
  throw Error(command);
 };
 const {view,handle,search}=fixture(invoke);
 search("P");await delay(0);
 assert.equal(view.querySelector("img"),null);
 assert.match(view.textContent!,/NOT a canonical entity registry/);
 view.querySelector<HTMLButtonElement>("[data-entity-claim]")!.click();await delay(0);
 assert.match(view.querySelector(".entity-lens-detail")!.textContent!,/raw-1/);
 handle.destroy();
});
test("changing entity lens invalidates outstanding search response",async()=>{
 let finish!: (v:any)=>void;
 const pending=new Promise<any>(r=>finish=r);
 const invoke:Invoke=async <T>(command: string)=>command==="list_memory_inspection"?pending as T:null as T;
 const {view,handle,search}=fixture(invoke);
 search("secret");
 view.querySelector<HTMLButtonElement>('[data-entity-lens="people"]')!.click();
 finish([claim]);await delay(0);
 assert.equal(view.querySelectorAll("[data-entity-claim]").length,0);
 handle.destroy();
});
test("destroy clears sensitive input and prevents late detail render",async()=>{
 let finish!: (v:any)=>void;
 const pending=new Promise<any>(r=>finish=r);
 const invoke:Invoke=async <T>(command: string)=>command==="list_memory_inspection"?[claim] as T:pending as T;
 const {view,handle,search}=fixture(invoke);
 search("private");await delay(0);
 view.querySelector<HTMLButtonElement>("[data-entity-claim]")!.click();
 handle.destroy();finish({ranked:claim,history:[],relations:[]});await delay(0);
 assert.equal(view.textContent,"");
});
