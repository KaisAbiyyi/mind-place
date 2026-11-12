// MP-IMPORT-SANITIZE-01: structure-preserving sanitizer for ChatGPT / Gemini exports.
// All data is synthetic (see test/fixtures/import-sanitizer/synthetic-exports.mjs).
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { Readable } from 'node:stream';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { sanitize } from '../scripts/import-sanitizer/sanitize.mjs';
import { DEFAULT_BUDGETS, totalUpperBoundBytes } from '../scripts/import-sanitizer/budgets.mjs';
import { DistinctCounter, LeakRegistry } from '../scripts/import-sanitizer/leak-check.mjs';
import { rewriteHtml, scanHtml } from '../scripts/import-sanitizer/html-stream.mjs';
import { rewriteJson } from '../scripts/import-sanitizer/json-stream.mjs';
import { lengthClass, neutralizeText, Rules } from '../scripts/import-sanitizer/rules.mjs';
import { openZip, ZipWriter } from '../scripts/import-sanitizer/zip.mjs';
import { readTree, writeChatGptExport, writeGeminiTakeout, zipDirectory } from './fixtures/import-sanitizer/synthetic-exports.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const cli = path.join(root, 'scripts', 'import-sanitizer', 'cli.mjs');

function scratch(name) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `mp-sanitizer-${name}-`));
  return { dir, done: () => fs.rmSync(dir, { recursive: true, force: true }) };
}

async function collect(generator) {
  let out = '';
  for await (const chunk of generator) out += chunk;
  return out;
}

const bytesIn = (text, size = 1 << 16) => Readable.from((function* () {
  const buffer = Buffer.from(text);
  for (let i = 0; i < buffer.length; i += size) yield buffer.subarray(i, i + size);
})());

function outputText(dir) {
  return [...readTree(dir)].map(([name, bytes]) => `${name}\n${bytes.toString('latin1')}`).join('\n');
}

function jsonDocuments(dir) {
  return [...readTree(dir)].filter(([name]) => name.endsWith('.json')).map(([name, bytes]) => [name, JSON.parse(bytes.toString('utf8'))]);
}

// ------------------------------------------------------------------ rules

test('length classes bound output size and keep the class of the input', () => {
  assert.equal(lengthClass(0), 0);
  assert.equal(lengthClass(5), 4);
  assert.equal(lengthClass(300), 256);
  for (const n of [1, 2, 3, 7, 9, 100, 4097, 99_999]) assert.ok(lengthClass(n) <= n, 'placeholders never grow');
  assert.equal(lengthClass(5_000_000), 16384);
  const filler = neutralizeText('x'.repeat(100_000));
  assert.equal(filler.length, 16384);
  assert.match(filler, /^[a-z ]+$/);
});

test('text neutralization keeps lines and markdown structure but no words', () => {
  const input = '# Heading about Zanzibar\n- first item with secrets\n\n```python\nprint("hello zanzibar")\n```\r\nplain line';
  const output = neutralizeText(input);
  assert.equal(output.split('\n').length, input.split('\n').length);
  const lines = output.split('\n');
  assert.ok(lines[0].startsWith('# '));
  assert.ok(lines[1].startsWith('- '));
  assert.equal(lines[2], '');
  assert.ok(lines[3].startsWith('```'));
  assert.ok(lines[5].startsWith('```') && lines[5].endsWith('\r'));
  assert.doesNotMatch(output, /Zanzibar|secrets|hello|print/i);
});

test('identifier pseudonyms are deterministic, shape-preserving and injective', () => {
  const rules = new Rules();
  const id = '12345678-1234-4a23-b123-123456789abc';
  const a = rules.pseudonymize(id);
  assert.equal(a, rules.pseudonymize(id));
  assert.match(a, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-b[0-9a-f]{3}-[0-9a-f]{12}$/); // version and variant kept
  assert.notEqual(a, id);
  assert.notEqual(new Rules({ salt: 'another' }).pseudonymize(id), a);
  const seen = new Set();
  for (let i = 0; i < 3000; i++) seen.add(rules.pseudonymize(`n${i}`));
  assert.equal(seen.size, 3000, 'short identifiers must stay distinct');
  assert.equal(rules.pseudonymize('client-created-root'), 'client-created-root');
  assert.match(rules.pseudonymize('file-abcdefghij12'), /^file-[a-z]{10}\d{2}$/);
  assert.match(rules.pseudonymize('user-AbCdEf123456'), /^user-[A-Za-z0-9]{12}$/);
});

