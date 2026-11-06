/** Synthetic cloud evidence only; never claims native authority or Windows acceptance. */
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { chromium } from '@playwright/test';
const output='artifacts/mp8f-visual/ui09';
const reference='docs/ui/evidence/mp-ui-09/reference.html';
assert.equal(createHash('sha256').update(await readFile(reference)).digest('hex'),'b28acf5d50c62bfbaecc6285c0621e9903c04fe73b5a10de6f22948cdcf47579');
await mkdir(output,{recursive:true});
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1421','--strictPort'],{stdio:'pipe'});
let browser;const rows=[];const errors=[];const findings=[];
try {
 for(let i=0;i<100;i++){try{if((await fetch('http://127.0.0.1:1421')).ok)break;}catch{}await new Promise(r=>setTimeout(r,100));}
 browser=await chromium.launch({headless:true});
 const page=await browser.newPage();const original=await browser.newPage();
 page.on('pageerror',e=>errors.push(e.message));
 await page.goto('http://127.0.0.1:1421',{waitUntil:'networkidle'});
 await original.goto('http://127.0.0.1:1421/'+reference,{waitUntil:'networkidle'});
 // Fixture data is deliberately synthetic; all unspecified commands fail visibly.
 await page.evaluate(async()=>{
  const {mountReader}=await import('/src/reader.ts');
  window.fixtureState={mode:'populated',locked:false};window.calls=[];window.copied=[];
  const summary={id:'a',conversation_id:'provider-a',provider:'chatgpt',title:'Synthetic project planning — evidence and a deliberately long title for narrow layouts',source_url:'https://example.invalid/conversation',captured_at:'2026-11-06T00:00:00Z',message_count:2,snapshot_count:2,completeness:'partial',warnings:['Synthetic partial capture']};
  const capture={schema_version:1,provider:'chatgpt',conversation_id:'provider-a',source_url:summary.source_url,title:summary.title,captured_at:summary.captured_at,capture_method:'manual-dom',completeness:'partial',warnings:summary.warnings,messages:[{id:'m',role:'user',text:'Synthetic evidence: inspect the source before using a claim.',format:'plain',attachments:[]},{id:'answer',role:'assistant',text:'## Traceable answer\nNo invented provider status. <script>unsafe()</script>',format:'markdown',attachments:[]}]};
  const ranked={claim:{id:'claim:a',statement:'Synthetic project preference requires source review',asserted_by:'user',epistemic_origin:'explicit',status:'active',revision:1,recorded_at_ms:1793923200000,evidence:[{source_ref:'src:v1:opaque',evidence_kind:'support'}]},disposition:'contested',score:100,related_claim_ids:[]};
  window.invokeFixture=async(command,args)=>{
   window.calls.push({command,args});const state=window.fixtureState;
   if(command==='get_security_status')return {configured:true,locked:state.locked,epoch:1,retry_after_ms:0,vault_mode:'legacy'};
   if(state.locked)throw Error('MP_SECURITY_LOCKED');
   if(state.mode==='error'&&['list_conversations','list_document_roots','list_memory_inspection','read_archive_hive','search_archive_index'].includes(command))throw Error('Synthetic service unavailable');
   if(command==='list_conversations'&&state.mode==='loading')return new Promise(()=>{});
   if(command==='list_conversations')return state.mode==='empty'?[]:Array.from({length:65},(_,i)=>({...summary,id:i?'a'+i:'a',title:i?`Synthetic chat ${i} — `+summary.title:summary.title,provider:i%2?'claude':'chatgpt'}));
   if(command==='read_conversation')return {summary:{...summary,id:args.id},snapshot_id:args.snapshotId||'snap-1',capture};
   if(command==='list_snapshots')return [{snapshot_id:'snap-1',captured_at:summary.captured_at,message_count:2,completeness:'partial',warnings:summary.warnings},{snapshot_id:'snap-0',captured_at:'2026-11-05T00:00:00Z',message_count:1,completeness:'complete',warnings:[]}];
   if(command==='list_memory_inspection')return state.mode==='empty'?[]:[ranked];
   if(command==='get_memory_inspection')return {ranked,history:[{revision:1,recorded_at_ms:1793923200000,event_type:'created',status:'active',reason:null,superseded_by:null,evidence:ranked.claim.evidence}],relations:[]};
   if(command==='read_archive_hive')return {nodes:state.mode==='empty'?[]:[{id:'a',kind:'import',label:'Synthetic saved conversation',canonical_key:'a',source_ref:'s',depth:0},{id:'b',kind:'conversation',label:'Follow-up evidence',canonical_key:'b',source_ref:'s',depth:1},{id:'c',kind:'revision',label:'Isolated evidence',canonical_key:'c',source_ref:'other',depth:0}],edges:state.mode==='empty'?[]:[{id:'ab',from_node:'a',to_node:'b',relation:'revision',origin:'inferred',confidence_bps:8100,source_ref:'s',source_revision:'snap-1',locator:'line:1'}],truncated:false};
   if(command==='list_document_roots')return state.mode==='empty'?[]:[{id:'r1',canonical_path:'C:\\Synthetic\\Approved Project',virtual_prefix:'mpdoc://root/r1',approval_state:'approved'}];
   if(command==='search_archive_index')return [{source_id:'s',archive_id:'a',snapshot_id:'snap-1',message_id:'m',kind:'body',role:'user',title:summary.title,snippet:'Synthetic evidence <img src=x onerror=unsafe()>',score:1}];
   if(command==='search_document_index')return [];
   if(command==='get_archive_index_stats'||command==='rebuild_archive_index')return {sources:65,revisions:66,chunks:130,generation:2};
   if(command==='get_local_mcp_status'||command==='get_hosted_tunnel_status')return {running:false,connection_verified:false};
   if(command==='list_local_mcp_clients')return [];
   if(command==='get_setup')return {platform:'Windows',archive_path:'C:\\Synthetic\\Archive',extension_path:'C:\\Synthetic\\Extension',host_path:'C:\\Synthetic\\Host',native_host_name:'com.mindplace.capture',extension_id:'synthetic',registered_browsers:[],legacy_available:false};
   if(command==='build_retrieval_context')return {groups:[{source_id:'s',archive_id:'a',snapshot_id:'snap-1',conversation_id:'provider-a',title:summary.title,captured_at:summary.captured_at,excerpts:[{kind:'body',message_id:'m',role:'user',text:'Synthetic exact evidence',byte_start:0,byte_end:24,total_bytes:24,score:1,matched_queries:[0]}]}],estimated_tokens:6,truncated:false};
   if(command==='inspect_retrieval')return {trace:{operation:'search',elapsed_us:150,index_generation:2,candidate_count:1,result_count:1,source_count:1,estimated_tokens:6,truncated:false},result:{hits:[]}};
   throw Error('Synthetic fixture unavailable: '+command);
  };
  window.mountFixture=async()=>{window.reader?.destroy();window.reader=mountReader({root:document.querySelector('#app'),invoke:window.invokeFixture,clipboard:{writeText:async t=>window.copied.push(t)},onLock:()=>{window.reader.destroy();document.querySelector('#app').textContent='Locked synthetic fixture';}});await window.reader.ready;};
  await window.mountFixture();
 });
 const routes=[['overview','home'],['conversations','conversations'],['curated-memory','memory'],['hive-mind-graph','hive'],['files-secure-vault','files'],['ai-providers-mcp','providers'],['setup-onboarding','setup'],['settings','settings']];
 const snap=async(name,ref,meta={})=>{
  await page.waitForTimeout(60);const integrated=name+'-integrated.png';await page.screenshot({path:output+'/'+integrated});
  const metrics=await page.evaluate(()=>({overflow:document.documentElement.scrollWidth>innerWidth+1,heading:document.querySelector('#reader-view h1, #reader-view .ui-state-panel h2, #reader-view .state-panel h2')?.textContent,theme:document.documentElement.dataset.theme}));
  if(metrics.overflow)findings.push({name,issue:'viewport overflow'});
  assert.ok(metrics.heading||name.includes('locked'),'meaningful heading '+name);
  let baseline=null;if(ref){await original.evaluate(p=>{if(p==='conversation-detail')openConversationDetail('Building the Mind Place archive');else if(p==='memory-detail')openMemoryDetail(allMemories()[0].id);else go(p);},ref);await original.waitForTimeout(60);baseline=name+'-original.png';await original.screenshot({path:output+'/'+baseline});}
  rows.push({name,integrated,original:baseline,...metrics,...meta});
 };
 for(const theme of ['light','dark'])for(const width of [1440,1120,780,390])for(const collapsed of [false,true]){
  await page.setViewportSize({width,height:900});await original.setViewportSize({width,height:900});
  await page.evaluate(({theme,collapsed})=>{const t=document.querySelector('[data-action=toggle-theme]');if(document.documentElement.dataset.theme!==theme)t.click();const s=document.querySelector('.app-shell');if((s.dataset.sidebarCollapsed==='true')!==collapsed)document.querySelector('[data-action=toggle-sidebar]').click();},{theme,collapsed});
  await original.evaluate(({theme,collapsed})=>{setTheme(theme);document.querySelector('#appShell').classList.toggle('sidebar-collapsed',collapsed);},{theme,collapsed});
  for(const [route,ref] of routes){await page.locator('#knowledge-os-nav [data-route="'+route+'"]').click();if(route==='hive-mind-graph'){await page.locator('.hive-workspace form button[type=submit]').click();}await snap(`${theme}-${width}-${collapsed?'collapsed':'expanded'}-${ref}`,ref,{width,collapsed,theme});}
 }
 // Key details and wizard states in each theme, at desktop and narrow widths.
 for(const theme of ['light','dark'])for(const width of [1440,390]){
  await page.setViewportSize({width,height:900});await original.setViewportSize({width,height:900});
  await page.evaluate(t=>{if(document.documentElement.dataset.theme!==t)document.querySelector('[data-action=toggle-theme]').click();},theme);await original.evaluate(t=>setTheme(t),theme);
  const nav=async route=>page.locator('#knowledge-os-nav [data-route="'+route+'"]').click();
  await nav('conversations');await page.locator('#reader-view [data-conversation-id]').first().click();await snap(`${theme}-${width}-transcript`,'conversation-detail');
  await nav('curated-memory');await page.locator('[data-memory-quick]').first().click();await snap(`${theme}-${width}-memory-quick`,null);await page.keyboard.press('Escape');await page.locator('[data-memory-claim]').first().click();await snap(`${theme}-${width}-memory-detail`,'memory-detail');
  await nav('files-secure-vault');await page.locator('[data-action=inspect-document-root]').first().click();await snap(`${theme}-${width}-folder-inspector`,'files');await page.locator('[data-action=authorize-document-folder]').click();await snap(`${theme}-${width}-folder-approval`,null);await page.keyboard.press('Escape');
  await nav('ai-providers-mcp');await page.locator('.providers-workspace [data-action=open-setup]').click();await snap(`${theme}-${width}-capture-setup`,null);await page.keyboard.press('Escape');
  await nav('setup-onboarding');for(const step of ['welcome','vault','providers','first-result']){await page.locator('button[data-step="'+step+'"]').click();await original.evaluate(n=>{setupStep=n;renderSetup();},['welcome','vault','providers','first-result'].indexOf(step));await snap(`${theme}-${width}-onboarding-${step}`,'setup');}
  await nav('settings');for(const section of ['general','storage-vault','privacy','diagnostics']){await page.locator('[data-settings-section="'+section+'"]').click();await snap(`${theme}-${width}-settings-${section}`,'settings');}
  // Use global palette, proving the search route is reachable without synthetic navigation.
  await page.locator('[data-action=open-command-palette]').click();await page.locator('#command-palette input').fill('Search');await snap(`${theme}-${width}-palette`,null);await page.locator('[data-command-id="search-all-sources"]').click();
  await page.locator('#library-query').fill('evidence');await page.locator('.search-query-form button[type=submit]').click();await page.waitForTimeout(80);await snap(`${theme}-${width}-search-results`,null);
  await page.locator('[data-action=build-context]').click();await snap(`${theme}-${width}-context`,null);await page.locator('#search-context-drawer [data-action=copy-context]').click();await page.locator('#search-context-drawer [data-action=close-ui-dialog]').click();
  await page.locator('[data-action=inspect-trace]').click();await snap(`${theme}-${width}-trace`,null);await page.keyboard.press('Escape');
 }
 for(const theme of ['light','dark'])for(const width of [1440,390]) {
  await page.setViewportSize({width,height:900});
  for(const mode of ['empty','error']){
   await page.evaluate(async m=>{window.fixtureState.mode=m;await window.mountFixture();},mode);
   await page.evaluate(t=>{if(document.documentElement.dataset.theme!==t)document.querySelector('[data-action=toggle-theme]').click();},theme);
   for(const [route,ref]of routes){await page.locator('#knowledge-os-nav [data-route="'+route+'"]').click();await snap(`${theme}-${width}-${mode}-${ref}`,null,{mode});}
  }
  await page.evaluate(()=>{window.fixtureState.mode='loading';void window.mountFixture();});await snap(`${theme}-${width}-loading-overview`,null,{mode:'loading'});
  await page.evaluate(async()=>{window.reader.destroy();window.fixtureState.mode='populated';window.fixtureState.locked=true;const {mountApplication}=await import('/src/app.ts');await mountApplication({root:document.querySelector('#app'),invoke:window.invokeFixture});});
  await snap(`${theme}-${width}-locked`,null,{mode:'locked'});
  await page.evaluate(async()=>{window.fixtureState.locked=false;await window.mountFixture();});
 }
 assert.deepEqual(errors,[],'browser runtime errors');
} catch(e){await writeFile(output+'/failure.txt',String(e.stack));throw e;}
finally{
 const receipt={platform:'synthetic cloud Chromium; NOT native Windows',head:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),tree:execFileSync('git',['rev-parse','HEAD^{tree}'],{encoding:'utf8'}).trim(),reference_sha256:'b28acf5d50c62bfbaecc6285c0621e9903c04fe73b5a10de6f22948cdcf47579',screenshots:rows,errors,findings};
 await writeFile(output+'/manifest.json',JSON.stringify(receipt,null,2));
 const gallery=rows.map(r=>`<article><h2>${r.name}</h2><div>${r.original?`<figure><figcaption>Original prototype (demo)</figcaption><img src="${r.original}"></figure>`:''}<figure><figcaption>Integrated synthetic candidate</figcaption><img src="${r.integrated}"></figure></div></article>`).join('');
 await writeFile(output+'/gallery.html','<!doctype html><title>MP-UI-09 A/B evidence</title><style>body{font:14px system-ui;background:#eee;color:#111}article>div{display:flex;gap:8px}figure{margin:0;flex:1;min-width:0}img{width:100%;border:1px solid #888}h2{font-size:16px}</style><h1>Original versus integrated candidate</h1><p>Synthetic cloud evidence only. Original contains demo content; candidate uses explicit synthetic fixtures. No native, security or owner acceptance.</p>'+gallery);
 await browser?.close();server.kill();console.log(JSON.stringify({screenshots:rows.length,findings,errors}));
}
