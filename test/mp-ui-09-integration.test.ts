import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { JSDOM } from "jsdom";
import { mountSearchWorkspace } from "../src/search-workspace.js";
import { mountSettingsWorkspace } from "../src/settings-workspace.js";
import { mountReader, type Invoke } from "../src/reader.js";
const sec = { configured:true, locked:false, epoch:1, vault_mode:"legacy" };
const hit = {source_id:"s",archive_id:"a",snapshot_id:"v",message_id:"m",kind:"body",role:"user",title:"Current",snippet:"Evidence",score:1};
function fixture() { const dom=new JSDOM('<main></main>',{url:'https://integration.test'});return {dom,view:dom.window.document.querySelector('main')!}; }
function submit(dom:JSDOM, view:HTMLElement, q:string) { view.querySelector<HTMLInputElement>('#library-query')!.value=q;view.querySelector('form')!.dispatchEvent(new dom.window.Event('submit',{bubbles:true,cancelable:true})); }
test('UI09 search refuses missing security authority before reading any index',async()=>{
 const {dom,view}=fixture();const calls:string[]=[];
 const invoke:Invoke=async<T>(c:string)=>{calls.push(c);if(c==='get_security_status')throw Error('offline');return [hit] as T;};
 const h=mountSearchWorkspace(view,invoke);submit(dom,view,'private');await delay(0);
 assert.deepEqual(calls,['get_security_status']);assert.equal(view.querySelector('[data-library-archive]'),null);h.destroy();dom.window.close();
});
test('UI09 search discards older query completions and destroyed state',async()=>{
 const {dom,view}=fixture();let resolve!:(x:any)=>void;const old=new Promise(r=>resolve=r);
 const invoke:Invoke=async<T>(c:string,a?:Record<string,unknown>)=>{if(c==='get_security_status')return sec as T;if(c==='search_archive_index')return (a?.query==='old'?old:[hit]) as T;return [] as T;};
 const h=mountSearchWorkspace(view,invoke);submit(dom,view,'old');await delay(0);submit(dom,view,'new');await delay(0);resolve([{...hit,title:'STALE PRIVATE'}]);await delay(0);
 assert.doesNotMatch(view.textContent!,/STALE PRIVATE/);h.destroy();assert.equal((h.state().data as any).archiveHits.length,0);dom.window.close();
});
test('UI09 context request matches native strict contract and displays actual estimated tokens',async()=>{
 const {dom,view}=fixture();let request:any;
 const invoke:Invoke=async<T>(c:string,a?:Record<string,unknown>)=>{if(c==='get_security_status')return sec as T;if(c==='search_archive_index')return [hit] as T;if(c==='build_retrieval_context'){request=a?.request;return {groups:[],estimated_tokens:37,truncated:false} as T;}return [] as T;};
 const h=mountSearchWorkspace(view,invoke);submit(dom,view,'query');await delay(0);view.querySelector<HTMLButtonElement>('[data-action=build-context]')!.click();await delay(0);
 assert.deepEqual(request.budget,{max_estimated_tokens:4000});assert.equal(request.preset,'balanced');assert.doesNotMatch(view.textContent!,/undefined tokens/);assert.match(view.textContent!,/37 tokens/);h.destroy();dom.window.close();
});
test('UI09 storage delegates recovery to locked gate without collecting credentials in reader',async()=>{
 const {dom,view}=fixture();const invoke:Invoke=async<T>(c:string)=>c==='get_security_status'?sec as T:{} as T;
 const h=mountSettingsWorkspace(view,invoke,{initialSection:'storage-vault'});await delay(0);
 assert.equal(view.querySelector('input[type=password]'),null);assert.match(view.textContent!,/legacy|not encrypted/i);h.destroy();dom.window.close();
});
test('UI09 canceled confirmation cannot retain a pending rebuild callback',async()=>{
 const {dom,view}=fixture();let rebuilds=0;
 const invoke:Invoke=async<T>(c:string)=>{if(c==='get_security_status')return sec as T;if(c==='get_archive_index_stats')return {sources:0,revisions:0,chunks:0,generation:0} as T;if(c==='rebuild_archive_index'){rebuilds++;return {generation:1,chunks:0} as T;}return {running:false} as T;};
 const h=mountSettingsWorkspace(view,invoke,{initialSection:'diagnostics'});await delay(0);
 const rebuild=()=>[...view.querySelectorAll<HTMLButtonElement>('button')].find(b=>b.textContent==='Rebuild Archive Index')!.click();
 rebuild();view.querySelector<HTMLButtonElement>('.settings-confirm-actions [data-action=close-ui-dialog]')!.click();rebuild();view.querySelector<HTMLButtonElement>('.settings-confirm-actions .ui-button-primary')!.click();await delay(0);assert.equal(rebuilds,1);h.destroy();dom.window.close();
});
test('UI09 transcript completion cannot replace Settings after navigation',async()=>{
 const {dom,view}=fixture();let resolve!:(x:any)=>void;const pending=new Promise(r=>resolve=r);
 const summary={id:'a',provider:'chatgpt',conversation_id:'a',title:'A',captured_at:'2026-11-06',message_count:1,snapshot_count:1,completeness:'complete',warnings:[]};
 const invoke:Invoke=async<T>(c:string)=>{if(c==='list_conversations')return [summary] as T;if(c==='read_conversation')return pending as T;if(c==='list_snapshots')return [] as T;if(c==='get_security_status')return sec as T;return {} as T;};
 const h=mountReader({root:view,invoke});await h.ready;view.querySelector<HTMLButtonElement>('[data-route=conversations]')!.click();view.querySelector<HTMLButtonElement>('#reader-view [data-conversation-id]')!.click();view.querySelector<HTMLButtonElement>('[data-route=settings]')!.click();resolve({summary,capture:{messages:[]}});await delay(0);assert.ok(view.querySelector('.settings-workspace'));h.destroy();dom.window.close();
});
