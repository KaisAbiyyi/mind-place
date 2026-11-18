// Regenerates the committed parser-compatibility fixtures:
//   node test/fixtures/import-sanitizer/generate-parser-compat.mjs
// The "synthetic" files are invented data; the "sanitized" files are the sanitizer's output for them.
// A test fails if the committed sanitized files drift from what the tool produces.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { sanitize } from '../../../scripts/import-sanitizer/sanitize.mjs';
import { writeChatGptExport, writeGeminiTakeout } from './synthetic-exports.mjs';

export const FIXTURE_DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), 'parser-compat');

export async function buildParserCompatFixtures(targetDir) {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'mp-compat-'));
  try {
    writeChatGptExport(path.join(work, 'chatgpt'), { seed: 31, conversations: 6, multi: false });
    writeGeminiTakeout(path.join(work, 'gemini'), { seed: 32, cards: 6 });
    fs.mkdirSync(targetDir, { recursive: true });
    const files = {
      'chatgpt-synthetic.json': path.join(work, 'chatgpt', 'conversations.json'),
      'gemini-synthetic.html': path.join(work, 'gemini', 'Takeout', 'My Activity', 'Gemini Apps', 'MyActivity.html'),
    };
    for (const [name, source] of Object.entries(files)) fs.copyFileSync(source, path.join(targetDir, name));
    for (const [from, to] of [['chatgpt-synthetic.json', 'chatgpt-sanitized.json'], ['gemini-synthetic.html', 'gemini-sanitized.html']]) {
      const output = path.join(work, to);
      // A fixed key keeps the committed fixtures reproducible; real runs default to a random key.
      await sanitize({ input: path.join(targetDir, from), output, force: true, salt: 'parser-compat-fixture-key' });
      fs.copyFileSync(output, path.join(targetDir, to));
    }
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await buildParserCompatFixtures(FIXTURE_DIR);
  process.stdout.write(`Wrote fixtures to ${FIXTURE_DIR}\n`);
}