test('URLs keep scheme and known hosts only; everything else becomes inert', () => {
  const rules = new Rules();
  assert.match(rules.neutralizeUrl('https://gemini.google.com/app/c_abcdefgh12345?x=1#frag'), /^https:\/\/gemini\.google\.com\/app\/[a-z0-9_]+\?x=x#x$/);
  const unknown = rules.neutralizeUrl('https://private-host.example.org/secret/folder/file.txt?token=abc');
  assert.match(unknown, /^https:\/\/[a-z0-9]+\.invalid\//);
  assert.doesNotMatch(unknown, /private|secret|folder|token=abc/);
  assert.match(rules.neutralizeUrl('file-service://file-ABCDEFGH1234'), /^file-service:\/\/file-[A-Za-z0-9]{12}$/);
  assert.equal(rules.neutralizeUrl('not a url'), null);
  assert.match(rules.neutralizeUrl('data:image/png;base64,AAAA'), /^data:image\/png;base64,iVBOR/);
});

test('timestamps keep their representation and lose the time of day', () => {
  const rules = new Rules();
  assert.equal(rules.jsonNumber('1700001234.567', { key: 'create_time' }), '1699920000.000');
  assert.equal(rules.jsonNumber('1700001234', { key: 'update_time' }), '1699920000');
  assert.equal(rules.jsonNumber('1700001234567', { key: 'timestamp' }), '1699920000000');
  assert.equal(rules.jsonNumber('42', { key: 'weight' }), '42');
  assert.equal(new Rules({ timestamps: 'keep' }).jsonNumber('1700001234.567', { key: 'create_time' }), '1700001234.567');
  assert.equal(rules.coarsenClock('4 Nov 2026, 21:15:40 WIB'), '4 Nov 2026, 00:00:00 WIB');
  assert.equal(rules.coarsenClock('not a time'), null);
});

// ------------------------------------------------------------------ streaming rewriters

test('JSON rewriting is independent of how the input is chunked and keeps layout byte-for-byte', async () => {
  const source = '[ { "id" : "12345678-1234-4a23-b123-123456789abc",\n  "title":"Private \\"quoted\\" caf\\u00e9 title", "n": [1, 2.50, -3e2, true, null, {}] } ,\n\n 7 ]';
  const key = () => new Rules({ salt: 'chunking-test-key' });
  const whole = await collect(rewriteJson(bytesIn(source, 1 << 20), key(), { multiRoot: false }));
  for (const size of [1, 2, 3, 7, 64]) {
    assert.equal(await collect(rewriteJson(bytesIn(source, size), key())), whole, `chunk size ${size}`);
  }
  JSON.parse(whole);
  assert.ok(whole.includes('"n": [1, 2.50, -3e2, true, null, {}] }'), 'numbers, spacing and literals are untouched');
  assert.doesNotMatch(whole, /Private|quoted|café/);
});

test('malformed JSON fails with a code and never quotes the input', async () => {
  for (const bad of ['{"a": "secret-value-xyz" "b": 1', '[1, 2', '{"a": nope}', '"unterminated secret-value-xyz', '[1] [2]', '']) {
    await assert.rejects(collect(rewriteJson(bytesIn(bad), new Rules())), error => {
      assert.match(error.code, /^E_JSON_/);
      assert.doesNotMatch(error.message, /secret-value-xyz|nope/);
      return true;
    });
  }
  assert.ok(await collect(rewriteJson(bytesIn('{"a":1}\n{"a":2}\n'), new Rules(), { multiRoot: true })));
});

test('HTML scanning is independent of chunking and preserves tags, quoting and comments', async () => {
  const html = '<!doctype html><DIV class=\'outer-cell secret-class\' data-x=unquoted hidden><p>1 < 2 and a&amp;b</p><!-- note: Zanzibar --><br/><script>if (a<b) {document.write("<div>")}</script><a href="https://example.org/private">go</a></DIV>';
  const key = () => new Rules({ salt: 'chunking-test-key' });
  const whole = await collect(rewriteHtml(bytesIn(html, 1 << 20), key()));
  for (const size of [1, 2, 5, 11]) assert.equal(await collect(rewriteHtml(bytesIn(html, size), key())), whole, `chunk size ${size}`);
  assert.ok(whole.includes("<DIV class='outer-cell ") && whole.includes("' data-x=") && whole.includes(' hidden>'), 'attribute quoting and valueless attributes are kept');
  assert.doesNotMatch(whole, /secret-class/, 'an unknown class name is private by default');
  assert.ok(whole.includes('<!--') && whole.includes('<br/>') && whole.endsWith('</DIV>'));
  assert.doesNotMatch(whole, /Zanzibar|private|document\.write|a&amp;b/);
  const segments = [];
  for await (const s of scanHtml(bytesIn(html, 3))) segments.push(s.type);
  assert.ok(segments.includes('rawtext') && segments.includes('comment'));
});

test('an unterminated tag at end of input is treated as text, never as markup that hides content', async () => {
  const out = await collect(rewriteHtml(bytesIn('<p>fine</p><div class="never closed secret-token-zzz'), new Rules()));
  assert.doesNotMatch(out, /secret-token-zzz/);
});

// ------------------------------------------------------------------ ChatGPT exports

function conversationsOf(dir) {
  return jsonDocuments(dir).filter(([name]) => name.startsWith('conversations')).flatMap(([, doc]) => doc);
}

function shapeOf(conversation) {
  const nodes = Object.values(conversation.mapping);
  return {
    nodes: nodes.length,
    roots: nodes.filter(node => node.parent === null).length,
    branching: nodes.filter(node => node.children.length > 1).length,
    roles: nodes.map(node => node.message?.author?.role ?? null),
    contentTypes: nodes.map(node => node.message?.content?.content_type ?? null),
    partCounts: nodes.map(node => node.message?.content?.parts?.length ?? null),
    keys: nodes.map(node => Object.keys(node.message ?? {}).join(',')),
    hasCurrent: conversation.current_node in conversation.mapping,
  };
}

test('ChatGPT export with several conversations-*.json members: structure, references and quirks survive', async () => {
  const t = scratch('chatgpt-multi');
  try {
    const { secrets, conversations } = writeChatGptExport(path.join(t.dir, 'in'), { seed: 11, conversations: 6, multi: true });
    const zip = path.join(t.dir, 'export.zip');
    await zipDirectory(path.join(t.dir, 'in'), zip);
    const result = await sanitize({ input: zip, output: path.join(t.dir, 'out.zip') });
    assert.equal(result.exitCode, 0);
    assert.equal(result.report.checks.structure, 'PASS');
    assert.equal(result.report.checks.leak, 'PASS');
    const names = result.report.members.map(member => member.name);
    assert.deepEqual(names.filter(name => name.startsWith('conversations')).sort(), ['conversations-000.json', 'conversations-001.json']);
    assert.deepEqual(result.report.members.filter(m => m.name.startsWith('conversations')).map(m => m.structure.records), [3, 3]);
    assert.equal(result.report.totals.mediaStandIns, 2);

    // Re-read the sanitized zip and compare with the input conversation by conversation.
    const out = scratch('chatgpt-multi-read');
    try {
      const archive = await openZip(path.join(t.dir, 'out.zip'));
      for (const entry of archive.entries) {
        const target = path.join(out.dir, entry.name);
        fs.mkdirSync(path.dirname(target), { recursive: true });
        let data = Buffer.alloc(0);
        for await (const chunk of await archive.openEntry(entry)) data = Buffer.concat([data, chunk]);
        fs.writeFileSync(target, data);
      }
      await archive.close();
      const after = conversationsOf(out.dir);
      assert.equal(after.length, conversations.length);
      // Number formats are compared as text (JSON.parse would drop trailing zeros): same digit
      // counts and decimal places, only the time of day is gone.
      for (const member of ['conversations-000.json', 'conversations-001.json']) {
        const shapes = text => [...text.matchAll(/"(?:create|update)_time":(-?[\d.]+)/g)].map(match => match[1].replace(/\d/g, '9'));
        const rawIn = fs.readFileSync(path.join(t.dir, 'in', member), 'utf8');
        const rawOut = fs.readFileSync(path.join(out.dir, member), 'utf8');
        assert.deepEqual(shapes(rawOut), shapes(rawIn), member);
        assert.ok(shapes(rawIn).length > 5);
      }
      conversations.forEach((before, index) => {
        assert.deepEqual(shapeOf(after[index]), shapeOf(before), `conversation ${index}`);
        for (const [key, node] of Object.entries(after[index].mapping)) {
          assert.equal(node.id, key, 'mapping keys equal node ids');
          if (node.parent !== null && !(node.parent in after[index].mapping)) assert.match(JSON.stringify(before.mapping), /missing-parent-node/);
          for (const child of node.children) assert.equal(after[index].mapping[child].parent, key, 'child/parent links stay reciprocal');
        }
        // Duplicate message identifiers stay duplicates; distinct ones stay distinct.
        const ids = list => Object.values(list.mapping).map(node => node.message?.id).filter(Boolean);
        assert.equal(new Set(ids(after[index])).size, new Set(ids(before)).size);
      });
      assert.ok(after.some(c => 'client-created-root' in c.mapping));
      assert.ok(after.some(c => 'experimental_field_7' in c), 'unknown fields are kept as structure');
      assert.ok(Object.keys(result.report.unknownFields).length > 0, 'and reported for review');
      const text = outputText(out.dir);
      for (const secret of secrets) assert.ok(!text.includes(secret), 'a planted private string survived');
      assert.doesNotMatch(text, /privatemail|example\.net|PRIVATE-PIXELS/);
    } finally { out.done(); }
  } finally { t.done(); }
});

test('single-file ChatGPT export (loose conversations.json) works and its inventory is shareable', async () => {
  const t = scratch('chatgpt-single');
  try {
    const { secrets } = writeChatGptExport(path.join(t.dir, 'in'), { seed: 5, multi: false, conversations: 4, pretty: true });
    const input = path.join(t.dir, 'in', 'conversations.json');
    const output = path.join(t.dir, 'sanitized.json');
    const result = await sanitize({ input, output });
    assert.equal(result.exitCode, 0);
    assert.equal(result.report.inputKind, 'file');
    assert.equal(result.report.members.length, 1);
    assert.equal(result.report.members[0].structure.records, 4);
    const written = fs.readFileSync(output, 'utf8');
    assert.equal(JSON.parse(written).length, 4);
    assert.ok(/^\[\n {2}\{\n/.test(written), 'pretty-printed layout is preserved');
    const reportText = JSON.stringify(result.report);
    for (const secret of secrets) { assert.ok(!written.includes(secret)); assert.ok(!reportText.includes(secret), 'the report must not contain input text'); }
    assert.equal(fs.existsSync(`${output}.partial`), false);
  } finally { t.done(); }
});

test('directory export is sanitized member by member with safe, structure-keeping names', async () => {
  const t = scratch('chatgpt-dir');
  try {
    const { secrets } = writeChatGptExport(path.join(t.dir, 'in'), { seed: 2 });
    const result = await sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'out') });
    assert.equal(result.exitCode, 0);
    const files = [...readTree(path.join(t.dir, 'out')).keys()];
    for (const required of ['conversations-000.json', 'conversations-001.json', 'user.json', 'message_feedback.json', 'chat.html']) assert.ok(files.includes(required), required);
    assert.ok(files.some(name => name.startsWith('dalle-generations/')));
    for (const name of files) {
      assert.ok(!name.includes('..') && !path.isAbsolute(name));
      for (const secret of secrets) assert.ok(!name.includes(secret));
    }
    const png = [...readTree(path.join(t.dir, 'out'))].find(([name]) => name.endsWith('.png'));
    assert.ok(png && png[1].length < 100 && png[1].subarray(0, 4).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47])), 'media become tiny valid stand-ins');
  } finally { t.done(); }
});

