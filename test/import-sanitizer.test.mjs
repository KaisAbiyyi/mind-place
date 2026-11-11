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
import { LeakRegistry } from '../scripts/import-sanitizer/leak-check.mjs';
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
  const whole = await collect(rewriteJson(bytesIn(source, 1 << 20), new Rules(), { multiRoot: false }));
  for (const size of [1, 2, 3, 7, 64]) {
    assert.equal(await collect(rewriteJson(bytesIn(source, size), new Rules())), whole, `chunk size ${size}`);
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
  const html = '<!doctype html><DIV class=\'a b\' data-x=unquoted hidden><p>1 < 2 and a&amp;b</p><!-- note: Zanzibar --><br/><script>if (a<b) {document.write("<div>")}</script><a href="https://example.org/private">go</a></DIV>';
  const whole = await collect(rewriteHtml(bytesIn(html, 1 << 20), new Rules()));
  for (const size of [1, 2, 5, 11]) assert.equal(await collect(rewriteHtml(bytesIn(html, size), new Rules())), whole, `chunk size ${size}`);
  assert.ok(whole.includes("<DIV class='a b' data-x=") && whole.includes(' hidden>'), 'attribute quoting and valueless attributes are kept');
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

test('output is deterministic: two runs give byte-identical zips and reports', async () => {
  const t = scratch('determinism');
  try {
    writeChatGptExport(path.join(t.dir, 'in'), { seed: 9 });
    await zipDirectory(path.join(t.dir, 'in'), path.join(t.dir, 'e.zip'));
    const a = await sanitize({ input: path.join(t.dir, 'e.zip'), output: path.join(t.dir, 'a.zip') });
    const b = await sanitize({ input: path.join(t.dir, 'e.zip'), output: path.join(t.dir, 'b.zip') });
    assert.ok(fs.readFileSync(path.join(t.dir, 'a.zip')).equals(fs.readFileSync(path.join(t.dir, 'b.zip'))));
    assert.deepEqual(a.report, b.report);
    const salted = await sanitize({ input: path.join(t.dir, 'e.zip'), output: path.join(t.dir, 'c.zip'), salt: 'another-key' });
    assert.ok(!fs.readFileSync(path.join(t.dir, 'a.zip')).equals(fs.readFileSync(path.join(t.dir, 'c.zip'))), 'a different key gives different pseudonyms');
    assert.equal(salted.report.options.salt, 'custom');
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
    await assert.rejects(sanitize({ input: path.join(t.dir, 'in'), output: path.join(t.dir, 'in', 'out') }), { code: 'E_OUTPUT_INSIDE_INPUT' });
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
    assert.match(run.stdout, /Leak check: PASS/);
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
    const kept = result.report.keptValues.byKey;
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
