// Synthetic (invented) ChatGPT-export and Gemini-Takeout shaped data for sanitizer tests.
// Every conversation embeds distinctive "private" strings; the builders return them so tests
// can assert none survives sanitization. Nothing here comes from a real export.
import fs from 'node:fs';
import path from 'node:path';
import { Readable } from 'node:stream';
import { ZipWriter } from '../../../scripts/import-sanitizer/zip.mjs';

export function seeded(seed) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

const WORDS = ['quartzfinch', 'marmalade', 'obsidian', 'lanternfly', 'wombatine', 'cassiopeia', 'tarragon', 'ziggurat',
  'nightjar', 'perihelion', 'vermicelli', 'saffronwood', 'harbinger', 'tundrawolf', 'moonstone'];

export function uuid(rand) {
  const hex = n => Array.from({ length: n }, () => Math.floor(rand() * 16).toString(16)).join('');
  return `${hex(8)}-${hex(4)}-4${hex(3)}-${'89ab'[Math.floor(rand() * 4)]}${hex(3)}-${hex(12)}`;
}

function sentence(rand, secrets, tag) {
  const picked = Array.from({ length: 3 + Math.floor(rand() * 5) }, () => WORDS[Math.floor(rand() * WORDS.length)]);
  const marker = `${picked[0]}${tag}-${Math.floor(rand() * 1e6)}`;
  secrets.add(marker);
  const email = `person.${marker}@privatemail.test`;
  secrets.add(email);
  return `Please remind ${marker} about ${picked.join(' ')} and write to ${email} at https://private-${marker}.example.net/deep/secret/path?token=${marker}`;
}

export function chatgptConversation(rand, secrets, { quirks = [] } = {}) {
  const id = uuid(rand);
  const root = quirks.includes('literal-root') ? 'client-created-root' : uuid(rand);
  const mapping = {};
  const nodes = [root];
  mapping[root] = { id: root, message: null, parent: null, children: [] };
  let parent = root;
  const messageCount = 4 + Math.floor(rand() * 4);
  for (let i = 0; i < messageCount; i++) {
    const nodeId = uuid(rand);
    const role = i === 0 ? 'system' : i % 2 ? 'user' : 'assistant';
    const text = sentence(rand, secrets, `m${i}`);
    const content = i === 0
      ? { content_type: 'text', parts: [''] }
      : quirks.includes('tool') && i === 2
        ? { content_type: 'code', language: 'python', text: `print("${text}")` }
        : { content_type: 'text', parts: [text] };
    if (quirks.includes('image') && i === 1) {
      const file = `file-${Math.floor(rand() * 1e12).toString(36).padStart(10, 'a')}`;
      secrets.add(file);
      content.content_type = 'multimodal_text';
      content.parts = [{ content_type: 'image_asset_pointer', asset_pointer: `file-service://${file}`, size_bytes: 1234, width: 10, height: 10 }, text];
    }
    mapping[nodeId] = {
      id: nodeId,
      parent,
      children: [],
      message: {
        id: quirks.includes('duplicate-id') && i === 3 ? mapping[parent]?.message?.id ?? nodeId : nodeId,
        author: { role, name: role === 'tool' ? 'browser' : null, metadata: {} },
        create_time: 1700000000 + i * 60 + rand(),
        update_time: null,
        content,
        status: 'finished_successfully',
        end_turn: true,
        weight: 1,
        metadata: { model_slug: 'gpt-4o', attachments: quirks.includes('attachment') && i === 1 ? [{ id: `file-${i}secretdoc000`, name: `Quarterly-${WORDS[i]}-plan.pdf`, size: 99 }] : undefined },
        recipient: 'all',
      },
    };
    mapping[parent].children.push(nodeId);
    nodes.push(nodeId);
    parent = nodeId;
  }
  if (quirks.includes('branch')) {
    const alt = uuid(rand);
    mapping[alt] = { id: alt, parent: nodes[2], children: [], message: { id: alt, author: { role: 'assistant', metadata: {} }, create_time: 1700000999.5, content: { content_type: 'text', parts: [sentence(rand, secrets, 'alt')] }, status: 'finished_successfully', metadata: {}, recipient: 'all' } };
    mapping[nodes[2]].children.push(alt);
  }
  if (quirks.includes('orphan')) {
    const orphan = uuid(rand);
    mapping[orphan] = { id: orphan, parent: 'missing-parent-node', children: [], message: null };
  }
  if (quirks.includes('empty-parts')) mapping[nodes[nodes.length - 1]].message.content.parts = [];
  const title = `Notes about ${WORDS[Math.floor(rand() * WORDS.length)]} ${Math.floor(rand() * 1e5)}`;
  secrets.add(title);
  return {
    id, title, create_time: 1700000000.123456, update_time: 1700000900.25,
    mapping, current_node: parent, conversation_id: id, default_model_slug: 'gpt-4o',
    plugin_ids: null, is_archived: false, safe_urls: [`https://private-${id}.example.net/x`], moderation_results: [],
    ...(quirks.includes('unknown-field') ? { experimental_field_7: { nested: true } } : {}),
  };
}

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64');