test('property: across many random synthetic exports no input string of 8+ characters survives', async () => {
  const structural = new Set(['finished_successfully', 'client-created-root', 'multimodal_text', 'image_asset_pointer', 'file-service']);
  const tokensOf = text => (text.match(/[\p{L}\p{N}_-]{8,}/gu) ?? []);
  for (let seed = 100; seed < 112; seed++) {
    const t = scratch(`prop-${seed}`);
    try {
      writeChatGptExport(path.join(t.dir, 'in'), { seed, conversations: 3 + (seed % 4), multi: seed % 2 === 0 });
      const result = await sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'out') });
      assert.equal(result.exitCode, 0, `seed ${seed}`);
      // Independent oracle (does not use the tool's registry): every 8+ character token of every
      // input string must be absent from the output unless it is a structural key or enum value.
      const inputTokens = new Set();
      const allowed = new Set(structural);
      const walk = (value, key) => {
        if (typeof value === 'string') for (const token of tokensOf(value)) (['role', 'content_type', 'status', 'model_slug', 'default_model_slug', 'recipient'].includes(key) ? allowed : inputTokens).add(token);
        else if (Array.isArray(value)) value.forEach(item => walk(item, key));
        else if (value && typeof value === 'object') for (const [k, v] of Object.entries(value)) { for (const token of tokensOf(k)) allowed.add(token); walk(v, k); }
      };
      for (const [, doc] of jsonDocuments(path.join(t.dir, 'in'))) walk(doc, '');
      const output = outputText(path.join(t.dir, 'out'));
      const leaked = tokensOf(output).filter(token => inputTokens.has(token) && !allowed.has(token));
      assert.deepEqual(leaked, [], `seed ${seed}`);
    } finally { t.done(); }
  }
});

// ------------------------------------------------------------------ Gemini Takeout

test('Gemini Takeout folder: cards, quoting quirks, labels and duplicates survive; text does not', async () => {
  const t = scratch('gemini');
  try {
    const { secrets, html } = writeGeminiTakeout(path.join(t.dir, 'in'), { seed: 21, cards: 6 });
    const result = await sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'out'), ownerHints: true });
    assert.equal(result.exitCode, 0);
    assert.equal(result.report.provider, 'gemini');
    const out = [...readTree(path.join(t.dir, 'out'))].find(([name]) => name.endsWith('MyActivity.html'))[1].toString('utf8');
    const count = (text, pattern) => (text.match(pattern) ?? []).length;
    for (const pattern of [/class=["']?outer-cell/g, /class=["']?content-cell/g, /Prompted /g, /<br>/g, /<!--/g, /<p>/g, /<img /g, /<DIV class='outer-cell mdl-cell'>/g, /class=content-cell/g, /<\/DIV>/g]) {
      assert.equal(count(out, pattern), count(html, pattern), String(pattern));
    }
    assert.equal(count(out, /\d{1,2} Nov 2026, 00:00:00 WIB/g), count(html, /\d{1,2} Nov 2026, \d\d:\d\d:40 WIB/g), 'activity times keep their format, minus time of day');
    for (const secret of secrets) assert.ok(!out.includes(secret), 'a planted private string survived');
    assert.doesNotMatch(out, /privatemail|example\.net|gemini\.google\.com\/app\/c_/);
    assert.ok(result.hints.some(([word]) => word === 'Prompted'), 'owner-only hints list frequent leading words');
    assert.ok(!JSON.stringify(result.report).includes('Prompted '), 'hints never enter the report');
    assert.equal(result.report.members.find(m => m.name.endsWith('MyActivity.html')).structure.textNodes > 0, true);
  } finally { t.done(); }
});

test('Gemini Takeout zip round-trips as a zip', async () => {
  const t = scratch('gemini-zip');
  try {
    writeGeminiTakeout(path.join(t.dir, 'in'), { seed: 3, cards: 3 });
    await zipDirectory(path.join(t.dir, 'in'), path.join(t.dir, 'takeout.zip'));
    const result = await sanitize({ input: path.join(t.dir, 'takeout.zip'), output: path.join(t.dir, 'out.zip') });
    assert.equal(result.exitCode, 0);
    assert.equal(result.report.inputKind, 'zip');
    const archive = await openZip(path.join(t.dir, 'out.zip'));
    assert.ok(archive.entries.some(entry => entry.name.endsWith('MyActivity.html')));
    await archive.close();
  } finally { t.done(); }
});

// ------------------------------------------------------------------ determinism and fault injection

test('output is reproducible with an owner-supplied key: two runs give byte-identical zips and reports', async () => {
  const t = scratch('determinism');
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 9 });
    await zipDirectory(path.join(t.dir, 'in'), path.join(t.dir, 'e.zip'));
    const a = await sanitize({ input: path.join(t.dir, 'e.zip'), output: path.join(t.dir, 'a.zip'), salt: 'reproducible-owner-key' });
    const b = await sanitize({ input: path.join(t.dir, 'e.zip'), output: path.join(t.dir, 'b.zip'), salt: 'reproducible-owner-key' });
    assert.ok(fs.readFileSync(path.join(t.dir, 'a.zip')).equals(fs.readFileSync(path.join(t.dir, 'b.zip'))));
    assert.deepEqual(a.report, b.report);
    const salted = await sanitize({ input: path.join(t.dir, 'e.zip'), output: path.join(t.dir, 'c.zip'), salt: 'another-key' });
    assert.ok(!fs.readFileSync(path.join(t.dir, 'a.zip')).equals(fs.readFileSync(path.join(t.dir, 'c.zip'))), 'a different key gives different pseudonyms');
    assert.equal(salted.report.options.pseudonymKey, 'owner-supplied');
  } finally { t.done(); }
});

test('the leak check fails the run and leaves no output when a rule lets an input string through', async () => {
  const t = scratch('leak-trip');
  const original = Rules.prototype.jsonString;
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 4, multi: false });
    // Simulated rule bug: free text under "title" is passed through unchanged and unregistered.
    Rules.prototype.jsonString = function patched(value, ctx) { return ctx.key === 'title' ? value : original.call(this, value, ctx); };
    const output = path.join(t.dir, 'out');
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in'), output }), error => {
      assert.equal(error.code, 'E_LEAK');
      assert.equal(error.exitCode, 3);
      assert.doesNotMatch(error.message, /Notes about/);
      return true;
    });
    assert.equal(fs.existsSync(output), false);
    assert.equal(fs.existsSync(`${output}.partial`), false);
    assert.deepEqual(fs.readdirSync(t.dir).sort(), ['in']);
  } finally { Rules.prototype.jsonString = original; t.done(); }
});

test('the structure check fails the run when output keys no longer match the input', async () => {
  const t = scratch('structure-trip');
  const original = Rules.prototype.jsonKey;
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 4, multi: false });
    Rules.prototype.jsonKey = function patched(key, frame) { return key === 'title' ? 'titel' : original.call(this, key, frame); };
    const output = path.join(t.dir, 'out');
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in'), output }), error => error.code === 'E_STRUCTURE' && error.exitCode === 4);
    assert.equal(fs.existsSync(output), false);
    assert.equal(fs.existsSync(`${output}.partial`), false);
  } finally { Rules.prototype.jsonKey = original; t.done(); }
});

test('leak registry unit: whole strings and distinctive tokens are caught; kept values are not', () => {
  const registry = new LeakRegistry();
  registry.protect('Meeting with Bartholomew about confidential-roadmap');
  registry.protect('short');
  registry.markKept('multimodal_text');
  registry.protect('contains multimodal_text inside');
  assert.ok(registry.countLeaks('Meeting with Bartholomew about confidential-roadmap') >= 1);
  assert.equal(registry.countLeaks('only Bartholomew here'), 1);
  assert.equal(registry.countLeaks('x multimodal_text y'), 0);
  assert.equal(registry.countLeaks('short'), 0);
  assert.equal(registry.countLeaks('lorem ipsum dolor sit amet'), 0);
});

// ------------------------------------------------------------------ hostile and broken input

