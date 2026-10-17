import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { initPopup } from '../extension/popup.mjs';

function fixture(grant = true) {
  const dom = new JSDOM('<button id="save">Save</button><p id="status"></p><input type="checkbox" id="auto-save"><p id="auto-status"></p>');
  let requests = 0, enabled = false, eventType = '';
  for (const type of ['click', 'change']) dom.window.document.addEventListener(type, () => { eventType = type; }, true);
  const chromeApi = { runtime: { sendMessage: async message => {
    if (message.type === 'set-auto-saving') enabled = message.enabled;
    return { enabled };
  } }, permissions: { request: async value => { requests++; assert.equal(eventType, 'click', 'origin permission must be requested directly in the click gesture'); assert.deepEqual(value.origins, ['https://chatgpt.com/*']); if (grant instanceof Error) throw grant; return grant; } } };
  const controller = initPopup({ chromeApi, documentRef: dom.window.document });
  return { dom, controller, toggle: dom.window.document.querySelector('#auto-save'), get requests() { return requests; }, get enabled() { return enabled; } };
}

test('popup opening never requests origin access; only the automatic toggle does', async () => {
  const fake = fixture();
  try {
    await fake.controller.whenIdle(); assert.equal(fake.requests, 0); assert.equal(fake.toggle.checked, false);
    fake.toggle.click();
    await fake.controller.whenIdle(); assert.equal(fake.requests, 1); assert.equal(fake.enabled, true);
    fake.toggle.click();
    await fake.controller.whenIdle(); assert.equal(fake.requests, 1); assert.equal(fake.enabled, false);
  } finally { fake.controller.destroy(); fake.dom.window.close(); }
});

test('declined ChatGPT permission keeps automatic capture off', async () => {
  const fake = fixture(false);
  try {
    await fake.controller.whenIdle(); fake.toggle.click(); await fake.controller.whenIdle();
    assert.equal(fake.enabled, false); assert.equal(fake.toggle.checked, false);
    assert.match(fake.dom.window.document.querySelector('#auto-status').textContent, /not granted/i);
  } finally { fake.controller.destroy(); fake.dom.window.close(); }
});

test('activation errors display the actual Chrome permission error and restore the OFF toggle', async () => {
  const fake = fixture(new Error('Permission request rejected: user gesture required'));
  try {
    await fake.controller.whenIdle(); fake.toggle.click(); await fake.controller.whenIdle();
    assert.equal(fake.toggle.checked, false);
    assert.match(fake.dom.window.document.querySelector('#auto-status').textContent, /user gesture required/);
  } finally { fake.controller.destroy(); fake.dom.window.close(); }
});