/** Writes a loose ChatGPT export into `dir`. `multi` splits conversations over several members. */
export function writeChatGptExport(dir, { seed = 1, conversations = 6, multi = true, pretty = false } = {}) {
  const rand = seeded(seed);
  const secrets = new Set();
  const quirkSets = [['literal-root'], ['image', 'attachment'], ['tool', 'branch'], ['duplicate-id', 'orphan'], ['empty-parts', 'unknown-field'], []];
  const all = Array.from({ length: conversations }, (_, i) => chatgptConversation(rand, secrets, { quirks: quirkSets[i % quirkSets.length] }));
  const encode = value => JSON.stringify(value, null, pretty ? 2 : 0);
  fs.mkdirSync(dir, { recursive: true });
  const files = [];
  if (multi) {
    const half = Math.ceil(all.length / 2);
    fs.writeFileSync(path.join(dir, 'conversations-000.json'), encode(all.slice(0, half)));
    fs.writeFileSync(path.join(dir, 'conversations-001.json'), encode(all.slice(half)));
    files.push('conversations-000.json', 'conversations-001.json');
  } else {
    fs.writeFileSync(path.join(dir, 'conversations.json'), encode(all));
    files.push('conversations.json');
  }
  const email = `owner.${seed}@privatemail.test`;
  secrets.add(email);
  fs.writeFileSync(path.join(dir, 'user.json'), encode({ id: `user-${uuid(rand)}`, email, chatgpt_plus_user: true, phone_number: '+15551234567' }));
  secrets.add('+15551234567');
  fs.writeFileSync(path.join(dir, 'message_feedback.json'), encode([{ id: uuid(rand), conversation_id: all[0].id, rating: 'thumbs_up', create_time: '2023-11-14T22:13:20.123456+00:00', content: `{"text": "${sentence(rand, secrets, 'fb')}"}` }]));
  fs.writeFileSync(path.join(dir, 'chat.html'), `<!doctype html><html><head><title>${WORDS[0]} chats</title><script>var jsonData = ${encode(all.slice(0, 1))};</script></head><body><div class="conversation"><div class="message"><p>${sentence(rand, secrets, 'html')}</p></div></div></body></html>`);
  const mediaName = `file-${'x'.repeat(4)}${Math.floor(rand() * 1e9)}-vacation_with_${WORDS[3]}.png`;
  secrets.add(mediaName);
  fs.writeFileSync(path.join(dir, mediaName), Buffer.concat([PNG, Buffer.from(`PRIVATE-PIXELS-${WORDS[5]}`.repeat(50))]));
  fs.mkdirSync(path.join(dir, 'dalle-generations'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'dalle-generations', `file-${Math.floor(rand() * 1e9)}-${WORDS[6]}-art.webp`), Buffer.from('RIFF....WEBPprivate-art'));
  secrets.add(WORDS[6]);
  return { secrets, files, conversations: all };
}

/** Writes a Gemini Takeout style tree into `dir`. */
export function writeGeminiTakeout(dir, { seed = 7, cards = 5 } = {}) {
  const rand = seeded(seed);
  const secrets = new Set();
  const out = [];
  for (let i = 0; i < cards; i++) {
    const id = `c_${Math.floor(rand() * 1e15).toString(36)}${i}abc`;
    secrets.add(id);
    const prompt = sentence(rand, secrets, `p${i}`);
    const answer = sentence(rand, secrets, `a${i}`);
    const stamp = `${4 + i} Nov 2026, 21:${10 + i}:40 WIB`;
    if (i === 2) {
      // response-only card, single-quoted attributes, unquoted attribute and an uppercase tag
      out.push(`<DIV class='outer-cell mdl-cell'><div class=content-cell>${stamp}<br><p>${answer}</p></div></DIV>`);
    } else if (i === 3) {
      // two prompts in peer content cells, plus an entity and a comment
      out.push(`<div class="outer-cell"><!-- ${prompt} --><div class="content-cell">Prompted ${prompt}<br>${stamp}<br><p>${answer} &amp; more</p></div><div class="content-cell">Prompted ${sentence(rand, secrets, 'q')}<br><p>${sentence(rand, secrets, 'r')}</p></div></div>`);
    } else {
      out.push(`<div class="outer-cell mdl-cell mdl-cell--12-col mdl-shadow--2dp"><div class="content-cell mdl-cell mdl-cell--6-col mdl-typography--body-1">Prompted ${prompt}<br>${stamp}<br><p>${answer}</p><img src="data:image/png;base64,${PNG.toString('base64')}"></div><div class="content-cell mdl-typography--caption"><b>Details:</b> https://gemini.google.com/app/${id}: <a href="https://gemini.google.com/app/${id}" title="${prompt}">Link</a></div></div>`);
    }
  }
  // The same card twice, to keep duplicate-card occurrences.
  out.push(out[0]);
  const html = `<!doctype html><html><head><meta charset="utf-8"><title>My Activity</title><style>.outer-cell{margin:1em}</style></head><body>${out.join('\n')}</body></html>`;
  const activity = path.join(dir, 'Takeout', 'My Activity', 'Gemini Apps');
  fs.mkdirSync(activity, { recursive: true });
  fs.writeFileSync(path.join(activity, 'MyActivity.html'), html);
  fs.writeFileSync(path.join(dir, 'Takeout', 'archive_browser.html'), '<html><body><a href="index.html">Takeout</a></body></html>');
  fs.writeFileSync(path.join(activity, `${WORDS[2]}-attachment-${seed}.jpg`), Buffer.from('\xff\xd8\xff private jpeg bytes'));
  secrets.add(`${WORDS[2]}-attachment-${seed}`);
  return { secrets, html };
}

export function readTree(dir) {
  const out = new Map();
  (function walk(current) {
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const file = path.join(current, entry.name);
      if (entry.isDirectory()) walk(file);
      else out.set(path.relative(dir, file).split(path.sep).join('/'), fs.readFileSync(file));
    }
  })(dir);
  return out;
}

export async function zipDirectory(dir, zipFile) {
  const writer = new ZipWriter(zipFile);
  for (const [name, bytes] of [...readTree(dir)].sort(([a], [b]) => (a < b ? -1 : 1))) {
    await writer.addFile(name, Readable.from([bytes]));
  }
  return writer.finish();
}