test('a member that is not valid JSON is replaced by an empty stand-in and reported without content', async () => {
  const t = scratch('broken-member');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    fs.writeFileSync(path.join(t.dir, 'in', 'conversations.json'), '[{"id":"x","title":"super-secret-title-xyz" "oops": 1}');
    fs.writeFileSync(path.join(t.dir, 'in', 'user.json'), '{"id":"abc","email":"owner@privatemail.test"}');
    const result = await sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'out') });
    assert.equal(result.exitCode, 2);
    const broken = result.report.members.find(m => m.name === 'conversations.json');
    assert.match(broken.status, /^failed:E_JSON_/);
    assert.doesNotMatch(JSON.stringify(result.report), /super-secret-title-xyz/);
    assert.equal(fs.readFileSync(path.join(t.dir, 'out', 'conversations.json'), 'utf8'), '{}\n');
    assert.doesNotMatch(outputText(path.join(t.dir, 'out')), /super-secret-title-xyz|privatemail/);
  } finally { t.done(); }
});

test('zip safety: tampered data and lying headers fail the member, not the process', async () => {
  const t = scratch('zip-tamper');
  try {
    const writer = new ZipWriter(path.join(t.dir, 'good.zip'));
    await writer.addFile('conversations.json', Readable.from([Buffer.from(JSON.stringify([{ id: 'a'.repeat(40), title: 'abc'.repeat(200) }]))]));
    await writer.finish();
    const bytes = fs.readFileSync(path.join(t.dir, 'good.zip'));
    // Flip a byte inside the compressed payload (after the 30-byte header + 18-byte name).
    const bad = Buffer.from(bytes);
    bad[30 + 'conversations.json'.length + 4] ^= 0xff;
    fs.writeFileSync(path.join(t.dir, 'bad.zip'), bad);
    const result = await sanitize({ input: path.join(t.dir, 'bad.zip'), output: path.join(t.dir, 'out.zip') });
    assert.equal(result.exitCode, 2);
    assert.match(result.report.members[0].status, /^failed:/);
    // Truncated file: not a zip, and not silently treated as a JSON file either.
    fs.writeFileSync(path.join(t.dir, 'trunc.zip'), bytes.subarray(0, bytes.length - 10));
    await assert.rejects(sanitize({ input: path.join(t.dir, 'trunc.zip'), output: path.join(t.dir, 'o2.zip') }), error => /^E_ZIP_/.test(error.code));
    assert.equal(fs.existsSync(path.join(t.dir, 'o2.zip')), false);
  } finally { t.done(); }
});

test('output path safety: no overwrite, not inside the input, no symlinked input', async () => {
  const t = scratch('paths');
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 1, multi: false, conversations: 2 });
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'in', 'out') }), { code: 'E_OUTPUT_OVERLAPS_INPUT' });
    fs.mkdirSync(path.join(t.dir, 'exists'));
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'exists') }), { code: 'E_OUTPUT_EXISTS' });
    fs.symlinkSync(path.join(t.dir, 'in'), path.join(t.dir, 'link'));
    await assert.rejects(sanitize({ input: path.join(t.dir, 'link'), output: path.join(t.dir, 'o') }), { code: 'E_INPUT_SYMLINK' });
    // A symlink inside the input tree is skipped, never followed.
    fs.writeFileSync(path.join(t.dir, 'outside-secret.json'), '{"id":"outside"}');
    fs.symlinkSync(path.join(t.dir, 'outside-secret.json'), path.join(t.dir, 'in', 'escape.json'));
    const result = await sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'safe-out') });
    assert.equal(result.report.skippedSymbolicLinks, 1);
    assert.ok(![...readTree(path.join(t.dir, 'safe-out')).keys()].some(name => /escape/.test(name)));
  } finally { t.done(); }
});

test('a large conversation is streamed: a multi-megabyte message becomes a bounded placeholder', async () => {
  const t = scratch('large');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    const huge = 'secretparagraph '.repeat(300_000); // ~4.8 MB of text in one string
    fs.writeFileSync(path.join(t.dir, 'in', 'conversations.json'), JSON.stringify([{ id: 'c'.repeat(36), title: 't', mapping: { a: { id: 'a', parent: null, children: [], message: { content: { content_type: 'text', parts: [huge] } } } }, current_node: 'a' }]));
    const result = await sanitize({ input: path.join(t.dir, 'in', 'conversations.json'), output: path.join(t.dir, 'out.json') });
    assert.equal(result.exitCode, 0);
    assert.ok(fs.statSync(path.join(t.dir, 'out.json')).size < 40_000);
    assert.doesNotMatch(fs.readFileSync(path.join(t.dir, 'out.json'), 'utf8'), /secretparagraph/);
  } finally { t.done(); }
});

// ------------------------------------------------------------------ zip module

test('zip writer output is valid for independent readers and supports zip64 entry counts', async () => {
  const t = scratch('zip');
  try {
    const writer = new ZipWriter(path.join(t.dir, 'a.zip'));
    await writer.addDirectory('d/');
    await writer.addFile('d/é.txt', Readable.from([Buffer.from('hello '.repeat(5000))]));
    await writer.addFile('empty', Buffer.alloc(0));
    assert.equal((await writer.finish()).zip64, false);
    const many = new ZipWriter(path.join(t.dir, 'many.zip'));
    for (let i = 0; i < 66_000; i++) await many.addFile(`f${i}`, Buffer.from(String(i)));
    assert.equal((await many.finish()).zip64, true);
    const reread = await openZip(path.join(t.dir, 'many.zip'));
    assert.equal(reread.entries.length, 66_000);
    await reread.close();
    const python = spawnSync('python3', ['-I', '-c', 'import sys, zipfile\nfor f in sys.argv[1:]:\n    z = zipfile.ZipFile(f)\n    assert z.testzip() is None, f\nprint("ok")', path.join(t.dir, 'a.zip'), path.join(t.dir, 'many.zip')], { encoding: 'utf8' });
    if (python.error) return; // python3 not installed: the in-process reader already verified CRCs
    assert.equal(python.stdout.trim(), 'ok', python.stderr);
  } finally { t.done(); }
});

// ------------------------------------------------------------------ command line

function runCli(args, env = {}) {
  return spawnSync(process.execPath, [cli, ...args], { encoding: 'utf8', env: { ...process.env, ...env } });
}

test('CLI: exit codes, report file and terminal output contain structure and no input text', async () => {
  const t = scratch('cli');
  try {
    const { secrets } = writeChatGptExport(path.join(t.dir, 'in'), { seed: 6 });
    await zipDirectory(path.join(t.dir, 'in'), path.join(t.dir, 'e.zip'));
    const report = path.join(t.dir, 'report.json');
    const run = runCli(['--input', path.join(t.dir, 'e.zip'), '--output', path.join(t.dir, 'o.zip'), '--report', report]);
    assert.equal(run.status, 0, run.stderr);
    assert.match(run.stdout, /Structure check: PASS/);
    assert.match(run.stdout, /Leak check \(nothing outside the allow-lists survived\): PASS/);
    assert.match(run.stdout, /RESIDUAL METADATA/);
    assert.match(run.stdout, /NOT_RUN/, 'running on a real export is recorded as an owner-local NOT_RUN step');
    const shared = `${run.stdout}\n${run.stderr}\n${fs.readFileSync(report, 'utf8')}`;
    for (const secret of secrets) assert.ok(!shared.includes(secret), 'terminal/report text must not include input content');
    assert.equal(JSON.parse(fs.readFileSync(report, 'utf8')).shareable, true);

    assert.equal(runCli(['--input', path.join(t.dir, 'e.zip'), '--output', path.join(t.dir, 'o.zip')]).status, 1, 'refuses to overwrite');
    assert.equal(runCli(['--input', path.join(t.dir, 'e.zip')]).status, 1, 'needs --output or --dry-run');
    assert.equal(runCli(['--input', path.join(t.dir, 'e.zip'), '--dry-run']).status, 0);
    assert.equal(fs.readdirSync(t.dir).some(name => name.startsWith('mp-sanitize')), false, 'dry run leaves nothing behind');
    assert.match(runCli(['--help']).stdout, /Exit codes/);
    assert.equal(runCli(['--input', path.join(t.dir, 'missing'), '--dry-run']).status, 1);
    assert.equal(runCli(['--input', path.join(t.dir, 'e.zip'), '--dry-run', '--timestamps', 'week']).status, 1);

    const saltFile = path.join(t.dir, 'salt.txt');
    fs.writeFileSync(saltFile, 'private-salt-value');
    assert.equal(runCli(['--input', path.join(t.dir, 'e.zip'), '--output', path.join(t.dir, 'salted.zip'), '--salt-file', saltFile]).status, 0);
    assert.equal(runCli(['--input', path.join(t.dir, 'e.zip'), '--output', path.join(t.dir, 'env.zip')], { MIND_PLACE_SANITIZER_SALT: 'private-salt-value' }).status, 0);
    assert.ok(fs.readFileSync(path.join(t.dir, 'salted.zip')).equals(fs.readFileSync(path.join(t.dir, 'env.zip'))), 'salt file and environment salt are equivalent');
    assert.ok(!/--salt\b(?!-file)/.test(runCli(['--help']).stdout), 'there is no command-line salt option');
  } finally { t.done(); }
});

