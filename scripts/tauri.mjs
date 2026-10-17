import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
function run(command,args){return new Promise((resolve,reject)=>{
 const child=spawn(command,args,{cwd:root,stdio:'inherit',windowsHide:true});
 child.on('error',reject);child.on('exit',(code,signal)=>code===0?resolve():reject(new Error(`${command} exited ${code??signal}`)));
});}

export function stageProduct(source,output,extension,notices,license){
 fs.mkdirSync(output,{recursive:true});
 const exe=process.platform==='win32'?'.exe':'';
 for(const name of [`mind-place${exe}`,`mind-place-capture-host${exe}`]){
  const file=path.join(source,name);if(!fs.statSync(file).isFile())throw new Error(`Missing product ${name}`);
  fs.copyFileSync(file,path.join(output,name));
 }
 fs.cpSync(extension,path.join(output,'extension'),{recursive:true,filter:source=>!fs.lstatSync(source).isSymbolicLink()});
 fs.copyFileSync(notices,path.join(output,'THIRD-PARTY-NOTICES.txt'));
 fs.copyFileSync(license,path.join(output,'LICENSE'));
 const manifest=JSON.parse(fs.readFileSync(path.join(output,'extension/manifest.json'),'utf8'));
 if(manifest.content_scripts?.length||manifest.host_permissions?.length)throw new Error('Capture package cannot contain unconditional page scripts or host access');
 if(JSON.stringify(manifest.optional_host_permissions)!==JSON.stringify(['https://chatgpt.com/*']))throw new Error('Only optional ChatGPT origin access may be packaged');
 if(manifest.background?.service_worker!=='worker.mjs'||manifest.background?.type!=='module'||Object.keys(manifest.background).length!==2)throw new Error('Only the explicit manual-capture worker can be packaged');
 if(JSON.stringify(manifest.permissions)!==JSON.stringify(['activeTab','scripting','nativeMessaging','storage']))throw new Error('Unexpected capture permissions');
 const files=[];
 function walk(dir){for(const entry of fs.readdirSync(dir,{withFileTypes:true})){const file=path.join(dir,entry.name);if(entry.isDirectory())walk(file);else if(entry.isFile())files.push({path:path.relative(output,file).replaceAll('\\','/'),bytes:fs.statSync(file).size});else throw new Error('Unexpected package link or special file');}}
 walk(output);
 if(files.some(f=>/app\.asar|cloudflared|tunnel-client|node_modules|node-pty|koffi|tree-sitter|pets\//i.test(f.path)))throw new Error('Unexpected legacy payload found in reader package');
 const receipt={date:new Date().toISOString(),platform:process.platform,arch:process.arch,bytes:files.reduce((n,f)=>n+f.bytes,0),files};
 fs.mkdirSync(path.join(root,'outputs'),{recursive:true});fs.writeFileSync(path.join(root,'outputs/lean-package-footprint.json'),JSON.stringify(receipt,null,2)+'\n');
 console.log(`Reader payload: ${(receipt.bytes/1e6).toFixed(2)} MB (${(receipt.bytes/1048576).toFixed(2)} MiB); ${files.length} files`);
 return receipt;
}

async function main(){
 const [operation,...args]=process.argv.slice(2);
 if(!['dev','build'].includes(operation))throw new Error('Usage: node scripts/tauri.mjs dev|build [Tauri arguments]');
 if(args.some(a=>a==='--target'||a.startsWith('--target=')))throw new Error('This wrapper builds the native host architecture; cross compilation is not configured');
 const version=spawnSync('rustc',['-vV'],{encoding:'utf8',windowsHide:true});if(version.status!==0)throw new Error('Rust compiler is unavailable');
 const triple=version.stdout.match(/^host:\s*(.+)$/m)?.[1].trim();if(!triple)throw new Error('Cannot determine Rust target');
 const profile=operation==='build'?'release':'debug';
 await run('cargo',['build','-p','mind-place-capture-host',...(profile==='release'?['--release']:[])]);
 const exe=process.platform==='win32'?'.exe':'';
 const source=path.join(root,'target',profile);fs.mkdirSync(path.join(root,'src-tauri/binaries'),{recursive:true});
 fs.copyFileSync(path.join(source,`mind-place-capture-host${exe}`),path.join(root,'src-tauri/binaries',`mind-place-capture-host-${triple}${exe}`));
 await run(process.execPath,[path.join(root,'scripts/generate-notices.mjs')]);
 await run(process.execPath,[path.join(root,'node_modules/@tauri-apps/cli/tauri.js'),operation,...args]);
 if(operation==='build')stageProduct(source,path.join(root,'release',`${process.platform}-${process.arch}`),path.join(root,'extension'),path.join(root,'THIRD-PARTY-NOTICES.txt'),path.join(root,'LICENSE'));
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))main().catch(error=>{console.error(error.message);process.exitCode=1;});
