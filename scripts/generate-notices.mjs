import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { transitiveLicenseNotices, hasMplLicense, registrySourceArchive } from './notice-coverage.mjs';
import { helperPin, helperDirectory, readVerifiedHelperBundle, readVerifiedRegularFile } from './tunnel-helper.mjs';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const product=JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8'));
const targetTriple=process.platform==='win32'?'x86_64-pc-windows-msvc':process.platform==='darwin'?'aarch64-apple-darwin':'x86_64-unknown-linux-gnu';
const metadata=spawnSync('cargo',['metadata','--locked','--format-version','1','--filter-platform',targetTriple],{cwd:root,encoding:'utf8',maxBuffer:32*1024*1024});
if(metadata.status!==0)throw new Error(metadata.stderr||'Cargo metadata failed');
const tree=spawnSync('cargo',['tree','--locked','--workspace','--edges','normal,build','--target',targetTriple,'--prefix','none'],{cwd:root,encoding:'utf8',maxBuffer:32*1024*1024});
if(tree.status!==0)throw new Error(tree.stderr||'Cargo tree failed');
const activeCrates=new Set();
for(const line of tree.stdout.split(/\r?\n/)){
 const match=/^(\S+)\s+v(\S+)/.exec(line.trim());
 if(match)activeCrates.add(`${match[1]}@${match[2]}`);
}
const packages=JSON.parse(metadata.stdout).packages.filter(p=>p.source&&activeCrates.has(`${p.name}@${p.version}`));
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
function add(name,version,license,dir,supplements=[],extraSourceUrls=[]){
 sections.push(`\n${'='.repeat(72)}\n${name} ${version}\nDeclared license: ${license||'See source license'}\n`);
 const sourceUrls=new Set([...supplements.map(s=>s.sourceUrl).filter(Boolean),...extraSourceUrls]);
 for (const sourceUrl of sourceUrls) sections.push('Source archive: ' + sourceUrl);
 const files=licenseFiles(dir);
 for(const file of files){const body=readVerifiedRegularFile(file,{maxBytes:1024*1024,sizeTypeError:`License file too large: ${name}`}).toString('utf8');sections.push(`--- ${path.relative(dir,file).replaceAll('\\','/')} ---\n${body}`);}
 for(const supplement of supplements)sections.push(`--- ${supplement.name} ---\n${supplement.text}`);
 if(!files.length&&!supplements.length)sections.push(`License declaration: ${license||'Not declared'}; package source: ${name}`);
}
for (const p of packages.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))) {
 const isRegistry=p.source.startsWith('registry+');
 const supplements = isRegistry ? transitiveLicenseNotices(p.name, p.version) : [];
 const mplSources = isRegistry && hasMplLicense(p.license) ? [registrySourceArchive(p.name, p.version)] : [];
 add(p.name, p.version, p.license, path.dirname(p.manifest_path), supplements, mplSources);
}
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
 const mplSources = hasMplLicense(pkg.license) ? [`https://registry.npmjs.org/${pkg.name}/-/${pkg.name.split('/').pop()}-${encodeURIComponent(pkg.version)}.tgz`] : [];
 add(pkg.name,pkg.version,pkg.license,dir,[],mplSources);
 for(const dep of Object.keys(pkg.dependencies||{}))nodePackage(dep,dir);
}
for(const dep of Object.keys(product.dependencies))nodePackage(dep,root);
if(process.platform==='win32'){
 const bundle=readVerifiedHelperBundle(helperDirectory);
 sections.push(`\n${'='.repeat(72)}\nOpenAI tunnel-client ${helperPin.version} (Windows x64)\nSource and release: ${helperPin.repository}/releases/tag/v${helperPin.version}\nPinned executable SHA-256: ${helperPin.executable.sha256}\nBundled notices, upstream release SBOM and exact included-file receipt: tunnel/\nOnly tunnel-client.exe is distributed; the upstream cloudflared companion is excluded.\n`);
 for(const name of Object.keys(helperPin.resources).filter(name=>!name.endsWith('.json'))){const notice=bundle.resources.get(name).toString('utf8');sections.push(`--- tunnel/${name} ---\n${notice}`);}
}
const output=sections.join('\n')+'\n';
fs.writeFileSync(path.join(root,'THIRD-PARTY-NOTICES.txt'),output);
console.log(`Notices: ${packages.length} Rust packages, ${seen.size} frontend packages; ${Buffer.byteLength(output)} bytes`);