test('the sanitizer tree is offline: no network, process or eval APIs', () => {
  const dir = path.join(root, 'scripts', 'import-sanitizer');
  for (const name of fs.readdirSync(dir).filter(file => file.endsWith('.mjs'))) {
    const source = fs.readFileSync(path.join(dir, name), 'utf8');
    assert.doesNotMatch(source, /node:(?:http2?|https|net|dgram|dns|tls|child_process|worker_threads|vm)\b/, name);
    assert.doesNotMatch(source, /\bfetch\s*\(|\bXMLHttpRequest\b|\bWebSocket\b|\beval\s*\(|new Function\s*\(/, name);
    assert.doesNotMatch(source, /\bconsole\./, `${name} must not log`);
  }
});

test('committed parser-compatibility fixtures are exactly what the tool produces (no drift)', async () => {
  const { buildParserCompatFixtures, FIXTURE_DIR } = await import('./fixtures/import-sanitizer/generate-parser-compat.mjs');
  const t = scratch('compat');
  try {
    await buildParserCompatFixtures(t.dir);
    for (const name of ['chatgpt-synthetic.json', 'chatgpt-sanitized.json', 'gemini-synthetic.html', 'gemini-sanitized.html']) {
      assert.ok(fs.readFileSync(path.join(t.dir, name)).equals(fs.readFileSync(path.join(FIXTURE_DIR, name))),
        `${name} differs; regenerate with: node test/fixtures/import-sanitizer/generate-parser-compat.mjs`);
    }
  } finally { t.done(); }
});

test('the report lists exactly which enum-like values were kept so the owner can review them', async () => {
  const t = scratch('kept-values');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    fs.writeFileSync(path.join(t.dir, 'in', 'conversations.json'), JSON.stringify([{
      id: 'a'.repeat(36), title: 'visible-private-title', current_node: 'n1',
      mapping: { n1: { id: 'n1', parent: null, children: [], message: { author: { role: 'user', name: 'browser' }, content: { content_type: 'text', parts: ['x'.repeat(40)] }, status: 'finished_successfully', metadata: { source: 'private.example.com extra words here' } } } },
    }]));
    const result = await sanitize({ input: path.join(t.dir, 'in', 'conversations.json'), output: path.join(t.dir, 'out.json') });
    const kept = result.report.residualMetadata.enumValues.byKey;
    assert.deepEqual(Object.keys(kept.role), ['user']);
    assert.deepEqual(Object.keys(kept.content_type), ['text']);
    assert.deepEqual(Object.keys(kept.status), ['finished_successfully']);
    assert.deepEqual(Object.keys(kept['author.name']), ['browser']);
    // A free-text value under an enum key does not look like an enum, so it is replaced, not kept.
    assert.equal(kept.source, undefined);
    assert.doesNotMatch(fs.readFileSync(path.join(t.dir, 'out.json'), 'utf8'), /private\.example|extra words|visible-private-title/);
  } finally { t.done(); }
});

test('member names are portable: backslash separators and Windows-reserved characters are neutralized', () => {
  const rules = new Rules();
  const name = rules.memberName('Photos\\Holiday: "Bali" <2023>?\\IMG*001.PNG');
  assert.ok(!name.includes('\\'));
  assert.equal(name.split('/').length, 3);
  assert.doesNotMatch(name, /[:"<>?*|]/);
  assert.ok(name.endsWith('.PNG'));
  assert.ok(!rules.memberName('../../etc/passwd').split('/').includes('..'));
  assert.ok(!rules.memberName('/abs/path.txt').startsWith('/'));
  assert.equal(rules.memberName('trailing.dot. ').endsWith('.'), false);
});

// ------------------------------------------------------------------ re-review round 1 (SAN-F01..F05)

test('SAN-F01: an output that contains the input is refused before anything is touched, even with --force', async () => {
  const t = scratch('san-f01-ancestor');
  try {
    writeChatGptExport(path.join(t.dir, 'src'), { seed: 1, multi: false, conversations: 2 });
    fs.mkdirSync(path.join(t.dir, 'exports'));
    const original = path.join(t.dir, 'exports', 'original.zip');
    await zipDirectory(path.join(t.dir, 'src'), original);
    const before = fs.readFileSync(original);
    const survives = () => assert.ok(fs.readFileSync(original).equals(before), 'the original export must survive');

    for (const output of [path.join(t.dir, 'exports'), `${path.join(t.dir, 'exports')}${path.sep}`, t.dir, path.join(t.dir, 'exports', '..', 'exports')]) {
      await assert.rejects(sanitize({ input: original, output, force: true }), { code: 'E_OUTPUT_OVERLAPS_INPUT' }, output);
      survives();
    }
    // The input itself and a location inside an input directory are refused as before.
    await assert.rejects(sanitize({ input: original, output: original, force: true }), { code: 'E_OUTPUT_OVERLAPS_INPUT' });
    await assert.rejects(sanitize({ input: path.join(t.dir, 'src'), output: path.join(t.dir, 'src', 'nested'), force: true }), { code: 'E_OUTPUT_OVERLAPS_INPUT' });
    survives();

    // Symbolic-link aliases resolve to the same real paths.
    fs.symlinkSync(path.join(t.dir, 'exports'), path.join(t.dir, 'alias'));
    await assert.rejects(sanitize({ input: original, output: path.join(t.dir, 'alias'), force: true }), { code: 'E_OUTPUT_OVERLAPS_INPUT' });
    await assert.rejects(sanitize({ input: path.join(t.dir, 'alias', 'original.zip'), output: path.join(t.dir, 'exports'), force: true }), { code: 'E_OUTPUT_OVERLAPS_INPUT' });
    fs.symlinkSync(path.join(t.dir, 'src'), path.join(t.dir, 'src-alias'));
    await assert.rejects(sanitize({ input: path.join(t.dir, 'src'), output: path.join(t.dir, 'src-alias', 'nested') }), { code: 'E_OUTPUT_OVERLAPS_INPUT' });
    survives();
    assert.deepEqual(fs.readdirSync(path.join(t.dir, 'exports')), ['original.zip'], 'nothing was created next to the input either');
  } finally { t.done(); }
});

test('SAN-F01: --force never destroys the existing output unless the replacement is complete and verified', async () => {
  const t = scratch('san-f01-replace');
  const original = Rules.prototype.jsonString;
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 8, multi: false, conversations: 2 });
    const output = path.join(t.dir, 'out.json');
    fs.writeFileSync(output, 'PREVIOUS OUTPUT');
    const input = path.join(t.dir, 'in', 'conversations.json');

    // Replacement fails its leak check: the previous output is untouched and nothing is left over.
    Rules.prototype.jsonString = function leaky(value, ctx) { return ctx.key === 'title' ? value : original.call(this, value, ctx); };
    await assert.rejects(sanitize({ input, output, force: true }), { code: 'E_LEAK' });
    Rules.prototype.jsonString = original;
    assert.equal(fs.readFileSync(output, 'utf8'), 'PREVIOUS OUTPUT');
    assert.deepEqual(fs.readdirSync(t.dir).sort(), ['in', 'out.json']);

    // Without --force an existing output is never replaced.
    await assert.rejects(sanitize({ input, output }), { code: 'E_OUTPUT_EXISTS' });
    assert.equal(fs.readFileSync(output, 'utf8'), 'PREVIOUS OUTPUT');

    // A verified replacement swaps in and leaves no aside copy or partial file behind.
    const result = await sanitize({ input, output, force: true });
    assert.equal(result.exitCode, 0);
    assert.equal(JSON.parse(fs.readFileSync(output, 'utf8')).length, 2);
    assert.deepEqual(fs.readdirSync(t.dir).sort(), ['in', 'out.json']);
  } finally { Rules.prototype.jsonString = original; t.done(); }
});

test('SAN-F01: --force only replaces sanitizer output, never an unrelated directory or a symlink', async () => {
  const t = scratch('san-f01-targets');
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 3, conversations: 2 });
    fs.mkdirSync(path.join(t.dir, 'precious'));
    fs.writeFileSync(path.join(t.dir, 'precious', 'thesis.txt'), 'irreplaceable');
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'precious'), force: true }), { code: 'E_OUTPUT_NOT_REPLACEABLE' });
    assert.equal(fs.readFileSync(path.join(t.dir, 'precious', 'thesis.txt'), 'utf8'), 'irreplaceable');

    // A directory this tool wrote earlier is recognized and can be replaced.
    const out = path.join(t.dir, 'sanitized');
    assert.equal((await sanitize({ input: path.join(t.dir, 'in'), output: out })).exitCode, 0);
    assert.equal((await sanitize({ input: path.join(t.dir, 'in'), output: out, force: true })).exitCode, 0);
    assert.deepEqual(fs.readdirSync(t.dir).sort(), ['in', 'precious', 'sanitized']);

    fs.mkdirSync(path.join(t.dir, 'target-dir'));
    fs.symlinkSync(path.join(t.dir, 'target-dir'), path.join(t.dir, 'link-out'));
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'link-out'), force: true }), { code: 'E_OUTPUT_SYMLINK' });
    assert.ok(fs.existsSync(path.join(t.dir, 'target-dir')));
  } finally { t.done(); }
});

