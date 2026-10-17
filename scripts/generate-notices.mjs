import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const product=JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8'));
const metadata=spawnSync('cargo',['metadata','--format-version','1','--filter-platform',process.platform==='win32'?'x86_64-pc-windows-msvc':process.platform==='darwin'?'aarch64-apple-darwin':'x86_64-unknown-linux-gnu'],{cwd:root,encoding:'utf8',maxBuffer:32*1024*1024});
if(metadata.status!==0)throw new Error(metadata.stderr||'Cargo metadata failed');
const packages=JSON.parse(metadata.stdout).packages.filter(p=>p.source);
const sections=[`Mind Place ${product.version} — third-party source and license notices\n\nRust dependencies include locked product/build dependencies for the target platform. Frontend notices include the production dependency closure. Development-only tooling is not distributed.`];
function licenseFiles(dir,depth=0){
 const out=[];
 for(const entry of fs.readdirSync(dir,{withFileTypes:true})){
  const file=path.join(dir,entry.name);
  if(entry.isFile()&&/^(licen[cs]e|copying|notice|copyright|ofl|authors)([._-]|$)/i.test(entry.name))out.push(file);
  else if(entry.isDirectory()&&depth<2&&!['node_modules','target','.git'].includes(entry.name))out.push(...licenseFiles(file,depth+1));
 }
 return out;
}
function add(name,version,license,dir){
 sections.push(`\n${'='.repeat(72)}\n${name} ${version}\nDeclared license: ${license||'See source license'}\n`);
 const files=licenseFiles(dir);
 for(const file of files){if(fs.statSync(file).size>1024*1024)throw new Error(`License file too large: ${name}`);sections.push(`--- ${path.relative(dir,file).replaceAll('\\','/')} ---\n${fs.readFileSync(file,'utf8')}`);}
 if(!files.length)sections.push(`License declaration: ${license||'Not declared'}; package source: ${name}`);
}
for(const p of packages.sort((a,b)=>a.name.localeCompare(b.name)||a.version.localeCompare(b.version)))add(p.name,p.version,p.license,path.dirname(p.manifest_path));
const seen=new Set();
function nodePackage(name,from){
 let parent=from;let dir;
 while(parent!==path.dirname(parent)){
  const candidate=path.join(parent,'node_modules',name);
  if(fs.existsSync(path.join(candidate,'package.json'))){dir=candidate;break;}
  parent=path.dirname(parent);
 }
 if(!dir)throw new Error(`Missing production package ${name}`);
 if(seen.has(dir))return;seen.add(dir);
 const pkg=JSON.parse(fs.readFileSync(path.join(dir,'package.json'),'utf8'));
 add(pkg.name,pkg.version,pkg.license,dir);
 for(const dep of Object.keys(pkg.dependencies||{}))nodePackage(dep,dir);
}
for(const dep of Object.keys(product.dependencies))nodePackage(dep,root);
fs.writeFileSync(path.join(root,'THIRD-PARTY-NOTICES.txt'),sections.join('\n')+'\n');
console.log(`Notices: ${packages.length} Rust packages, ${seen.size} frontend packages; ${fs.statSync(path.join(root,'THIRD-PARTY-NOTICES.txt')).size} bytes`);
