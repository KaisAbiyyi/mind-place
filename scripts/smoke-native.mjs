import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { JSDOM } from 'jsdom';
import { captureConversation } from '../extension/capture.mjs';

const project=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const output=path.join(project,'outputs');fs.mkdirSync(output,{recursive:true});
const archive=fs.mkdtempSync(path.join(output,'native-smoke-'));
const host=path.resolve(process.argv[2]||path.join(project,`release/${process.platform}-${process.arch}/mind-place-capture-host${process.platform==='win32'?'.exe':''}`));
const origin='chrome-extension://dninfeidggaaioioficnigfpfjkdicjh/';
function roundtrip(request,archiveRoot=archive){
 const body=Buffer.from(JSON.stringify(request));const prefix=Buffer.alloc(4);prefix.writeUInt32LE(body.length);
 const result=spawnSync(host,[origin],{input:Buffer.concat([prefix,body]),env:{...process.env,MIND_PLACE_DATA_DIR:archiveRoot},windowsHide:true,timeout:20000,maxBuffer:2*1024*1024});
 if(result.status!==0)throw new Error(`Host process failed: ${result.error?.message||result.stderr.toString()}`);
 if(result.stdout.length<4||result.stdout.readUInt32LE(0)!==result.stdout.length-4)throw new Error('Invalid stdout native framing');
 return JSON.parse(result.stdout.subarray(4).toString());
}
function capture(id,title,answer){
 const fixture=new JSDOM(`<!doctype html><html><head><title>${title} - ChatGPT</title></head><body><main><article data-testid="conversation-turn-1"><div data-message-id="user-1" data-message-author-role="user"><p>Keep our architecture decision.</p></div></article><article data-testid="conversation-turn-2"><div data-message-id="assistant-1" data-message-author-role="assistant"><div class="markdown"><h2>Decision</h2><p>${answer}</p><pre><code class="language-rust">fn main() { println!("local archive"); }</code></pre><p>Equation: $E = mc^2$</p><a href="https://example.com/source">Source reference</a><img alt="Offline diagram" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN3sAAAAASUVORK5CYII="></div></div></article></main></body></html>`,{url:`https://chatgpt.com/c/${id}`,runScripts:'outside-only'});
 fixture.window.TextEncoder=TextEncoder;const result=fixture.window.eval(`(${captureConversation.toString()})()`);
 fixture.window.close();if(!result?.ok)throw new Error(result?.error||'Production DOM capture failed');return result.capture;
}
const first=capture('12345678-1234-4123-8123-123456789abc','Rust archive decision','Use Rust and Tauri. Keep the raw JSONL source locally.');
const saved=roundtrip({protocol:1,operation:'save',capture:first});if(!saved.ok||saved.message_count!==2)throw new Error('Initial save not acknowledged');
const duplicate=roundtrip({protocol:1,operation:'save',capture:first});if(!duplicate.unchanged)throw new Error('Identical save was not deduplicated');
const changed=capture('12345678-1234-4123-8123-123456789abc','Rust archive decision','Use Rust and Tauri. Manual capture is now the chosen policy.');
const revised=roundtrip({protocol:1,operation:'save',capture:changed});if(!revised.ok||revised.snapshot_id===saved.snapshot_id)throw new Error('Changed snapshot was not retained');
const second=capture('87654321-4321-4321-8321-abcdef012345','Archive reading notes','Search and export should work offline.');
const other=roundtrip({protocol:1,operation:'save',capture:second});if(!other.ok||other.id===saved.id)throw new Error('Conversation identities collided');
const avifDom=new JSDOM('<main><div data-message-id="avif-1" data-message-author-role="assistant"><div class="markdown"><p>Keep the text despite unsupported media.</p><img alt="AVIF source" src="data:image/avif;base64,AAAAIGZ0eXBhdmlm"></div></div></main>',{url:'https://chatgpt.com/c/12345678-1234-4123-8123-123456789abc',runScripts:'outside-only'});
avifDom.window.TextEncoder=TextEncoder;
const avif=avifDom.window.eval(`(${captureConversation.toString()})()`);avifDom.window.close();
if(!avif.ok||avif.capture.messages[0].attachments[0].data_url||!avif.capture.messages[0].attachments[0].unavailable_reason)throw new Error('Unsupported inline media was not made explicit');
const avifSaved=roundtrip({protocol:1,operation:'save',capture:avif.capture},path.join(archive,'avif-regression'));
if(!avifSaved.ok)throw new Error(`Unsupported-media fallback rejected the text: ${avifSaved.error}`);
const raw=JSON.parse(fs.readFileSync(path.join(archive,'conversations',saved.id,'captures',`${saved.snapshot_id}.jsonl`),'utf8'));
if(!raw.messages[1].text.includes('Keep the raw JSONL'))throw new Error('Prior raw snapshot was overwritten');
const receipt={kind:'production extension DOM function on fixture -> packaged native host process -> isolated durable archive',host,archive_path:archive,initial:saved,duplicate,revised,second:other,avif_fallback:avifSaved.ok,raw_reopen:true,live_chatgpt:false};
fs.writeFileSync(path.join(output,'native-process-smoke.json'),JSON.stringify(receipt,null,2)+'\n');
console.log(JSON.stringify({archive_path:archive,conversations:2,snapshots:3,dedup:true,raw_reopen:true,avif_fallback:true,live_chatgpt:false}));