test('SAN-F01: the report path cannot clobber the input or the output', async () => {
  const t = scratch('san-f01-report');
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 3, multi: false, conversations: 2 });
    const input = path.join(t.dir, 'in', 'conversations.json');
    const before = fs.readFileSync(input);
    const run = report => runCli(['--input', input, '--output', path.join(t.dir, 'o.json'), '--report', report, '--force']);
    assert.equal(run(input).status, 1);
    assert.equal(run(path.join(t.dir, 'in')).status, 1);
    assert.equal(run(path.join(t.dir, 'o.json')).status, 1);
    assert.ok(fs.readFileSync(input).equals(before));
    assert.equal(fs.existsSync(path.join(t.dir, 'o.json')), false, 'a refused report path stops the run before any work');
    assert.equal(run(path.join(t.dir, 'report.json')).status, 0);
  } finally { t.done(); }
});

test('SAN-F02: private author names, unknown enum values, custom classes and attributes are replaced, not kept', async () => {
  const t = scratch('san-f02');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    fs.writeFileSync(path.join(t.dir, 'in', 'conversations.json'), JSON.stringify([{
      id: 'a'.repeat(36), title: 't', current_node: 'n1', default_model_slug: 'acme-secret-model',
      mapping: { n1: { id: 'n1', parent: null, children: [], message: {
        author: { role: 'user', name: 'Alice-Johnson-Private' },
        content: { content_type: 'x-acme-secret-type', parts: ['x'.repeat(40)] },
        status: 'queued-for-acme-corp', recipient: 'acme.internal.tool', channel: 'secret-channel-name',
        metadata: { model_slug: 'gpt-4o', type: 'acme-private-kind', language: 'cobol-for-acme', mime_type: 'application/x-acme-secret' },
      } } },
    }]));
    const result = await sanitize({ input: path.join(t.dir, 'in', 'conversations.json'), output: path.join(t.dir, 'out.json') });
    const written = fs.readFileSync(path.join(t.dir, 'out.json'), 'utf8');
    for (const secret of ['Alice-Johnson-Private', 'x-acme-secret-type', 'queued-for-acme-corp', 'acme.internal.tool', 'secret-channel-name', 'acme-secret-model', 'acme-private-kind', 'cobol-for-acme', 'application/x-acme-secret']) {
      assert.ok(!written.includes(secret), `${secret} survived`);
    }
    assert.ok(result.report.counters.enumValuesReplaced >= 8);
    const kept = result.report.residualMetadata.enumValues.byKey;
    assert.deepEqual(Object.keys(kept.role), ['user']);
    assert.deepEqual(Object.keys(kept.model_slug), ['gpt-4o'], 'only independently known provider vocabulary is kept');
    assert.equal(kept['author.name'], undefined);
    assert.equal(kept.status, undefined);

    fs.writeFileSync(path.join(t.dir, 'in', 'page.html'), '<div class="acme-internal-banner outer-cell" lang="xx-Private" dir="ltr" role="acme-role" title="Alice private" data-user="alice" id="user-alice-1"><p class="acme-text">hello there friend</p></div>');
    const html = await sanitize({ input: path.join(t.dir, 'in', 'page.html'), output: path.join(t.dir, 'out.html') });
    const out = fs.readFileSync(path.join(t.dir, 'out.html'), 'utf8');
    for (const secret of ['acme-internal-banner', 'acme-text', 'xx-Private', 'acme-role', 'Alice private', 'alice', 'hello there']) assert.ok(!out.includes(secret), `${secret} survived`);
    assert.ok(out.includes('outer-cell') && out.includes('dir="ltr"'), 'known vocabulary is kept');
    assert.deepEqual(html.report.residualMetadata.htmlClassTokens, ['outer-cell']);
    assert.deepEqual(html.report.residualMetadata.htmlAttributeValues, ['dir=ltr']);

    // Explicit, reviewed opt-in keeps a value and lists it as residual.
    const optIn = await sanitize({ input: path.join(t.dir, 'in', 'page.html'), output: path.join(t.dir, 'out2.html'), keepClasses: ['acme-text'] });
    assert.ok(fs.readFileSync(path.join(t.dir, 'out2.html'), 'utf8').includes('acme-text'));
    assert.ok(optIn.report.residualMetadata.htmlClassTokens.includes('acme-text'));
    const kept2 = await sanitize({ input: path.join(t.dir, 'in', 'conversations.json'), output: path.join(t.dir, 'out3.json'), keepEnums: ['status=queued-for-acme-corp'] });
    assert.deepEqual(Object.keys(kept2.report.residualMetadata.enumValues.byKey.status), ['queued-for-acme-corp']);
    assert.equal(kept2.report.options.keepEnums, 1);
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in', 'page.html'), output: path.join(t.dir, 'x.html'), keepEnums: ['no-equals-sign'] }), /E_OPTION/);
  } finally { t.done(); }
});

