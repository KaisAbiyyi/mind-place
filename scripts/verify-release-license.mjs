import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const portable = path.join(root, 'release', `${process.platform}-${process.arch}`);
const expected = fs.readFileSync(path.join(root, 'LICENSE'));
const actual = fs.readFileSync(path.join(portable, 'LICENSE'));
if (!expected.equals(actual) || !actual.toString('utf8').startsWith('MIT License\n')) {
  throw new Error('Portable first-party LICENSE does not match the MIT source LICENSE');
}
const notices = fs.readFileSync(path.join(portable, 'THIRD-PARTY-NOTICES.txt'), 'utf8');
if (!notices.includes('third-party source and license notices') || !notices.includes('Declared license:')) {
  throw new Error('Portable third-party notice inventory is missing or incomplete');
}
if (process.platform === 'win32') {
  for (const name of ['tunnel-client.exe', 'tunnel/receipt.json']) {
    if (!fs.statSync(path.join(portable, name)).isFile()) throw new Error(`Missing bundled tunnel helper provenance: ${name}`);
  }
  if (!notices.includes('OpenAI tunnel-client') || !fs.readdirSync(path.join(portable, 'tunnel')).some(name => /license|notice/i.test(name))) {
    throw new Error('Missing tunnel-helper third-party license notices');
  }
}
console.log('Portable MIT license and third-party notice resources OK');