test('SAN-F03: the default pseudonym key is a fresh random secret, so identifiers cannot be enumerated offline', async () => {
  const t = scratch('san-f03');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    // Low-entropy, sequential identifiers: exactly what a public key would let an attacker confirm.
    const nodes = Array.from({ length: 40 }, (_, i) => `conv-${String(i + 1).padStart(9, '0')}`);
    fs.writeFileSync(path.join(t.dir, 'in', 'conversations.json'), JSON.stringify([{
      id: nodes[0], title: 't', current_node: nodes[39],
      mapping: Object.fromEntries(nodes.map((id, i) => [id, { id, parent: i ? nodes[i - 1] : null, children: i < 39 ? [nodes[i + 1]] : [], message: null }])),
    }]));
    const input = path.join(t.dir, 'in', 'conversations.json');
    const a = await sanitize({ input, output: path.join(t.dir, 'a.json') });
    const b = await sanitize({ input, output: path.join(t.dir, 'b.json') });
    const idsOf = file => Object.keys(JSON.parse(fs.readFileSync(file, 'utf8'))[0].mapping);
    assert.notDeepEqual(idsOf(path.join(t.dir, 'a.json')), idsOf(path.join(t.dir, 'b.json')), 'every run uses its own key');
    assert.equal(a.report.options.pseudonymKey, 'random-per-run');
    // Structure is intact: links resolve and identifiers stay distinct.
    const mapping = JSON.parse(fs.readFileSync(path.join(t.dir, 'a.json'), 'utf8'))[0].mapping;
    assert.equal(new Set(Object.keys(mapping)).size, 40);
    for (const node of Object.values(mapping)) if (node.parent !== null) assert.ok(node.parent in mapping);

    // An attacker who knows the tool and the former public default key, and enumerates the
    // candidate identifiers, cannot reproduce any pseudonym.
    const attacker = new Rules({ salt: 'mind-place-import-sanitizer-v1' });
    const candidates = new Set(nodes.map(id => attacker.pseudonymize(id)));
    assert.equal(idsOf(path.join(t.dir, 'a.json')).filter(id => candidates.has(id)).length, 0);
    // Nothing that would let someone recover the mapping is written anywhere.
    const everything = `${JSON.stringify(a.report)}\n${fs.readFileSync(path.join(t.dir, 'a.json'), 'utf8')}`;
    assert.ok(!nodes.some(id => everything.includes(id)), 'no original identifier appears in the output or report');
    assert.doesNotMatch(everything, /mind-place-import-sanitizer-v1|"salt"/);
    // Reproducibility is an explicit owner opt-in.
    const keyA = await sanitize({ input, output: path.join(t.dir, 'c.json'), salt: 'owner-key' });
    const keyB = await sanitize({ input, output: path.join(t.dir, 'd.json'), salt: 'owner-key' });
    assert.ok(fs.readFileSync(path.join(t.dir, 'c.json')).equals(fs.readFileSync(path.join(t.dir, 'd.json'))));
    assert.equal(keyA.report.options.pseudonymKey, 'owner-supplied');
    assert.notEqual(new Rules().pseudonymize('conv-000000001'), new Rules().pseudonymize('conv-000000001'));
    assert.equal(keyB.exitCode, 0);
  } finally { t.done(); }
});

test('SAN-F04: if leak tracking overflows the run fails closed and publishes nothing', async () => {
  const t = scratch('san-f04');
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 14, conversations: 3, multi: false });
    const input = path.join(t.dir, 'in', 'conversations.json');
    const output = path.join(t.dir, 'out.json');
    await assert.rejects(sanitize({ input, output, budgets: { maxProtectedPieces: 5 } }), error => {
      assert.equal(error.code, 'E_LEAK_COVERAGE');
      assert.equal(error.exitCode, 3);
      assert.equal(error.checks.leak, 'INCOMPLETE');
      return true;
    });
    assert.equal(fs.existsSync(output), false);
    assert.equal(fs.existsSync(`${output}.partial`), false);
    const full = await sanitize({ input, output });
    assert.equal(full.report.checks.leakRegistry.coverage, 'full');
    assert.equal(full.report.checks.leak, 'PASS');
  } finally { t.done(); }
});

test('SAN-F05: the report separates structure fidelity from privacy and lists the residual metadata', async () => {
  const t = scratch('san-f05');
  try {
    writeChatGptExport(path.join(t.dir, 'chatgpt'), { seed: 2 });
    writeGeminiTakeout(path.join(t.dir, 'gemini'), { seed: 2, cards: 4 });
    const chat = await sanitize({ input: path.join(t.dir, 'chatgpt'), output: path.join(t.dir, 'o1') });
    assert.equal(chat.report.qualification.structureFidelity, 'PASS');
    assert.equal(chat.report.qualification.leakCheck, 'PASS');
    assert.equal(chat.report.qualification.contentFreeAssertion, 'NOT_ASSERTED');
    assert.match(chat.report.qualification.meaning, /not a guarantee of anonymity/);
    const residual = chat.report.residualMetadata;
    assert.ok(residual.memberNames.includes('conversations-000.json') && residual.memberNames.includes('user.json'));
    assert.ok(residual.enumValues.byKey.role && residual.enumValues.byKey.content_type);
    assert.match(residual.timestampPrecision, /time of day removed/);
    assert.ok(residual.structuralLabels.includes('Prompted'));
    assert.equal(chat.report.keptValues, undefined);

    const gemini = await sanitize({ input: path.join(t.dir, 'gemini'), output: path.join(t.dir, 'o2') });
    assert.ok(gemini.report.residualMetadata.linkHosts.includes('gemini.google.com'));
    assert.ok(gemini.report.residualMetadata.htmlClassTokens.includes('outer-cell'));
    assert.ok(gemini.report.residualMetadata.memberNames.includes('MyActivity.html'));

    const run = runCli(['--input', path.join(t.dir, 'gemini'), '--dry-run']);
    assert.equal(run.status, 0, run.stderr);
    assert.match(run.stdout, /RESIDUAL METADATA that stays in the output/);
    assert.match(run.stdout, /link hosts: gemini\.google\.com/);
    assert.match(run.stdout, /HTML class tokens: .*outer-cell/);
    assert.match(runCli(['--help']).stdout, /NOT an anonymity guarantee/);

    const keep = await sanitize({ input: path.join(t.dir, 'chatgpt'), output: path.join(t.dir, 'o3'), timestamps: 'keep' });
    assert.match(keep.report.residualMetadata.timestampPrecision, /full precision/);
  } finally { t.done(); }
});

// ------------------------------------------------------------------ SAN-F06 (resource budgets)

/** Runs a sanitize that must hit a budget, and proves the failure is safe. */
async function assertBudgetStops(t, input, budgets, budget, extra = {}) {
  const output = path.join(t.dir, 'out');
  fs.writeFileSync(output, 'PREVIOUS OUTPUT');
  const before = fs.readdirSync(t.dir).sort();
  await assert.rejects(sanitize({ input, output, force: true, budgets, ...extra }), error => {
    assert.equal(error.code, 'E_BUDGET', `${budget}: ${error.message}`);
    assert.equal(error.budget, budget);
    assert.equal(error.exitCode, 5);
    assert.equal(error.limit, budgets[budget]);
    assert.doesNotMatch(error.message, /secret/i, 'a budget message never quotes input');
    return true;
  });
  assert.equal(fs.readFileSync(output, 'utf8'), 'PREVIOUS OUTPUT', 'the previous output survives');
  assert.deepEqual(fs.readdirSync(t.dir).sort(), before, 'no partial, spool or aside file is left behind');
}

test('SAN-F06: every input-dependent structure has a named budget and the worst case is bounded and documented', async () => {
  const names = Object.keys(DEFAULT_BUDGETS);
  for (const name of ['maxProtectedPieces', 'maxKeptPieces', 'maxDistinctIdentifiers', 'maxUnknownClassTokens', 'maxShortIdentifiers', 'maxShortIdentifierBytes', 'maxMembers', 'maxSummaryEntries', 'maxJsonDepth', 'maxTokenChars']) {
    assert.ok(names.includes(name), name);
    assert.ok(Number.isInteger(DEFAULT_BUDGETS[name]) && DEFAULT_BUDGETS[name] > 0, name);
  }
  assert.ok(totalUpperBoundBytes() < 1024 ** 3, 'if every budget were reached at once the structures still stay below 1 GiB');
  assert.ok(totalUpperBoundBytes({ ...DEFAULT_BUDGETS, maxProtectedPieces: 1000 }) < totalUpperBoundBytes());
  await assert.rejects(sanitize({ input: 'x', output: 'y', budgets: { nope: 1 } }), /E_OPTION/);
  const readme = fs.readFileSync(path.join(root, 'scripts', 'import-sanitizer', 'README.md'), 'utf8');
  for (const name of names) assert.ok(readme.includes(name), `README documents ${name}`);
  assert.doesNotMatch(readme, /memory bounded by the largest single string/);
});

test('SAN-F06: the hash tables never grow past their budget, however many distinct values arrive', () => {
  const counter = new DistinctCounter({ max: 1000, budget: 'maxDistinctIdentifiers' });
  let thrownAt = null;
  for (let i = 0; i < 1_000_000; i++) {
    try { counter.add(`identifier-${i}`); } catch (error) { thrownAt = i; assert.equal(error.code, 'E_BUDGET'); break; }
  }
  assert.equal(thrownAt, 1000, 'the 1001st distinct value is the first refused');
  assert.ok(counter.set.capacity <= 4096, `capacity ${counter.set.capacity}`);
  counter.add('identifier-5'); // a value already counted is never refused
  const registry = new LeakRegistry({ maxHashes: 100, maxKept: 100 });
  for (let i = 0; i < 100_000; i++) registry.protect(`input-string-number-${i}`);
  assert.equal(registry.partial, true);
  assert.equal(registry.protectedSet.capacity, 1 << 16, 'the table keeps its initial size instead of growing past the budget');
  assert.throws(() => { for (let i = 0; i < 1000; i++) registry.markKept(`allow-listed-value-${i}`); }, { code: 'E_BUDGET' });
});

test('SAN-F06: many distinct identifiers stop the run safely (JSON identifiers, short identifiers, kept keys)', async () => {
  const t = scratch('san-f06-ids');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    const conversation = (ids, key) => JSON.stringify([{ id: ids[0], title: 't', current_node: ids[0], mapping: Object.fromEntries(ids.map(id => [id, { id, parent: null, children: [], message: null, [key]: 1 }])) }]);
    const uuids = Array.from({ length: 300 }, (_, i) => `00000000-0000-4000-8000-${String(i).padStart(12, '0')}`);
    fs.writeFileSync(path.join(t.dir, 'in', 'a.json'), conversation(uuids, 'x'));
    await assertBudgetStops(t, path.join(t.dir, 'in', 'a.json'), { maxDistinctIdentifiers: 50 }, 'maxDistinctIdentifiers');

    const shorts = Array.from({ length: 300 }, (_, i) => `n${i}`);
    fs.writeFileSync(path.join(t.dir, 'in', 'b.json'), conversation(shorts, 'x'));
    await assertBudgetStops(t, path.join(t.dir, 'in', 'b.json'), { maxShortIdentifiers: 100 }, 'maxShortIdentifiers');
    await assertBudgetStops(t, path.join(t.dir, 'in', 'b.json'), { maxShortIdentifierBytes: 600 }, 'maxShortIdentifierBytes');

    // Many distinct object keys are each registered as an allow-listed (kept) value.
    const keys = Object.fromEntries(Array.from({ length: 300 }, (_, i) => [`distinct_field_name_${i}`, i]));
    fs.writeFileSync(path.join(t.dir, 'in', 'c.json'), JSON.stringify([keys]));
    await assertBudgetStops(t, path.join(t.dir, 'in', 'c.json'), { maxKeptPieces: 50 }, 'maxKeptPieces');
  } finally { t.done(); }
});

test('SAN-F06: many distinct HTML class names stop the run safely', async () => {
  const t = scratch('san-f06-classes');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    const cards = Array.from({ length: 200 }, (_, i) => `<div class="private-class-number-${i}">x</div>`).join('');
    fs.writeFileSync(path.join(t.dir, 'in', 'page.html'), `<html><body>${cards}</body></html>`);
    await assertBudgetStops(t, path.join(t.dir, 'in', 'page.html'), { maxUnknownClassTokens: 20 }, 'maxUnknownClassTokens');
  } finally { t.done(); }
});

test('SAN-F06: too many members, too many summary entries, and leak-table overflow stop the run safely', async () => {
  const t = scratch('san-f06-members');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    for (let i = 0; i < 40; i++) fs.writeFileSync(path.join(t.dir, 'in', `f${i}.json`), JSON.stringify({ [`field_${i}_a`]: { [`field_${i}_b`]: 1 }, [`field_${i}_c`]: [1, 2] }));
    await assertBudgetStops(t, path.join(t.dir, 'in'), { maxMembers: 10 }, 'maxMembers');
    await assertBudgetStops(t, path.join(t.dir, 'in'), { maxSummaryEntries: 30 }, 'maxSummaryEntries');
    await zipDirectory(path.join(t.dir, 'in'), path.join(t.dir, 'many.zip'));
    await assertBudgetStops(t, path.join(t.dir, 'many.zip'), { maxMembers: 10 }, 'maxMembers');
    // Leak-table overflow keeps its own, earlier exit code (3, E_LEAK_COVERAGE) and also stops at once.
    fs.writeFileSync(path.join(t.dir, 'in2.json'), JSON.stringify(Array.from({ length: 200 }, (_, i) => ({ text: `distinct sentence number ${i} with words` }))));
    fs.writeFileSync(path.join(t.dir, 'keep.txt'), 'x');
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in2.json'), output: path.join(t.dir, 'o2.json'), budgets: { maxProtectedPieces: 10 } }), { code: 'E_LEAK_COVERAGE', exitCode: 3 });
    assert.equal(fs.existsSync(path.join(t.dir, 'o2.json')), false);
  } finally { t.done(); }
});

test('SAN-F06: nesting depth and token size are bounded, so crafted input cannot exhaust memory', async () => {
  const bomb = '['.repeat(2_000_000); // 2 MB of opening brackets: unbounded frames would be ~2M objects
  const started = Date.now();
  await assert.rejects(collect(rewriteJson(bytesIn(bomb, 1 << 16), new Rules({ salt: 'k' }))), { code: 'E_JSON_TOO_DEEP' });
  assert.ok(Date.now() - started < 5000);
  await assert.rejects(collect(rewriteJson(bytesIn(`"${'a'.repeat(5000)}"`, 1024), new Rules({ salt: 'k' }), { budgets: { ...DEFAULT_BUDGETS, maxTokenChars: 2000 } })), { code: 'E_JSON_TOKEN_TOO_LARGE' });
  await assert.rejects(collect(rewriteJson(bytesIn(`[${'1'.repeat(5000)}]`, 1024), new Rules({ salt: 'k' }), { budgets: { ...DEFAULT_BUDGETS, maxTokenChars: 2000 } })), { code: 'E_JSON_TOKEN_TOO_LARGE' });
  await assert.rejects(collect(rewriteHtml(bytesIn(`<p>${'a'.repeat(5000)}`, 1024), new Rules({ salt: 'k' }), { budgets: { ...DEFAULT_BUDGETS, maxTokenChars: 2000 } })), { code: 'E_HTML_TOKEN_TOO_LARGE' });

  // At run level these are budget errors too: nothing published, previous output untouched.
  const t = scratch('san-f06-depth');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    fs.writeFileSync(path.join(t.dir, 'in', 'deep.json'), '['.repeat(1000) + ']'.repeat(1000));
    await assertBudgetStops(t, path.join(t.dir, 'in', 'deep.json'), { maxJsonDepth: 50 }, 'maxJsonDepth');
    fs.writeFileSync(path.join(t.dir, 'in', 'big.json'), JSON.stringify([`secret ${'x'.repeat(5000)}`]));
    await assertBudgetStops(t, path.join(t.dir, 'in', 'big.json'), { maxTokenChars: 1000 }, 'maxTokenChars');
  } finally { t.done(); }
});

test('SAN-F06: a high-cardinality export within the budgets completes and reports its limits', async () => {
  const t = scratch('san-f06-ok');
  try {
    fs.mkdirSync(path.join(t.dir, 'in'));
    const ids = Array.from({ length: 20_000 }, (_, i) => `00000000-0000-4000-8000-${String(i).padStart(12, '0')}`);
    fs.writeFileSync(path.join(t.dir, 'in', 'conversations.json'), JSON.stringify([{ id: ids[0], title: 't', current_node: ids[0], mapping: Object.fromEntries(ids.map(id => [id, { id, parent: null, children: [], message: null }])) }]));
    const result = await sanitize({ input: path.join(t.dir, 'in', 'conversations.json'), output: path.join(t.dir, 'out.json') });
    assert.equal(result.exitCode, 0);
    assert.equal(result.report.members[0].structure.distinctIdentifiers, 20_001 - 1 + 1 - 0 === 20_001 ? 20_000 : 0);
    assert.deepEqual(result.report.resourceBudgets.limits, DEFAULT_BUDGETS);
    assert.ok(result.report.resourceBudgets.worstCaseBytesIfAllReached < 1024 ** 3);
    assert.equal(runCli(['--help']).stdout.includes('5 resource budget exceeded'), true);
  } finally { t.done(); }
});
